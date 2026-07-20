use crate::cln::{CustomMsg, PLUGIN_MESSAGE_TYPE};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::fmt::Debug;

pub mod models;

pub trait Param
where
    Self: Clone + Serialize,
{
    fn method(&self) -> &str;
    fn as_custom_message(&self) -> Result<CustomMsg> {
        let req = JsonRpcRequest::new(self.clone())?;
        Ok(CustomMsg {
            message_type: PLUGIN_MESSAGE_TYPE,
            payload: serde_json::to_vec(&req)?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: JsonValue,
}

impl JsonRpcRequest {
    pub fn new<P>(param: P) -> Result<JsonRpcRequest>
    where
        P: Param,
    {
        Ok(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: param.method().to_string(),
            params: serde_json::to_value(param)?,
        })
    }
}
