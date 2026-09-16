mod cache;
mod events;
mod metrics;

use anyhow::Result;
use cln_plugin::{Builder, Plugin, options::ConfigOption};
use cln_rpc::ClnRpc;
use cln_rpc::model::requests::GetinfoRequest;
use serde_json::Value;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

use crate::cache::CachedData;
use crate::metrics::RefreshHealth;
use crate::metrics::{DerivedCollector, EventCounters};

const DEFAULT_REFRESH_SECS: u64 = 30;

const OPT_METRICS_ADDR: ConfigOption<'static, cln_plugin::options::config_type::String> =
    ConfigOption::new_str_no_default(
        "metrics-addr",
        "Address and port to expose the Prometheus /metrics endpoint on",
    );

const OPT_REFRESH_SECS: ConfigOption<'static, cln_plugin::options::config_type::DefaultInteger> =
    ConfigOption::new_i64_with_default(
        "metrics-refresh",
        DEFAULT_REFRESH_SECS as i64,
        "How often (seconds) to refresh CLN node data for gauge metrics",
    );

const OPT_METRICS_TOKEN_FILE: ConfigOption<'static, cln_plugin::options::config_type::String> =
    ConfigOption::new_str_no_default(
        "metrics-token-file",
        "Path to a file containing the bearer token required to access the /metrics endpoint",
    );

const OPT_METRICS_DISABLE_AUTH: ConfigOption<
    'static,
    cln_plugin::options::config_type::DefaultBoolean,
> = ConfigOption::new_bool_with_default(
    "metrics-disable-auth",
    false,
    "Serve /metrics without authentication (INSECURE - local/dev use only)",
);

/// Shared plugin state - event counters and a read-cache of the last CLN poll.
/// Event counters are mutated atomically from subscription/hook handlers; the cache
/// is rebuilt by the refresh loop and read by both 'rpc_status' and the metrics
/// scrape collector.
#[derive(Clone)]
pub struct PluginState {
    pub counters: EventCounters,
    pub cache: Arc<RwLock<CachedData>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let configured = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(OPT_METRICS_ADDR)
        .option(OPT_REFRESH_SECS)
        .option(OPT_METRICS_TOKEN_FILE)
        .option(OPT_METRICS_DISABLE_AUTH)
        .subscribe("channel_opened", events::on_channel_opened)
        .subscribe("forward_event", events::on_forward_event)
        .subscribe("channel_state_changed", events::on_channel_state_changed)
        .hook("htlc_accepted", events::hook_htlc_accepted)
        .rpcmethod("metrics-status", "Get metrics plugin status", rpc_status)
        .dynamic()
        .configure()
        .await?;

    let Some(configured) = configured else {
        return Ok(());
    };

    let addr: SocketAddr = configured
        .option(&OPT_METRICS_ADDR)?
        .ok_or_else(|| anyhow::anyhow!("metrics-addr option is required"))?
        .parse()?;

    let refresh_secs = configured
        .option(&OPT_REFRESH_SECS)
        .unwrap_or(DEFAULT_REFRESH_SECS as i64)
        .max(5) as u64; // floor at 5s to avoid hammering CLN RPC

    let metrics_token_file = configured.option(&OPT_METRICS_TOKEN_FILE)?;
    let disable_auth = configured.option(&OPT_METRICS_DISABLE_AUTH)?;
    let auth_token: Option<Arc<str>> = match (metrics_token_file, disable_auth) {
        (Some(path), _) => Some(read_token_file(&path).await?),
        (None, true) => {
            warn!("metrics-disable-auth is set: /metrics is being served WITHOUT authentication");
            None
        }
        (None, false) => {
            return Err(anyhow::anyhow!(
                "metrics-token-file is required to protect /metrics; set --metrics-disable-auth to run without authentication"
            ));
        }
    };

    let socket_path = PathBuf::from(configured.configuration().lightning_dir)
        .join(configured.configuration().rpc_file.clone());

    let mut rpc = ClnRpc::new(&socket_path).await?;
    let node_info = rpc.call_typed(&GetinfoRequest {}).await?;
    let node_id = node_info.id.to_string();

    let counters = EventCounters::new(&node_id)?;
    let health = RefreshHealth::new(&node_id)?;
    let cache: Arc<RwLock<CachedData>> = Arc::new(RwLock::new(CachedData::default()));

    metrics::register_build_info(&node_id, env!("CARGO_PKG_VERSION"))?;
    let derived = DerivedCollector::new(&node_id, cache.clone())?;
    prometheus::register(Box::new(derived))?;

    let state = PluginState {
        counters,
        cache: cache.clone(),
    };

    info!(%addr, refresh_secs, ?socket_path, %node_id, "Metrics plugin configured");

    let listener = TcpListener::bind(addr).await?;
    let plugin = configured.start(state).await?;

    metrics::start_metrics_server(listener, auth_token).await?;

    spawn_refresh_loop(cache, socket_path, refresh_secs, health);

    info!("Metrics plugin started");
    plugin.join().await?;
    Ok(())
}

/// Gauge metrics reflect current node state, not individual events, so we poll CLN periodically
/// instead of trying to update them from event handlers that lack full node context.
/// The DerivedCollector reads from this cache at scrape time, so the refresh loop only
/// has to keep the cache up to date.
fn spawn_refresh_loop(
    cache: Arc<RwLock<CachedData>>,
    socket_path: PathBuf,
    refresh_secs: u64,
    health: RefreshHealth,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(refresh_secs));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match cache::fetch(&socket_path).await {
                Ok(data) => {
                    *cache.write().expect("cache lock poisoned") = data;
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs() as f64)
                        .unwrap_or(0.0);
                    health.last_refresh_timestamp_seconds.set(now);
                }
                Err(e) => {
                    error!("metrics refresh failed: {:#}", e);
                    health.refresh_failures_total.inc();
                }
            }
        }
    });
}

/// Reads the bearer token from `path`, trimming surrounding whitespace so a trailing newline
/// left by `echo` or an editor doesn't become part of the expected token.
async fn read_token_file(path: &str) -> Result<Arc<str>> {
    let contents = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read metrics-token-file {path}: {e}"))?;
    let token = contents.trim();
    if token.is_empty() {
        return Err(anyhow::anyhow!("metrics-token-file {path} is empty"));
    }
    Ok(Arc::from(token))
}

async fn rpc_status(plugin: Plugin<PluginState>, _args: Value) -> Result<Value, anyhow::Error> {
    let cache = plugin.state().cache.read().expect("cache lock poisoned");
    Ok(serde_json::json!({
        "status": "running",
        "channels": cache.channels.len(),
        "blockheight": cache.node.blockheight,
    }))
}
