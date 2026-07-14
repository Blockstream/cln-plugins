pub mod client;

use crate::utils::from_hex;
use anyhow::{Error, bail};
use serde::Deserialize;
use std::fmt::Display;
use std::str::FromStr;

pub const PLUGIN_MESSAGE_TYPE: u16 = 37914;

// Constants for fail responses of htlc_accepted hook.
// Taken from lightning/wire/onion_wiregen.h
#[allow(unused)]
pub const WIRE_TEMPORARY_NODE_FAILURE: &str = "2002";
#[allow(unused)]
pub const WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS: &str = "4015";

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Onion {
    pub payload: String,
    pub forward_msat: u64,
    pub outgoing_cltv_value: u16,
    pub shared_secret: String,
    pub next_onion: String,
    pub total_msat: Option<u64>,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub short_channel_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct HTLC {
    pub short_channel_id: String,
    pub id: u64,
    pub amount_msat: u64,
    pub cltv_expiry: u16,
    pub cltv_expiry_relative: u16,
    #[serde(deserialize_with = "from_hex")]
    pub payment_hash: Vec<u8>,
    pub extra_tlvs: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct HTLCAcceptedRequest {
    pub htlc: HTLC,
    pub onion: Onion,
}

/// Represents a custom message with type and payload.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomMsg {
    pub message_type: u16,
    pub payload: Vec<u8>,
}

impl FromStr for CustomMsg {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s)?;

        if bytes.len() < 2 {
            bail!("Invalid length of custom message");
        }

        let message_type_bytes: [u8; 2] = bytes[..2].try_into()?;
        let message_type = u16::from_be_bytes(message_type_bytes);
        let payload = bytes[2..].to_owned();
        Ok(CustomMsg {
            message_type,
            payload,
        })
    }
}

impl Display for CustomMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut bytes = Vec::with_capacity(2 + self.payload.len());
        bytes.extend_from_slice(&self.message_type.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        write!(f, "{}", hex::encode(bytes))
    }
}
