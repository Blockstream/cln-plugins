mod rpc;
mod statement;

use anyhow::{Context, Result};
use cln_plugin::options::DefaultStringConfigOption;
use cln_plugin::{Builder, Plugin};
use cln_rpc::ClnRpc;
use reserves_attestor_plugin::RpcGetSignedBalanceResponse;
use serde_json::Value;
use std::path::PathBuf;
use tracing::info;

#[derive(Clone)]
pub struct PluginState {
    pub socket_path: PathBuf,
    pub service_name: String,
}

const SERVICE_NAME_OPTION: DefaultStringConfigOption =
    DefaultStringConfigOption::new_str_with_default(
        "reserves-attestor-service-name",
        "Core Lightning",
        "Service name printed in the signed statement heading",
    );

#[tokio::main]
async fn main() -> Result<()> {
    let configured = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(SERVICE_NAME_OPTION)
        .rpcmethod(
            "get-signed-balance",
            "Produce a signed on-chain + Lightning balance statement for this node",
            get_signed_balance,
        )
        .configure()
        .await?;

    let Some(configured) = configured else {
        return Ok(());
    };

    let socket_path = PathBuf::from(configured.configuration().lightning_dir)
        .join(configured.configuration().rpc_file.clone());

    let service_name = configured.option(&SERVICE_NAME_OPTION)?;

    let state = PluginState {
        socket_path,
        service_name,
    };

    info!("reserves-attestor-plugin configured");

    let plugin = configured.start(state).await?;
    plugin.join().await?;
    Ok(())
}

async fn get_signed_balance(
    plugin: Plugin<PluginState>,
    _args: Value,
) -> Result<Value, anyhow::Error> {
    let state = plugin.state();
    let mut rpc = ClnRpc::new(&state.socket_path)
        .await
        .context("connecting to CLN RPC")?;

    let inputs = rpc::fetch_inputs(&mut rpc).await?;
    let message = statement::build_message_block(&state.service_name, &inputs);
    let signature = rpc::sign_message(&mut rpc, &message).await?;

    // Self-check: run the same checkmessage round trip the verifier will do before
    // ever returning the statement, so a signing bug is caught here instead
    // of by whoever tries to verify it later.
    let (verified, recovered_pubkey) = rpc::check_message(&mut rpc, &message, &signature).await?;

    if !verified {
        anyhow::bail!(
            "signmessage/checkmessage self-check failed: verified={verified}, recovered pubkey={recovered_pubkey}",
        );
    }

    let signed = statement::build_signed_statement(&message, &signature);
    let total_msat = inputs.total_msat() as u64;

    let response = RpcGetSignedBalanceResponse {
        statement_version: statement::STATEMENT_VERSION,
        node_id: inputs.node_id,
        network: inputs.network,
        statement_time: inputs
            .statement_time
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        block_height: inputs.block_height,
        onchain_balance_msat: inputs.onchain_confirmed_msat as u64,
        lightning_balance_msat: inputs.lightning_open_msat as u64,
        total_msat,
        onchain_unconfirmed_msat: inputs.onchain_unconfirmed_msat as u64,
        onchain_reserved_msat: inputs.onchain_reserved_msat as u64,
        onchain_immature_msat: inputs.onchain_immature_msat as u64,
        lightning_pending_msat: inputs.lightning_pending_msat as u64,
        statement: signed,
        signature,
    };

    Ok(serde_json::to_value(response)?)
}
