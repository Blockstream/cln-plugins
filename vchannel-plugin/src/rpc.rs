use crate::datastore::Datastore;
use crate::datastore::entity::{VirtualChannel, VirtualChannelStatus};
use crate::errors::PluginError;
use crate::utils::create_virtual_channel_scid;
use crate::{State, unwrap_or_error};
use anyhow::Result;
use cln_plugin::Plugin;
use cln_rpc::RpcError;
use cln_rpc::primitives::PublicKey;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use std::str::FromStr;
use log::error;

pub async fn on_rpc_open(p: Plugin<State>, v: JsonValue) -> Result<JsonValue, RpcError> {
    let req: ClnRpcOpenRequest = unwrap_or_error!(
        serde_json::from_value(v),
        "failed to parse request json",
        PluginError::InvalidRequestParams
    );

    let peer_id = unwrap_or_error!(
        PublicKey::from_str(&req.peer_id),
        "failed to create public key from peer_id",
        PluginError::InvalidRequestParams
    );

    let virtual_channel_id = create_virtual_channel_scid(p.state().my_peer_id, peer_id);

    let mut cln_rpc = unwrap_or_error!(
        p.state().get_cln_rpc().await,
        "failed to create CLN rpc handler",
        PluginError::Internal
    );

    let channel = unwrap_or_error!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&virtual_channel_id)
            .await,
        "failed to get channel",
        PluginError::Internal
    );

    // Check if the channel already exist - then change its status to Opened.
    // Otherwise - create new channel
    match channel {
        Some(mut channel) => {
            channel.status = VirtualChannelStatus::Opened;
            unwrap_or_error!(
                Datastore::new(&mut cln_rpc)
                    .must_update_virtual_channel(&channel)
                    .await,
                "failed to update virtual channel",
                PluginError::Internal
            );
        }
        None => {
            unwrap_or_error!(
                Datastore::new(&mut cln_rpc)
                    .must_create_virtual_channel(&VirtualChannel {
                        id: virtual_channel_id.to_string(),
                        peer_id: req.peer_id,
                        status: VirtualChannelStatus::Opened,
                    })
                    .await,
                "failed to create virtual channel",
                PluginError::Internal
            );
        }
    }

    Ok(JsonValue::default())
}

pub async fn on_rpc_close(p: Plugin<State>, v: JsonValue) -> Result<JsonValue, RpcError> {
    let req: ClnRpcCloseRequest = unwrap_or_error!(
        serde_json::from_value(v),
        "failed to parse request json",
        PluginError::InvalidRequestParams
    );

    let mut cln_rpc = unwrap_or_error!(
        p.state().get_cln_rpc().await,
        "failed to create CLN rpc handler",
        PluginError::Internal
    );

    let channel = unwrap_or_error!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&req.virtual_channel_id)
            .await,
        "failed to get channel",
        PluginError::Internal
    );

    match channel {
        None => return Err(PluginError::NotFound.into()),
        Some(mut channel) => {
            channel.status = VirtualChannelStatus::Closed;
            unwrap_or_error!(
                Datastore::new(&mut cln_rpc)
                    .must_update_virtual_channel(&channel)
                    .await,
                "failed to update virtual channel",
                PluginError::Internal
            );
        }
    }

    Ok(JsonValue::default())
}

pub async fn on_rpc_list(p: Plugin<State>, _: JsonValue) -> Result<JsonValue, RpcError> {
    let mut cln_rpc = unwrap_or_error!(
        p.state().get_cln_rpc().await,
        "failed to create CLN rpc handler",
        PluginError::Internal
    );

    let channels = unwrap_or_error!(
        Datastore::new(&mut cln_rpc).list_virtual_channels().await,
        "failed to list channels",
        PluginError::Internal
    );

    let result: Vec<JsonValue> = channels
        .iter()
        .map(|channel| {
            json!({
                "virtual_channel_id": channel.id,
                "peer_id": channel.peer_id,
                "status": channel.status,
            })
        })
        .collect();

    Ok(unwrap_or_error!(
        serde_json::to_value(result),
        "failed to serialize result",
        PluginError::Internal
    ))
}

pub async fn on_rpc_activate(p: Plugin<State>, _: JsonValue) -> Result<JsonValue, RpcError> {
    p.state().activate();
    Ok(JsonValue::default())
}

pub async fn on_rpc_deactivate(p: Plugin<State>, _: JsonValue) -> Result<JsonValue, RpcError> {
    p.state().deactivate();
    Ok(JsonValue::default())
}

pub async fn on_rpc_status(p: Plugin<State>, _: JsonValue) -> Result<JsonValue, RpcError> {
    Ok(json!({"status": p.state().is_active().to_string()}))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClnRpcOpenRequest {
    peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClnRpcCloseRequest {
    virtual_channel_id: String,
}
