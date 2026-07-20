use cln_rpc::primitives::{PublicKey, ShortChannelId};
use hex::FromHex;
use serde::{Deserialize, Deserializer, Serializer};

/// Deserializes a lowercase hex string to a `Vec<u8>`.
pub fn from_hex<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    String::deserialize(deserializer)
        .and_then(|string| Vec::from_hex(string).map_err(|err| Error::custom(err.to_string())))
}

#[allow(dead_code)]
pub fn to_hex<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&hex::encode(bytes))
}

/// A compression function for two public keys which maps them into short channel id string
pub fn create_virtual_channel_scid(id1: PublicKey, id2: PublicKey) -> String {
    let arr1: [u8; 33] = id1.serialize();
    let arr2: [u8; 33] = id2.serialize();

    // Compress 33 of arr1^arr2 into 8 bytes using XOR, element by element:
    // [0..7]^[8..15]^[16..23]^[24..31]
    // and XOR 33-rd element with the first element of the result.
    // TODO: maybe add a hash function
    let mut res: [u8; 8] = [0; 8];
    for i in 0..8 {
        res[i] = arr1[i]
            ^ arr2[i]
            ^ arr1[i + 8]
            ^ arr2[i + 8]
            ^ arr1[i + 16]
            ^ arr2[i + 16]
            ^ arr1[i + 24]
            ^ arr2[i + 24];
    }
    res[0] = res[0] ^ arr1[32] & arr2[32];

    ShortChannelId::from(u64::from_be_bytes(res)).to_string()
}
#[macro_export]
macro_rules! unwrap_or_continue_with_deactivation {
    ($res:expr, $state:expr, $msg:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                $state.deactivate();
                error!("{:?}, error={:?}", $msg, e);
                return Ok(json!({"result": "continue"}));
            }
        }
    };
}

#[macro_export]
macro_rules! unwrap_or_fail_with_deactivation {
    ($res:expr, $state:expr, $msg:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                $state.deactivate();
                error!("{:?}, error={:?}", $msg, e);
                return Ok(json!({"result": "fail", "failure_message": WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS}));
            }
        }
    };
}

#[macro_export]
macro_rules! unwrap_or_continue {
    ($res:expr, $msg:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                error!("{:?}, error={:?}", $msg, e);
                return Ok(json!({"result": "continue"}));
            }
        }
    };
}

#[macro_export]
macro_rules! some_or_continue {
    ($res:expr) => {
        match $res {
            Some(val) => val,
            None => {
                return Ok(json!({"result": "continue"}));
            }
        }
    };
}

#[macro_export]
macro_rules! unwrap_or_error {
    ($res:expr, $msg:expr, $err:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                error!("{:?}, error={:?}", $msg, e);
                return Err(RpcError::from($err));
            }
        }
    };
}
