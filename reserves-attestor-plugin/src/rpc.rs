use crate::statement::BalanceStatementInputs;
use anyhow::{Context, Result};
use chrono::Utc;
use cln_rpc::ClnRpc;
use cln_rpc::model::requests::{
    CheckmessageRequest, GetinfoRequest, ListfundsRequest, ListpeerchannelsRequest,
    SignmessageRequest,
};
use cln_rpc::model::responses::ListfundsOutputsStatus;
use cln_rpc::primitives::{ChannelState, PublicKey};

/// Signs `message` (the plain message block, with no armor) via `signmessage`
/// and returns the zbase32-encoded signature.
pub async fn sign_message(rpc: &mut ClnRpc, message: &str) -> Result<String> {
    let resp = rpc
        .call_typed(&SignmessageRequest {
            message: message.to_string(),
        })
        .await
        .context("signmessage")?;

    Ok(resp.zbase)
}

/// Verifies a message/signature pair against `pubkey` via `checkmessage` and
/// returns whether it verified plus the pubkey CLN recovered from the
/// signature. Run right after signing so a broken signature is caught before
/// the statement ever leaves the node, instead of being discovered by users
/// later.
pub async fn check_message(
    rpc: &mut ClnRpc,
    message: &str,
    zbase: &str,
    pubkey: Option<PublicKey>,
) -> Result<(bool, String)> {
    let resp = rpc
        .call_typed(&CheckmessageRequest {
            pubkey,
            message: message.to_string(),
            zbase: zbase.to_string(),
        })
        .await
        .context("checkmessage")?;

    Ok((resp.verified, resp.pubkey.to_string()))
}

/// Pulls `getinfo` / `listfunds` / `listpeerchannels` and reduces them to the
/// msat-denominated inputs the statement needs. On-chain funds are split by
/// `listfunds` status/reserved flag; Lightning funds are split by whether the
/// channel is `CHANNELD_NORMAL` (open) or anything else (pending open/close) -
/// only the confirmed-unreserved and open buckets ever reach a total.
pub async fn fetch_inputs(rpc: &mut ClnRpc) -> Result<BalanceStatementInputs> {
    let info = rpc
        .call_typed(&GetinfoRequest {})
        .await
        .context("getinfo")?;

    let funds = rpc
        .call_typed(&ListfundsRequest { spent: None })
        .await
        .context("listfunds")?;

    let peerchannels = rpc
        .call_typed(&ListpeerchannelsRequest {
            id: None,
            channel_id: None,
            short_channel_id: None,
        })
        .await
        .context("listpeerchannels")?;

    let mut onchain_confirmed_msat = 0i64;
    let mut onchain_unconfirmed_msat = 0i64;
    let mut onchain_reserved_msat = 0i64;
    let mut onchain_immature_msat = 0i64;

    for output in &funds.outputs {
        let msat = output.amount_msat.msat() as i64;

        match output.status {
            ListfundsOutputsStatus::CONFIRMED if !output.reserved => {
                onchain_confirmed_msat += msat;
            }
            ListfundsOutputsStatus::CONFIRMED => onchain_reserved_msat += msat,
            ListfundsOutputsStatus::UNCONFIRMED => onchain_unconfirmed_msat += msat,
            ListfundsOutputsStatus::IMMATURE => onchain_immature_msat += msat,
            ListfundsOutputsStatus::SPENT => {}
        }
    }

    let mut lightning_open_msat = 0i64;
    let mut lightning_pending_msat = 0i64;

    for channel in &peerchannels.channels {
        let msat = channel.to_us_msat.map(|a| a.msat() as i64).unwrap_or(0);

        if channel.state == ChannelState::CHANNELD_NORMAL {
            lightning_open_msat += msat;
        } else {
            lightning_pending_msat += msat;
        }
    }

    Ok(BalanceStatementInputs {
        node_id: info.id.to_string(),
        network: info.network,
        statement_time: Utc::now(),
        block_height: info.blockheight,
        onchain_confirmed_msat,
        onchain_unconfirmed_msat,
        onchain_reserved_msat,
        onchain_immature_msat,
        lightning_open_msat,
        lightning_pending_msat,
    })
}
