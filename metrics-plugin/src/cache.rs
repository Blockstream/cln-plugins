use std::collections::HashMap;
use std::path::Path;

use cln_rpc::ClnRpc;
use cln_rpc::model::requests::{GetinfoRequest, ListfundsRequest, ListpeerchannelsRequest};

#[derive(Clone, Default)]
pub struct ChannelSnapshot {
    pub peer_id: String,
    pub scid: String,
    pub balance_msat: i64,
    pub spendable_msat: i64,
    pub receivable_msat: i64,
    pub capacity_msat: i64,
    pub htlcs: i64,

    pub in_payments_offered: i64,
    pub in_payments_fulfilled: i64,
    pub in_msat_offered: i64,
    pub in_msat_fulfilled: i64,
    pub out_payments_offered: i64,
    pub out_payments_fulfilled: i64,
    pub out_msat_offered: i64,
    pub out_msat_fulfilled: i64,
}

#[derive(Clone, Default)]
pub struct NodeSnapshot {
    pub blockheight: i64,
    pub fees_collected_msat: i64,
}

#[derive(Clone, Default)]
pub struct FundsSnapshot {
    pub output_msat: i64,
    pub channel_msat: i64,
}

#[derive(Clone, Default)]
pub struct PeerSnapshot {
    pub id: String,
    pub connected: bool,
    pub num_channels: usize,
    pub connected_channels: usize,
}

/// Decouples RPC parsing from metric updates and lets the status RPC answer without a CLN round-trip
#[derive(Clone, Default)]
pub struct CachedData {
    pub node: NodeSnapshot,
    pub funds: FundsSnapshot,
    pub channels: Vec<ChannelSnapshot>,
    pub peers: Vec<PeerSnapshot>,
}

pub async fn fetch(socket: &Path) -> anyhow::Result<CachedData> {
    let mut rpc = ClnRpc::new(socket).await?;

    let info = rpc.call_typed(&GetinfoRequest {}).await?;
    let channels_resp = rpc
        .call_typed(&ListpeerchannelsRequest {
            id: None,
            channel_id: None,
            short_channel_id: None,
        })
        .await?;
    let funds_resp = rpc.call_typed(&ListfundsRequest { spent: None }).await?;

    let node = NodeSnapshot {
        blockheight: info.blockheight as i64,
        fees_collected_msat: info.fees_collected_msat.msat() as i64,
    };

    let output_msat: i64 = funds_resp
        .outputs
        .iter()
        .map(|o| o.amount_msat.msat() as i64)
        .sum();

    let channel_msat: i64 = funds_resp
        .channels
        .iter()
        .map(|c| c.our_amount_msat.msat() as i64)
        .sum();

    let mut channels = Vec::with_capacity(channels_resp.channels.len());
    // CLN doesn't expose per-peer channel aggregates directly so we build them from channel records
    let mut peer_channel_count: HashMap<String, usize> = HashMap::new();
    let mut peer_connected_count: HashMap<String, usize> = HashMap::new();

    for c in channels_resp.channels {
        let peer_id = c.peer_id.to_string();
        let scid = c
            .short_channel_id
            .map(|s| s.to_string())
            .or_else(|| c.channel_id.map(|id| id.to_string()))
            .unwrap_or_default();
        let connected = c.peer_connected;

        *peer_channel_count.entry(peer_id.clone()).or_insert(0) += 1;
        if connected {
            *peer_connected_count.entry(peer_id.clone()).or_insert(0) += 1;
        }

        channels.push(ChannelSnapshot {
            peer_id,
            scid,
            balance_msat: c.to_us_msat.map(|a| a.msat() as i64).unwrap_or(0),
            spendable_msat: c.spendable_msat.map(|a| a.msat() as i64).unwrap_or(0),
            receivable_msat: c.receivable_msat.map(|a| a.msat() as i64).unwrap_or(0),
            capacity_msat: c.total_msat.map(|a| a.msat() as i64).unwrap_or(0),
            htlcs: c.htlcs.as_ref().map(|h| h.len() as i64).unwrap_or(0),
            in_payments_offered: c.in_payments_offered.map(|v| v as i64).unwrap_or(0),
            in_payments_fulfilled: c.in_payments_fulfilled.map(|v| v as i64).unwrap_or(0),
            in_msat_offered: c.in_offered_msat.map(|a| a.msat() as i64).unwrap_or(0),
            in_msat_fulfilled: c.in_fulfilled_msat.map(|a| a.msat() as i64).unwrap_or(0),
            out_payments_offered: c.out_payments_offered.map(|v| v as i64).unwrap_or(0),
            out_payments_fulfilled: c.out_payments_fulfilled.map(|v| v as i64).unwrap_or(0),
            out_msat_offered: c.out_offered_msat.map(|a| a.msat() as i64).unwrap_or(0),
            out_msat_fulfilled: c.out_fulfilled_msat.map(|a| a.msat() as i64).unwrap_or(0),
        });
    }

    let peers = peer_channel_count
        .into_iter()
        .map(|(id, num_channels)| {
            let connected_channels = *peer_connected_count.get(&id).unwrap_or(&0);
            PeerSnapshot {
                connected: connected_channels > 0,
                connected_channels,
                num_channels,
                id,
            }
        })
        .collect();

    Ok(CachedData {
        node,
        funds: FundsSnapshot {
            output_msat,
            channel_msat,
        },
        channels,
        peers,
    })
}
