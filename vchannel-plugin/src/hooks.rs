use crate::cln::{CustomMsg, HTLCAcceptedRequest, PLUGIN_MESSAGE_TYPE};
use crate::handlers::{handle_htlc_accepted, handle_paid, handle_pay, handle_unpaid};
use crate::jsonrpc::JsonRpcRequest;
use crate::jsonrpc::models::{
    PAID_METHOD, PAY_METHOD, PaidRequest, PayRequest, UNPAID_METHOD, UnpaidRequest,
};
use crate::{State, unwrap_or_continue};
use anyhow::Result;
use cln_plugin::Plugin;
use cln_rpc::notifications::CustomMsgNotification;
use log::{debug, error, info};
use serde_json::{Value as JsonValue, json};
use std::str::FromStr;

pub async fn on_hook_htlc_accepted(p: Plugin<State>, v: JsonValue) -> Result<JsonValue> {
    info!("Received hook {:?}", v.to_string());
    let hook = unwrap_or_continue!(
        serde_json::from_value::<HTLCAcceptedRequest>(v),
        "failed to parse htlc_accepted hook"
    );

    handle_htlc_accepted(p, hook).await.map_err(|e| e.into())
}

pub async fn on_hook_custommsg(p: Plugin<State>, v: JsonValue) -> Result<JsonValue> {
    debug!("Received custom message: {:?}", v);

    let msg = unwrap_or_continue!(
        serde_json::from_value::<CustomMsgNotification>(v),
        "failed to parse custommsg hook"
    );

    let peer_id = msg.peer_id;

    let msg = unwrap_or_continue!(
        CustomMsg::from_str(&msg.payload),
        "failed to parse custommsg hook payload"
    );

    if msg.message_type != PLUGIN_MESSAGE_TYPE {
        debug!("Received unrecognized message type {:?}", msg.message_type);
        // We don't care if this is not for us!
        return Ok(json!({"result": "continue"}));
    }

    let req: JsonRpcRequest = unwrap_or_continue!(
        serde_json::from_slice(&msg.payload),
        "failed to parse jsonrpc request from custommsg hook"
    );

    debug!(
        "Processing custom message request, method={:?}, params={:?}",
        req.method, req.params
    );

    match req.method.as_str() {
        PAY_METHOD => {
            let req: PayRequest = unwrap_or_continue!(
                serde_json::from_value(req.params),
                "failed to parse params for pay request"
            );

            handle_pay(p, peer_id, req).await.map_err(|e| e.into())
        }

        PAID_METHOD => {
            let req: PaidRequest = unwrap_or_continue!(
                serde_json::from_value(req.params),
                "failed to parse params for paid request"
            );

            handle_paid(p, peer_id, req).await.map_err(|e| e.into())
        }

        UNPAID_METHOD => {
            let req: UnpaidRequest = unwrap_or_continue!(
                serde_json::from_value(req.params),
                "failed to parse params for unpaid request"
            );

            handle_unpaid(p, peer_id, req).await.map_err(|e| e.into())
        }

        _ => {
            debug!("Got request with unknown method {:?}", req.method);
            Ok(json!({"result": "continue"}))
        }
    }
}
