//! Wire-format types for `get-signed-balance`, shared between the plugin
//! binary and clients that call it over CLN's JSON-RPC and need to deserialize
//! the exact response shape.
use serde::{Deserialize, Serialize};

/// `get-signed-balance`'s JSON response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcGetSignedBalanceResponse {
    pub statement_version: u32,
    pub node_id: String,
    pub network: String,
    pub statement_time: String,
    pub block_height: u32,
    pub onchain_balance_msat: u64,
    pub lightning_balance_msat: u64,
    pub total_msat: u64,
    pub onchain_unconfirmed_msat: u64,
    pub onchain_reserved_msat: u64,
    pub onchain_immature_msat: u64,
    pub lightning_pending_msat: u64,
    pub statement: String,
    pub signature: String,
}
