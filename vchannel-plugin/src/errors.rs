use cln_rpc::RpcError;
use thiserror::Error;

// Constants for JSON-RPC error codes.
#[allow(unused)]
pub const PARSE_ERROR: i32 = -32700;
#[allow(unused)]
pub const INVALID_REQUEST: i32 = -32600;
#[allow(unused)]
pub const METHOD_NOT_FOUND: i32 = -32601;
#[allow(unused)]
pub const INVALID_PARAMS: i32 = -32602;
#[allow(unused)]
pub const INTERNAL_ERROR: i32 = -32603;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Internal error")]
    Internal,
    #[error("Not found")]
    NotFound,
    #[error("Unauthorized")]
    Unauthorized,
    #[error("Invalid request")]
    InvalidRequestParams,
}

impl From<PluginError> for RpcError {
    fn from(err: PluginError) -> Self {
        match err {
            PluginError::Internal => RpcError {
                code: Some(INTERNAL_ERROR),
                message: "Internal error".to_string(),
                data: None,
            },
            PluginError::NotFound => RpcError {
                code: Some(INVALID_PARAMS),
                message: "Not found".to_string(),
                data: None,
            },
            PluginError::Unauthorized => RpcError {
                code: Some(INVALID_REQUEST),
                message: "Unauthorized".to_string(),
                data: None,
            },
            PluginError::InvalidRequestParams => RpcError {
                code: Some(INVALID_PARAMS),
                message: "Invalid request".to_string(),
                data: None,
            },
        }
    }
}
