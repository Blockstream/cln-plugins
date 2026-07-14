use crate::utils::{from_hex, to_hex};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum VirtualChannelStatus {
    Opened,
    Closed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VirtualChannel {
    #[serde(skip)] // Skip because it is a part of the key
    /// Virtual channel id
    pub id: String,
    /// Id of the another peer
    pub peer_id: String,
    /// Status of the virtual channel
    pub status: VirtualChannelStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PaymentStatus {
    InProgress,
    Resolved,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Payment {
    #[serde(skip)] // Skip because it is a part of the key
    pub payment_hash: Vec<u8>,
    pub virtual_channel_id: String,
    pub amount_msat: u64,
    pub cltv: u16,
    pub payment_status: PaymentStatus,
    #[serde(serialize_with = "to_hex", deserialize_with = "from_hex")]
    pub payment_preimage: Vec<u8>, // Appears when resolved
}
