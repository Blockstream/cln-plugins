mod cache;
mod events;
mod metrics;

use anyhow::Result;
use cln_plugin::{
    Builder, Plugin,
    options::{
        ConfigOption, DefaultBooleanConfigOption, DefaultIntegerConfigOption, StringConfigOption,
    },
};
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

const OPT_METRICS_ADDR: StringConfigOption = ConfigOption::new_str_no_default(
    "metrics-addr",
    "Address and port to expose the Prometheus /metrics endpoint on",
);

const OPT_REFRESH_SECS: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
    "metrics-refresh",
    DEFAULT_REFRESH_SECS as i64,
    "How often (seconds) to refresh CLN node data for gauge metrics",
);

const METRICS_TOKEN_ENV: &str = "METRICS_TOKEN";

const OPT_METRICS_DISABLE_AUTH: DefaultBooleanConfigOption = ConfigOption::new_bool_with_default(
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

    let metrics_token_env = std::env::var(METRICS_TOKEN_ENV).ok();
    let disable_auth = configured.option(&OPT_METRICS_DISABLE_AUTH)?;
    let auth_token: Option<String> = match (metrics_token_env, disable_auth) {
        (Some(token), _) => Some(validate_token(&token)?),
        (None, true) => {
            warn!("metrics-disable-auth is set: /metrics is being served WITHOUT authentication");
            None
        }
        (None, false) => {
            return Err(anyhow::anyhow!(
                "{METRICS_TOKEN_ENV} env var is required to protect /metrics; set --metrics-disable-auth to run without authentication"
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

/// Tokens shorter than this are rejected at startup - long enough to rule out trivially
/// guessable values without dictating a specific format.
const MIN_TOKEN_LEN: usize = 16;

/// Validates the bearer token read from the `METRICS_TOKEN` env var, trimming surrounding
/// whitespace so a stray newline from how the var was set doesn't become part of the expected
/// token.
fn validate_token(raw: &str) -> Result<String> {
    let token = raw.trim();

    if token.is_empty() {
        return Err(anyhow::anyhow!("{METRICS_TOKEN_ENV} env var is empty"));
    }

    if token.len() < MIN_TOKEN_LEN {
        return Err(anyhow::anyhow!(
            "{METRICS_TOKEN_ENV} env var holds a token shorter than {MIN_TOKEN_LEN} characters; use a longer, random value"
        ));
    }

    Ok(token.to_string())
}

async fn rpc_status(plugin: Plugin<PluginState>, _args: Value) -> Result<Value, anyhow::Error> {
    let cache = plugin.state().cache.read().expect("cache lock poisoned");
    Ok(serde_json::json!({
        "status": "running",
        "channels": cache.channels.len(),
        "blockheight": cache.node.blockheight,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_whitespace_and_trailing_newline() {
        let token = validate_token("  a-long-enough-token\n").unwrap();
        assert_eq!(&*token, "a-long-enough-token");
    }

    #[test]
    fn rejects_empty_value() {
        let err = validate_token("   \n").unwrap_err();
        assert!(err.to_string().contains("is empty"));
    }

    #[test]
    fn rejects_token_shorter_than_minimum() {
        let err = validate_token("short\n").unwrap_err();
        assert!(err.to_string().contains("shorter than"));
    }

    #[test]
    fn accepts_token_at_exactly_the_minimum_length() {
        let token = "a".repeat(MIN_TOKEN_LEN);
        let read = validate_token(&token).unwrap();
        assert_eq!(&*read, token.as_str());
    }
}
