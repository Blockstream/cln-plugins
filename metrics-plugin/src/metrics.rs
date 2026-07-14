use crate::cache::CachedData;
use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use prometheus::{
    Encoder, Gauge, GaugeVec, IntCounter, IntCounterVec, Opts, TextEncoder,
    core::{Collector, Desc},
    proto::MetricFamily,
};
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tracing::error;

const CHANNEL_LABELS: &[&str] = &["peer_id", "scid"];
const PEER_LABELS: &[&str] = &["peer_id"];

/// Registers with the global registry while returning a caller-owned handle.
/// Clone happens before boxing because 'register' takes ownership via 'Box<dyn Collector>'.
fn reg<T: Collector + Clone + 'static>(c: T) -> prometheus::Result<T> {
    prometheus::register(Box::new(c.clone()))?;
    Ok(c)
}

/// Metrics describing the health of the periodic refresh loop itself, so that
/// operators can alert on stale gauges or persistent fetch failures.
#[derive(Clone)]
pub struct RefreshHealth {
    pub last_refresh_timestamp_seconds: Gauge,
    pub refresh_failures_total: IntCounter,
}

impl RefreshHealth {
    pub fn new(node_id: &str) -> prometheus::Result<Self> {
        let o = |name: &str, help: &str| Opts::new(name, help).const_label("node_id", node_id);
        Ok(Self {
            last_refresh_timestamp_seconds: reg(Gauge::with_opts(o(
                "lightning_metrics_last_refresh_timestamp_seconds",
                "Unix timestamp of the most recent successful CLN data refresh.",
            ))?)?,
            refresh_failures_total: reg(IntCounter::with_opts(o(
                "lightning_metrics_refresh_failures_total",
                "Total number of refresh attempts that failed since plugin start.",
            ))?)?,
        })
    }
}

/// Event-driven counters mutated atomically by subscription/hook handlers.
#[derive(Clone)]
pub struct EventCounters {
    pub forward_events_total: IntCounterVec,
    pub channel_opened_total: IntCounter,
    pub htlc_accepted_total: IntCounter,
    pub channel_state_changes_total: IntCounterVec,
}

impl EventCounters {
    pub fn new(node_id: &str) -> prometheus::Result<Self> {
        let o = |name: &str, help: &str| Opts::new(name, help).const_label("node_id", node_id);
        Ok(Self {
            forward_events_total: reg(IntCounterVec::new(
                o(
                    "lightning_forward_events_total",
                    "Payment forwarding events observed by this node.",
                ),
                &["status"],
            )?)?,
            channel_opened_total: reg(IntCounter::with_opts(o(
                "lightning_channel_opened_total",
                "Total number of channels opened since plugin start.",
            ))?)?,
            htlc_accepted_total: reg(IntCounter::with_opts(o(
                "lightning_htlc_accepted_total",
                "Total number of HTLCs accepted since plugin start.",
            ))?)?,
            channel_state_changes_total: reg(IntCounterVec::new(
                o(
                    "lightning_channel_state_changes_total",
                    "Channel state change events.",
                ),
                &["old_state", "new_state"],
            )?)?,
        })
    }
}

/// Registers the build_info gauge with the global registry and pins it to 1.0.
/// Prometheus info-metric pattern - the gauge is constant and the version lives in the label.
pub fn register_build_info(node_id: &str, version: &str) -> prometheus::Result<()> {
    let opts =
        Opts::new("lightning_build_info", "Plugin version info.").const_label("node_id", node_id);
    let gv = GaugeVec::new(opts, &["version"])?;
    prometheus::register(Box::new(gv.clone()))?;
    gv.with_label_values(&[version]).set(1.0);
    Ok(())
}

/// Custom Prometheus collector that builds derived gauges from a cached snapshot.
/// All metrics for one scrape come from a single cache.read(), so a scrape never
/// observes torn or partial state across the gauge set.
pub struct DerivedCollector {
    cache: Arc<RwLock<CachedData>>,
    descs: Vec<Desc>,

    // Scalar gauges
    node_blockheight: Opts,
    fees_collected_msat: Opts,
    funds_output_msat: Opts,
    funds_channel_msat: Opts,
    funds_total_msat: Opts,
    total_outbound_msat: Opts,
    total_inbound_msat: Opts,
    peers: Opts,
    channels: Opts,

    // Per-channel GaugeVecs (labels: peer_id, scid)
    channel_balance_msat: Opts,
    channel_spendable_msat: Opts,
    channel_receivable_msat: Opts,
    channel_capacity_msat: Opts,
    channel_htlcs: Opts,
    channel_in_payments_offered: Opts,
    channel_in_payments_fulfilled: Opts,
    channel_in_msat_offered: Opts,
    channel_in_msat_fulfilled: Opts,
    channel_out_payments_offered: Opts,
    channel_out_payments_fulfilled: Opts,
    channel_out_msat_offered: Opts,
    channel_out_msat_fulfilled: Opts,

    // Per-peer GaugeVecs (label: peer_id)
    peer_connected: Opts,
    peer_channels: Opts,
    peer_channel_connected: Opts,
}

impl DerivedCollector {
    pub fn new(node_id: &str, cache: Arc<RwLock<CachedData>>) -> prometheus::Result<Self> {
        let o = |name: &str, help: &str| Opts::new(name, help).const_label("node_id", node_id);

        let node_blockheight = o(
            "lightning_node_blockheight",
            "Current Bitcoin blockheight on this node.",
        );
        let fees_collected_msat = o(
            "lightning_fees_collected_msat",
            "Total routing fees collected by this node in millisatoshis.",
        );
        let funds_output_msat = o(
            "lightning_funds_output_msat",
            "On-chain millisatoshis available to the node.",
        );
        let funds_channel_msat = o(
            "lightning_funds_channel_msat",
            "Millisatoshis held in payment channels (our side).",
        );
        let funds_total_msat = o(
            "lightning_funds_total_msat",
            "Total millisatoshis owned by this node (on-chain + channels).",
        );
        let total_outbound_msat = o(
            "lightning_total_outbound_msat",
            "Total spendable millisatoshis across all channels (outbound liquidity).",
        );
        let total_inbound_msat = o(
            "lightning_total_inbound_msat",
            "Total receivable millisatoshis across all channels (inbound liquidity).",
        );
        let peers = o(
            "lightning_peers",
            "Total number of peers with at least one channel.",
        );
        let channels = o("lightning_channels", "Total number of peer channels.");

        let channel_balance_msat = o(
            "lightning_channel_balance_msat",
            "Our balance in millisatoshis for this channel.",
        );
        let channel_spendable_msat = o(
            "lightning_channel_spendable_msat",
            "Currently spendable millisatoshis on this channel (outbound).",
        );
        let channel_receivable_msat = o(
            "lightning_channel_receivable_msat",
            "Currently receivable millisatoshis on this channel (inbound).",
        );
        let channel_capacity_msat = o(
            "lightning_channel_capacity_msat",
            "Total capacity of this channel in millisatoshis.",
        );
        let channel_htlcs = o(
            "lightning_channel_htlcs",
            "Number of active HTLCs on this channel.",
        );
        let channel_in_payments_offered = o(
            "lightning_channel_in_payments_offered_total",
            "Incoming payments offered for forwarding through this channel.",
        );
        let channel_in_payments_fulfilled = o(
            "lightning_channel_in_payments_fulfilled_total",
            "Incoming payments successfully forwarded through this channel.",
        );
        let channel_in_msat_offered = o(
            "lightning_channel_in_msat_offered_total",
            "Incoming millisatoshis offered for forwarding through this channel.",
        );
        let channel_in_msat_fulfilled = o(
            "lightning_channel_in_msat_fulfilled_total",
            "Incoming millisatoshis successfully forwarded through this channel.",
        );
        let channel_out_payments_offered = o(
            "lightning_channel_out_payments_offered_total",
            "Outgoing payments offered for forwarding through this channel.",
        );
        let channel_out_payments_fulfilled = o(
            "lightning_channel_out_payments_fulfilled_total",
            "Outgoing payments successfully forwarded through this channel.",
        );
        let channel_out_msat_offered = o(
            "lightning_channel_out_msat_offered_total",
            "Outgoing millisatoshis offered for forwarding through this channel.",
        );
        let channel_out_msat_fulfilled = o(
            "lightning_channel_out_msat_fulfilled_total",
            "Outgoing millisatoshis successfully forwarded through this channel.",
        );

        let peer_connected = o(
            "lightning_peer_connected",
            "Is the peer currently connected? (1=yes, 0=no)",
        );
        let peer_channels = o(
            "lightning_peer_channels",
            "Number of channels with this peer.",
        );
        let peer_channel_connected = o(
            "lightning_peer_channel_connected",
            "Number of connected channels with this peer.",
        );

        let mut descs = Vec::new();
        for opts in [
            &node_blockheight,
            &fees_collected_msat,
            &funds_output_msat,
            &funds_channel_msat,
            &funds_total_msat,
            &total_outbound_msat,
            &total_inbound_msat,
            &peers,
            &channels,
        ] {
            let g = Gauge::with_opts(opts.clone())?;
            descs.extend(g.desc().into_iter().cloned());
        }
        for opts in [
            &channel_balance_msat,
            &channel_spendable_msat,
            &channel_receivable_msat,
            &channel_capacity_msat,
            &channel_htlcs,
        ] {
            let gv = GaugeVec::new(opts.clone(), CHANNEL_LABELS)?;
            descs.extend(gv.desc().into_iter().cloned());
        }
        for opts in [
            &channel_in_payments_offered,
            &channel_in_payments_fulfilled,
            &channel_in_msat_offered,
            &channel_in_msat_fulfilled,
            &channel_out_payments_offered,
            &channel_out_payments_fulfilled,
            &channel_out_msat_offered,
            &channel_out_msat_fulfilled,
        ] {
            let cv = IntCounterVec::new(opts.clone(), CHANNEL_LABELS)?;
            descs.extend(cv.desc().into_iter().cloned());
        }
        for opts in [&peer_connected, &peer_channels, &peer_channel_connected] {
            let gv = GaugeVec::new(opts.clone(), PEER_LABELS)?;
            descs.extend(gv.desc().into_iter().cloned());
        }

        Ok(Self {
            cache,
            descs,
            node_blockheight,
            fees_collected_msat,
            funds_output_msat,
            funds_channel_msat,
            funds_total_msat,
            total_outbound_msat,
            total_inbound_msat,
            peers,
            channels,
            channel_balance_msat,
            channel_spendable_msat,
            channel_receivable_msat,
            channel_capacity_msat,
            channel_htlcs,
            channel_in_payments_offered,
            channel_in_payments_fulfilled,
            channel_in_msat_offered,
            channel_in_msat_fulfilled,
            channel_out_payments_offered,
            channel_out_payments_fulfilled,
            channel_out_msat_offered,
            channel_out_msat_fulfilled,
            peer_connected,
            peer_channels,
            peer_channel_connected,
        })
    }
}

impl Collector for DerivedCollector {
    fn desc(&self) -> Vec<&Desc> {
        self.descs.iter().collect()
    }

    fn collect(&self) -> Vec<MetricFamily> {
        let cache = self.cache.read().expect("cache lock poisoned");
        let mut out = Vec::with_capacity(26);

        out.extend(scalar(
            &self.node_blockheight,
            cache.node.blockheight as f64,
        ));

        out.extend(scalar(
            &self.fees_collected_msat,
            cache.node.fees_collected_msat as f64,
        ));
        out.extend(scalar(
            &self.funds_output_msat,
            cache.funds.output_msat as f64,
        ));
        out.extend(scalar(
            &self.funds_channel_msat,
            cache.funds.channel_msat as f64,
        ));
        out.extend(scalar(
            &self.funds_total_msat,
            (cache.funds.output_msat + cache.funds.channel_msat) as f64,
        ));

        let total_outbound: i64 = cache.channels.iter().map(|c| c.spendable_msat).sum();
        let total_inbound: i64 = cache.channels.iter().map(|c| c.receivable_msat).sum();
        out.extend(scalar(&self.total_outbound_msat, total_outbound as f64));
        out.extend(scalar(&self.total_inbound_msat, total_inbound as f64));

        out.extend(scalar(&self.peers, cache.peers.len() as f64));
        out.extend(scalar(&self.channels, cache.channels.len() as f64));

        let balance = GaugeVec::new(self.channel_balance_msat.clone(), CHANNEL_LABELS).unwrap();
        let spendable = GaugeVec::new(self.channel_spendable_msat.clone(), CHANNEL_LABELS).unwrap();
        let receivable =
            GaugeVec::new(self.channel_receivable_msat.clone(), CHANNEL_LABELS).unwrap();
        let capacity = GaugeVec::new(self.channel_capacity_msat.clone(), CHANNEL_LABELS).unwrap();
        let htlcs = GaugeVec::new(self.channel_htlcs.clone(), CHANNEL_LABELS).unwrap();
        let in_payments_offered =
            IntCounterVec::new(self.channel_in_payments_offered.clone(), CHANNEL_LABELS).unwrap();
        let in_payments_fulfilled =
            IntCounterVec::new(self.channel_in_payments_fulfilled.clone(), CHANNEL_LABELS).unwrap();
        let in_msat_offered =
            IntCounterVec::new(self.channel_in_msat_offered.clone(), CHANNEL_LABELS).unwrap();
        let in_msat_fulfilled =
            IntCounterVec::new(self.channel_in_msat_fulfilled.clone(), CHANNEL_LABELS).unwrap();
        let out_payments_offered =
            IntCounterVec::new(self.channel_out_payments_offered.clone(), CHANNEL_LABELS).unwrap();
        let out_payments_fulfilled =
            IntCounterVec::new(self.channel_out_payments_fulfilled.clone(), CHANNEL_LABELS)
                .unwrap();
        let out_msat_offered =
            IntCounterVec::new(self.channel_out_msat_offered.clone(), CHANNEL_LABELS).unwrap();
        let out_msat_fulfilled =
            IntCounterVec::new(self.channel_out_msat_fulfilled.clone(), CHANNEL_LABELS).unwrap();

        for ch in &cache.channels {
            let labels = [ch.peer_id.as_str(), ch.scid.as_str()];
            balance
                .with_label_values(&labels)
                .set(ch.balance_msat as f64);
            spendable
                .with_label_values(&labels)
                .set(ch.spendable_msat as f64);
            receivable
                .with_label_values(&labels)
                .set(ch.receivable_msat as f64);
            capacity
                .with_label_values(&labels)
                .set(ch.capacity_msat as f64);
            htlcs.with_label_values(&labels).set(ch.htlcs as f64);
            // Counters: each scrape constructs a fresh IntCounterVec at zero, so inc_by(absolute)
            // lands the metric at CLN's cumulative value. Closed channels disappear from the next
            // snapshot, which Prometheus interprets as a counter reset (correct).
            in_payments_offered
                .with_label_values(&labels)
                .inc_by(ch.in_payments_offered as u64);
            in_payments_fulfilled
                .with_label_values(&labels)
                .inc_by(ch.in_payments_fulfilled as u64);
            in_msat_offered
                .with_label_values(&labels)
                .inc_by(ch.in_msat_offered as u64);
            in_msat_fulfilled
                .with_label_values(&labels)
                .inc_by(ch.in_msat_fulfilled as u64);
            out_payments_offered
                .with_label_values(&labels)
                .inc_by(ch.out_payments_offered as u64);
            out_payments_fulfilled
                .with_label_values(&labels)
                .inc_by(ch.out_payments_fulfilled as u64);
            out_msat_offered
                .with_label_values(&labels)
                .inc_by(ch.out_msat_offered as u64);
            out_msat_fulfilled
                .with_label_values(&labels)
                .inc_by(ch.out_msat_fulfilled as u64);
        }

        out.extend(balance.collect());
        out.extend(spendable.collect());
        out.extend(receivable.collect());
        out.extend(capacity.collect());
        out.extend(htlcs.collect());
        out.extend(in_payments_offered.collect());
        out.extend(in_payments_fulfilled.collect());
        out.extend(in_msat_offered.collect());
        out.extend(in_msat_fulfilled.collect());
        out.extend(out_payments_offered.collect());
        out.extend(out_payments_fulfilled.collect());
        out.extend(out_msat_offered.collect());
        out.extend(out_msat_fulfilled.collect());

        let peer_connected = GaugeVec::new(self.peer_connected.clone(), PEER_LABELS).unwrap();
        let peer_channels = GaugeVec::new(self.peer_channels.clone(), PEER_LABELS).unwrap();
        let peer_channel_connected =
            GaugeVec::new(self.peer_channel_connected.clone(), PEER_LABELS).unwrap();

        for p in &cache.peers {
            let labels = [p.id.as_str()];
            peer_connected
                .with_label_values(&labels)
                .set(if p.connected { 1.0 } else { 0.0 });
            peer_channels
                .with_label_values(&labels)
                .set(p.num_channels as f64);
            peer_channel_connected
                .with_label_values(&labels)
                .set(p.connected_channels as f64);
        }

        out.extend(peer_connected.collect());
        out.extend(peer_channels.collect());
        out.extend(peer_channel_connected.collect());

        out
    }
}

fn scalar(opts: &Opts, value: f64) -> Vec<MetricFamily> {
    let g = Gauge::with_opts(opts.clone()).unwrap();
    g.set(value);
    g.collect()
}

async fn metrics_handler() -> Response {
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&prometheus::gather(), &mut buffer) {
        error!("failed to encode metrics: {}", e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    (
        [(
            axum::http::header::CONTENT_TYPE,
            encoder.format_type().to_owned(),
        )],
        buffer,
    )
        .into_response()
}

pub async fn start_metrics_server(listener: TcpListener) -> anyhow::Result<()> {
    let app = Router::new().route("/metrics", get(metrics_handler));
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!("metrics server error: {}", e);
        }
    });
    Ok(())
}
