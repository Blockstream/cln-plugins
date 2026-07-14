use crate::jsonrpc::Param;
use crate::utils::{from_hex, to_hex};
use serde::{Deserialize, Serialize};

pub const PAY_METHOD: &str = "vchannel_pay";
pub const PAID_METHOD: &str = "vchannel_paid";
pub const UNPAID_METHOD: &str = "vchannel_unpaid";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PayRequest {
    /// Contains our onion
    pub next_onion: String,
    #[serde(deserialize_with = "from_hex", serialize_with = "to_hex")]
    pub payment_hash: Vec<u8>,
    pub amount_msat: u64,
    pub cltv: u16,
    /// Contains a virtual channel id
    pub virtual_channel_id: String,
}

impl Param for PayRequest {
    fn method(&self) -> &str {
        PAY_METHOD
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaidRequest {
    #[serde(deserialize_with = "from_hex", serialize_with = "to_hex")]
    pub payment_hash: Vec<u8>,
    #[serde(deserialize_with = "from_hex", serialize_with = "to_hex")]
    pub payment_preimage: Vec<u8>,
}

impl Param for PaidRequest {
    fn method(&self) -> &str {
        PAID_METHOD
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnpaidRequest {
    #[serde(deserialize_with = "from_hex", serialize_with = "to_hex")]
    pub payment_hash: Vec<u8>,
}

impl Param for UnpaidRequest {
    fn method(&self) -> &str {
        UNPAID_METHOD
    }
}
