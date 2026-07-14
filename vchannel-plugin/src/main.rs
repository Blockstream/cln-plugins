mod cln;
mod datastore;
mod errors;
mod handlers;
mod hooks;
mod jsonrpc;
mod rpc;
mod utils;

use crate::cln::PLUGIN_MESSAGE_TYPE;
use crate::hooks::{on_hook_custommsg, on_hook_htlc_accepted};
use crate::rpc::{
    on_rpc_activate, on_rpc_close, on_rpc_deactivate, on_rpc_list, on_rpc_open, on_rpc_status,
};
use anyhow::{Context, Result};
use cln_plugin::Builder;
use cln_plugin::options::{ConfigOption, DefaultIntegerConfigOption};
use cln_rpc::ClnRpc;
use cln_rpc::model::requests::GetinfoRequest;
use cln_rpc::primitives::PublicKey;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const OPTION_CHECK_PAYMENT_SEC_NAME: &str = "vchannel-check-payment-sec";

pub const OPTION_CHECK_PAYMENT_SEC: DefaultIntegerConfigOption = ConfigOption::new_i64_with_default(
    OPTION_CHECK_PAYMENT_SEC_NAME,
    5,
    "The retry interval in seconds to check the payment result during htlc_accepted hook holding",
);

#[derive(Clone)]
pub struct State {
    pub cln_rpc_path: PathBuf,
    pub my_peer_id: PublicKey,
    /// We use this variable to safely reject any incoming payments
    /// if any error happened, but still try to finalize payments which are already in progress.
    ///
    /// WARNING: This variable always sets to true on the startup of plugin, so you should not start
    /// your plugin if there are any problems with your CLN node.
    pub is_active: Arc<AtomicBool>,
    pub payment_check_interval_sec: i64,
}

impl State {
    pub async fn get_cln_rpc(&self) -> Result<ClnRpc> {
        ClnRpc::new(&self.cln_rpc_path).await
    }

    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }

    pub fn deactivate(&self) {
        self.is_active.store(false, Ordering::SeqCst);
    }

    pub fn activate(&self) {
        self.is_active.store(true, Ordering::SeqCst);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let configured = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(OPTION_CHECK_PAYMENT_SEC)
        .rpcmethod(
            "vch-open",
            "Add virtual channel with peer",
            async move |p, v| -> Result<Value> { on_rpc_open(p, v).await.map_err(|e| e.into()) },
        )
        .rpcmethod(
            "vch-close",
            "Close virtual channel",
            async move |p, v| -> Result<Value> { on_rpc_close(p, v).await.map_err(|e| e.into()) },
        )
        .rpcmethod(
            "vch-list",
            "List virtual channels",
            async move |p, v| -> Result<Value> { on_rpc_list(p, v).await.map_err(|e| e.into()) },
        )
        .rpcmethod(
            "vch-status",
            "Get internal plugin status",
            async move |p, v| -> Result<Value> { on_rpc_status(p, v).await.map_err(|e| e.into()) },
        )
        .rpcmethod(
            "vch-activate",
            "Activate plugin",
            async move |p, v| -> Result<Value> {
                on_rpc_activate(p, v).await.map_err(|e| e.into())
            },
        )
        .rpcmethod(
            "vch-deactivate",
            "Deactivate plugin",
            async move |p, v| -> Result<Value> {
                on_rpc_deactivate(p, v).await.map_err(|e| e.into())
            },
        )
        .hook("htlc_accepted", on_hook_htlc_accepted)
        .hook("custommsg", on_hook_custommsg)
        .custommessages(vec![PLUGIN_MESSAGE_TYPE])
        .dynamic()
        .configure()
        .await?;

    let Some(configured) = configured else {
        // CLN shut down during configuration
        return Ok(());
    };

    let cfg = configured.configuration();
    let rpc_path = Path::new(&cfg.lightning_dir).join(&cfg.rpc_file);

    let payment_check_interval_sec: i64 = configured
        .option(&OPTION_CHECK_PAYMENT_SEC)
        .context("failed to parse option")?;

    let state = State {
        cln_rpc_path: rpc_path.clone(),
        my_peer_id: get_my_peer_id(rpc_path).await?,
        is_active: Arc::new(AtomicBool::new(true)),
        payment_check_interval_sec,
    };

    let plugin = configured.start(state).await?;
    plugin.join().await?;

    Ok(())
}

async fn get_my_peer_id(rpc_path: PathBuf) -> Result<PublicKey> {
    let mut cln_rpc = ClnRpc::new(&rpc_path).await?;
    let info = cln_rpc.call_typed(&GetinfoRequest {}).await?;
    Ok(info.id)
}
