use crate::cln::client::ClnClient;
use crate::cln::{HTLCAcceptedRequest, WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS};
use crate::datastore::Datastore;
use crate::datastore::entity::{Payment, PaymentStatus, VirtualChannelStatus};
use crate::jsonrpc::Param;
use crate::jsonrpc::models::{PaidRequest, PayRequest, UnpaidRequest};
use crate::{
    State, some_or_continue, unwrap_or_continue_with_deactivation, unwrap_or_fail_with_deactivation,
};
use anyhow::Result;
use bitcoin::hashes::Hash;
use bitcoin::hashes::sha256::Hash as Sha256;
use cln_plugin::Plugin;
use cln_rpc::RpcError;
use cln_rpc::model::requests::{
    InjectpaymentonionRequest, SendcustommsgRequest, WaitblockheightRequest,
};
use cln_rpc::primitives::{Amount, PublicKey};
use log::{debug, error, info, warn};
use serde_json::{Value as JsonValue, json};
use std::str::FromStr;
use std::time::Duration;
use tokio::pin;
use tokio::time::interval;

/// Handle a request for a new payment via virtual channel where we are the next hop.
/// It is assumed that it should also return Ok with "result":"continue",
/// while any other message or error will fail the plugin
pub async fn handle_pay(
    p: Plugin<State>,
    peer_id: PublicKey,
    req: PayRequest,
) -> Result<JsonValue, RpcError> {
    if !p.state().is_active() {
        debug!("Plugin is inactive, aborting vchannel_pay request");
        return Ok(json!({"result": "continue"}));
    }

    let mut cln_rpc = unwrap_or_continue_with_deactivation!(
        p.state().get_cln_rpc().await,
        p.state(),
        "failed to create CLN rpc handler"
    );

    // Check if virtual channel exists and corresponds to the sender peer
    let channel = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&req.virtual_channel_id)
            .await,
        p.state(),
        "failed to get channel entry"
    ));

    if channel.peer_id != peer_id.to_string() {
        debug!(
            "Unauthorized: channel={:?}, peer_id={:?}, channel_peer_id={:?}",
            req.virtual_channel_id,
            peer_id.to_string(),
            channel.peer_id,
        );
        return Ok(json!({"result": "continue"}));
    }

    if channel.status == VirtualChannelStatus::Closed {
        debug!("Virtual channel is closed: channel={:?}", channel.id);
        return Ok(json!({"result": "continue"}));
    }

    // Create entry for pay request
    let mut payment = Payment {
        payment_hash: req.payment_hash.clone(),
        virtual_channel_id: req.virtual_channel_id.clone(),
        amount_msat: req.amount_msat,
        cltv: req.cltv,
        payment_status: PaymentStatus::InProgress,
        payment_preimage: Vec::new(),
    };

    let payment_hash = hex::encode(req.payment_hash.clone());

    // We assume that the pay request can be executed only once.
    // If something went wrong then the whole payment is failed.

    // Check if payment entry exist
    if unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .is_payment_exist(&req.payment_hash)
            .await,
        p.state(),
        "failed to get payment entry"
    ) {
        debug!(
            "Payment already exist: channel={:?}, payment_hash={:?}",
            channel.id, payment_hash
        );
        return Ok(json!({"result": "continue"}));
    }

    // Create payment entry
    unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .must_create_payment(&payment)
            .await,
        p.state(),
        "failed to create payment entry"
    );

    info!(
        "Executing payment: channel={:?}, payment_hash={:?}",
        channel.id, payment_hash
    );

    // Call `injectpaymentonion` with received onion
    // TODO: check partid and groupid
    let cln_req = InjectpaymentonionRequest {
        onion: req.next_onion,
        amount_msat: Amount::from_msat(req.amount_msat),
        cltv_expiry: req.cltv,
        payment_hash: unwrap_or_continue_with_deactivation!(
            Sha256::from_slice(&req.payment_hash),
            p.state(),
            "failed to parse payment hash"
        ),
        destination_msat: None,
        invstring: None,
        label: None,
        localinvreqid: None,
        partid: 0,
        groupid: 0,
    };

    let res = ClnClient::new(&mut cln_rpc)
        .call_injectpaymentonion(&cln_req)
        .await;

    match res {
        Ok(res) => {
            info!(
                "Received payment secret: channel={:?}, payment_hash={:?}, payment_preimage={:?}",
                payment.virtual_channel_id,
                payment_hash,
                hex::encode(res.payment_preimage.to_vec()),
            );

            // Send vchannel_pay callback
            let req = SendcustommsgRequest {
                msg: unwrap_or_continue_with_deactivation!(
                    PaidRequest {
                        payment_hash: payment.payment_hash.clone(),
                        payment_preimage: res.payment_preimage.to_vec(),
                    }
                    .as_custom_message(),
                    p.state(),
                    "failed to parse vchannel_paid message"
                )
                .to_string(),
                node_id: peer_id,
            };

            unwrap_or_continue_with_deactivation!(
                ClnClient::new(&mut cln_rpc).call_sendcustommsg(&req).await,
                p.state(),
                "failed to send vchannel_paid message"
            );

            // Update local payment entry
            payment.payment_status = PaymentStatus::Resolved;
            payment.payment_preimage = res.payment_preimage.to_vec();

            unwrap_or_continue_with_deactivation!(
                Datastore::new(&mut cln_rpc)
                    .must_update_payment(&payment)
                    .await,
                p.state(),
                "failed to update payment entry"
            );

            info!(
                "Payment successful: channel={:?}, payment_hash={:?}",
                payment.virtual_channel_id, payment_hash
            );
        }
        Err(e) => {
            // We failed to perform a payment, but it does not mean that some critical error happen.
            // Maybe it was some routing or other payment-related problems.
            // So we log an error and submit a vchannel_unpaid message
            error!("failed to inject onion message: error={:?}", e);
            info!(
                "Payment failed: channel={:?}, payment_hash={:?}",
                req.virtual_channel_id, payment_hash
            );

            // Send vchannel_pay callback
            let req = SendcustommsgRequest {
                msg: unwrap_or_continue_with_deactivation!(
                    UnpaidRequest {
                        payment_hash: payment.payment_hash.clone(),
                    }
                    .as_custom_message(),
                    p.state(),
                    "failed to parse vchannel_paid message"
                )
                .to_string(),
                node_id: peer_id,
            };

            unwrap_or_continue_with_deactivation!(
                ClnClient::new(&mut cln_rpc).call_sendcustommsg(&req).await,
                p.state(),
                "failed to send vchannel_paid message"
            );

            payment.payment_status = PaymentStatus::Rejected;
            unwrap_or_continue_with_deactivation!(
                Datastore::new(&mut cln_rpc)
                    .must_update_payment(&payment)
                    .await,
                p.state(),
                "failed to update payment entry"
            );
        }
    }

    Ok(json!({"result": "continue"}))
}

/// Handle a notification about successful payment via virtual channel which we have requested.
/// It is assumed that it should also return Ok with "result":"continue",
/// while any other message or error will fail the plugin
pub async fn handle_paid(
    p: Plugin<State>,
    peer_id: PublicKey,
    req: PaidRequest,
) -> Result<JsonValue, RpcError> {
    let mut cln_rpc = unwrap_or_continue_with_deactivation!(
        p.state().get_cln_rpc().await,
        p.state(),
        "failed to create CLN rpc handler"
    );

    // Get payment entry
    let mut payment = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_payment(&req.payment_hash)
            .await,
        p.state(),
        "failed to get payment entry"
    ));

    // Check if virtual channel exists and corresponds to the sender peer
    let channel = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&payment.virtual_channel_id.to_string())
            .await,
        p.state(),
        "failed to get channel entry"
    ));

    if channel.peer_id != peer_id.to_string() {
        debug!(
            "Unauthorized: channel={:?}, peer_id={:?}, channel_peer_id={:?}",
            channel.id,
            peer_id.to_string(),
            channel.peer_id,
        );
        return Ok(json!({"result": "continue"}));
    }

    // We assume that the authorized party will not propose
    // to save a secret for rejected or already resolved payment
    payment.payment_status = PaymentStatus::Resolved;
    payment.payment_preimage = req.payment_preimage.clone();

    unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .must_update_payment(&payment)
            .await,
        p.state(),
        "failed to update payment entry"
    );

    info!(
        "Received a preimage for payment: channel={:?}, payment_hash={:?}, payment_preimage={:?}",
        channel.id,
        hex::encode(payment.payment_hash),
        hex::encode(req.payment_preimage),
    );

    Ok(json!({"result": "continue"}))
}

/// Handle a notification about unsuccessful payment via virtual channel which we have requested.
/// It is assumed that it should also return Ok with "result":"continue",
/// while any other message or error will fail the plugin
pub async fn handle_unpaid(
    p: Plugin<State>,
    peer_id: PublicKey,
    req: UnpaidRequest,
) -> Result<JsonValue, RpcError> {
    let mut cln_rpc = unwrap_or_continue_with_deactivation!(
        p.state().get_cln_rpc().await,
        p.state(),
        "failed to create CLN rpc handler"
    );

    // Get payment entry
    let mut payment = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_payment(&req.payment_hash)
            .await,
        p.state(),
        "failed to get payment entry"
    ));

    // Check if virtual channel exists and corresponds to the sender peer
    let channel = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&payment.virtual_channel_id.to_string())
            .await,
        p.state(),
        "failed to get channel entry"
    ));

    if channel.peer_id != peer_id.to_string() {
        debug!(
            "Unauthorized: channel={:?}, peer_id={:?}, channel_peer_id={:?}",
            channel.id,
            peer_id.to_string(),
            channel.peer_id,
        );
        return Ok(json!({"result": "continue"}));
    }

    // We assume that the authorized party will not propose
    // to reject an already resolved payment
    payment.payment_status = PaymentStatus::Rejected;

    unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .must_update_payment(&payment)
            .await,
        p.state(),
        "failed to update payment entry"
    );

    info!(
        "Received a failure for payment: channel={:?}, payment_hash={:?}",
        channel.id,
        hex::encode(payment.payment_hash),
    );

    Ok(json!({"result": "continue"}))
}

pub async fn handle_htlc_accepted(
    p: Plugin<State>,
    hook: HTLCAcceptedRequest,
) -> Result<JsonValue, RpcError> {
    // Check the plugin is active
    if !p.state().is_active() {
        // We dont want to decide about the future of this htlc
        warn!("Plugin is inactive, aborting htlc_accepted handling");
        return Ok(json!({"result": "continue"}));
    }

    let mut cln_rpc = unwrap_or_continue_with_deactivation!(
        p.state().get_cln_rpc().await,
        p.state(),
        "failed to create CLN rpc handler"
    );

    // Check if we are not the last hop: if we're the final destination then quite
    let channel_id = some_or_continue!(hook.onion.short_channel_id);

    // Check if the next channel is our virtual channel
    let channel = some_or_continue!(unwrap_or_continue_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .get_virtual_channel(&channel_id)
            .await,
        p.state(),
        "failed to get virtual channel"
    ));

    let payment_hash = hex::encode(hook.htlc.payment_hash.clone());

    info!(
        "Processing HTLC via a virtual channel: channel={:?}, payment_hash={:?}",
        channel.id, payment_hash
    );

    // Map peer_id string to PublicKey (we assume that this error never happens)
    let node_id = unwrap_or_fail_with_deactivation!(
        PublicKey::from_str(&channel.peer_id),
        p.state(),
        "failed to create public key from peer_id"
    );

    // Check if payment entry exist
    if unwrap_or_fail_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .is_payment_exist(&hook.htlc.payment_hash)
            .await,
        p.state(),
        "failed to get payment entry"
    ) {
        debug!(
            "Payment already exist: channel={:?}, payment_hash={:?}",
            channel.id, payment_hash
        );
        return Ok(
            json!({"result": "fail", "failure_message": WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS}),
        );
    }

    // Create payment entry.
    // We assume that payment for given payment_hash can be done only once
    unwrap_or_fail_with_deactivation!(
        Datastore::new(&mut cln_rpc)
            .must_create_payment(&Payment {
                payment_hash: hook.htlc.payment_hash.clone(),
                payment_preimage: Vec::new(),
                virtual_channel_id: channel_id.clone(),
                amount_msat: hook.onion.forward_msat,
                cltv: hook.onion.outgoing_cltv_value,
                payment_status: PaymentStatus::InProgress,
            })
            .await,
        p.state(),
        "failed to create payment"
    );

    // Call vchannel_pay
    let req = SendcustommsgRequest {
        msg: unwrap_or_fail_with_deactivation!(
            PayRequest {
                next_onion: hook.onion.next_onion,
                payment_hash: hook.htlc.payment_hash.clone(),
                amount_msat: hook.onion.forward_msat,
                cltv: hook.onion.outgoing_cltv_value,
                virtual_channel_id: channel_id.clone(),
            }
            .as_custom_message(),
            p.state(),
            "failed to create custom message for vchannel_pay request"
        )
        .to_string(),
        node_id,
    };

    unwrap_or_fail_with_deactivation!(
        ClnClient::new(&mut cln_rpc).call_sendcustommsg(&req).await,
        p.state(),
        "failed to send vchannel_pay message"
    );

    info!(
        "Requested a payment via virtual channel, waiting: channel={:?}, payment_hash={:?}",
        channel.id, payment_hash
    );

    // Now, after we notified the next peer about the payment, we have to wait until it becomes
    // resolved. So, each 5 seconds we check if the payment entry has been updated (with
    // rejected on resolved status) by vchannel_paid or vchannel_unpaid requests.
    // We check it until the CLTV block for the next hop is mined.

    // Create a signal to catch the CLTV block via waitblockheight RPC calls
    let cltv_signal = wait_cltv(hook.onion.outgoing_cltv_value as u32, p.state());
    pin!(cltv_signal);

    // We are going to check the payment result each N seconds
    let mut interval = interval(Duration::from_secs(
        p.state().payment_check_interval_sec as u64,
    ));

    let mut payment_preimage = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // We assume that payment exists and should be successfully fetched.
                // If no - we continue poling.
                let payment = Datastore::new(&mut cln_rpc).get_payment(&hook.htlc.payment_hash).await;
                match payment {
                    Ok(payment) => {
                        let Some(payment) = payment else {
                            warn!("Payment entry not found during HTLC holding");
                            continue;
                        };

                        if payment.payment_status == PaymentStatus::Resolved {
                            payment_preimage = Some(payment.payment_preimage.clone());
                            break;
                        }

                        if payment.payment_status == PaymentStatus::Rejected {
                            break;
                        }
                    },
                    Err(e) => {
                        // Deactivate plugin and log the error.
                        // Still try to check HTLC status until it reaches CLTV.
                        p.state().deactivate();
                        error!("failed to fetch payment entry, will retry, error={:?}", e)
                    }
                }

            }

            _ = &mut cltv_signal => {
                info!(
                    "Payment time limit exceeded: channel={:?}, payment_hash={:?}", channel.id, payment_hash
                );
                break;
            }
        }
    }

    match payment_preimage {
        None => {
            info!(
                "Failed to resolve HTLC: channel={:?}, payment_hash={:?}",
                channel.id, payment_hash
            );

            Ok(
                json!({"result": "fail", "failure_message": WIRE_INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS}),
            )
        }
        Some(payment_preimage) => {
            let payment_preimage = hex::encode(payment_preimage);
            info!(
                "Resolved HTLC: channel={:?}, payment_hash={:?}, payment_preimage={:?}",
                channel.id, payment_hash, payment_preimage
            );

            Ok(json!({"result": "resolve", "payment_key": payment_preimage}))
        }
    }
}

async fn wait_cltv(cltv: u32, state: &State) -> Result<()> {
    let mut cln_rpc = state.get_cln_rpc().await.map_err(|e| {
        // Deactivate plugin and log the error
        state.deactivate();
        error!("failed to create CLN rpc handler, error={:?}", e);
        e
    })?;

    let req = &WaitblockheightRequest {
        timeout: None,
        blockheight: cltv,
    };
    let mut cln_client = ClnClient::new(&mut cln_rpc);

    let _ = cln_client.call_waitblockheight(req).await.map_err(|e| {
        // Deactivate plugin and log the error
        state.deactivate();
        error!("failed to call waitblockheight request, error={:?}", e);
        e
    })?;

    Ok(())
}
