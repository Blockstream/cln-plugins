use crate::broker::MessageBroker;
use crate::proto::EventEnvelope;
use anyhow::Result;
use cln_plugin::Plugin;
use ferroid::define_snowflake_id;
use ferroid::futures::SnowflakeGeneratorAsyncTokioExt;
use ferroid::time::{MonotonicClock, UNIX_EPOCH};
use prost::Message;
use prost_types::Timestamp;
use serde_json::{Value as JsonValue, json};
use std::sync::OnceLock;
use std::time::SystemTime;
use tracing::debug;
use tracing::log::warn;

define_snowflake_id!(
    EventId, u64,
    reserved: 1,
    timestamp: 41,
    machine_id: 10,
    sequence: 12
);

static EVENT_ID_GENERATOR: OnceLock<
    ferroid::generator::AtomicSnowflakeGenerator<EventId, MonotonicClock<1>>,
> = OnceLock::new();

/// Called once from main.rs after fetching the node_id. Derives the 10-bit
/// machine identifier from the last 10 bits of the node's public key.
pub fn init_event_id_generator(node_id_bytes: &[u8]) {
    let len = node_id_bytes.len();
    let machine_bits = u16::from_be_bytes([node_id_bytes[len - 2], node_id_bytes[len - 1]]) & 0x3FF;
    let clock = MonotonicClock::<1>::with_epoch(UNIX_EPOCH);
    let generator =
        ferroid::generator::AtomicSnowflakeGenerator::<EventId, _>::new(machine_bits as u64, clock);

    assert!(
        EVENT_ID_GENERATOR.set(generator).is_ok(),
        "EVENT_ID_GENERATOR already initialized"
    )
}

async fn next_event_id() -> u64 {
    let generator = EVENT_ID_GENERATOR
        .get()
        .expect("expected EVENT_ID_GENERATOR to be initialized");
    generator.next_id_async().await.id
}

async fn new_envelope(
    event_type: &str,
    source_kind: i32,
    source_node_id: &[u8],
    producer_version: &str,
    payload: &JsonValue,
) -> Result<EventEnvelope> {
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
    Ok(EventEnvelope {
        envelope_version: 1,
        event_id: next_event_id().await,
        event_type: event_type.to_string(),
        emitted_at: Some(Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        }),
        source_kind,
        source_node_id: source_node_id.to_vec(),
        producer_version: producer_version.to_string(),
        payload_json: serde_json::to_vec(payload)?,
    })
}

fn encode_envelope(envelope: EventEnvelope) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    envelope.encode(&mut buf)?;
    Ok(buf)
}

/// Convenience function to handle events.
macro_rules! define_event_handler {
    ($fn_name:ident, $event_type:expr) => {
        pub async fn $fn_name(p: Plugin<MessageBroker>, v: JsonValue) -> Result<()> {
            debug!(
                "handled event, event_type={}, payload={:?}",
                $event_type, &v
            );
            let state = p.state();
            state
                .publish_bytes(encode_envelope(
                    new_envelope(
                        $event_type,
                        state.source_kind(),
                        state.source_node_id(),
                        state.producer_version(),
                        &v,
                    )
                    .await?,
                )?)
                .await
        }
    };
}

// Define event handler.
define_event_handler!(on_connect, "connect");
define_event_handler!(on_disconnect, "disconnect");
define_event_handler!(on_invoice_creation, "invoice_creation");
define_event_handler!(on_invoice_payment, "invoice_payment");
define_event_handler!(on_channel_opened, "channel_opened");
define_event_handler!(on_channel_open_failed, "channel_open_failed");
define_event_handler!(on_channel_state_changed, "channel_state_changed");
define_event_handler!(on_forward_event, "forward_event");
define_event_handler!(on_block_added, "block_added");
define_event_handler!(on_custommsg, "custommsg");
define_event_handler!(on_sendpay_success, "sendpay_success");
define_event_handler!(on_sendpay_failure, "sendpay_failure");
define_event_handler!(on_coin_movement, "coin_movement");
define_event_handler!(on_openchannel_peer_sigs, "openchannel_peer_sigs");
define_event_handler!(on_onionmessage_forward_fail, "onionmessage_forward_fail");
define_event_handler!(on_pay_part_start, "pay_part_start");
define_event_handler!(on_pay_part_end, "pay_part_end");

pub async fn on_warning(_p: Plugin<MessageBroker>, _v: JsonValue) -> Result<()> {
    // TODO: no-op for now, will eff-up integration test teardown otherwise as
    // it will produce and infinite loop of "channel closed warning" ->
    // on_warning -> publish -> produce warning -> "channel closed warning"
    // -> ... an infinite card draw combo. We need a better way to shutdown
    // the MessageBroker.
    Ok(())
}

#[allow(unused)]
/// This hook is called when an incoming HTLC is received as part of a payment flow so it
/// publishes the event data to RabbitMQ and then allows the payment to continue processing.
pub async fn hook_htlc_accepted(p: Plugin<MessageBroker>, v: JsonValue) -> Result<JsonValue> {
    let state = p.state();
    debug!(
        "handled event, event_type=htlc_accepted_hook, payload={:?}",
        &v
    );

    if let Err(e) = async {
        // TODO (PN): This is dangerous if this is a blocking function. It could
        // lead to a forced channel close if we hold onto the HTLC for too long.
        // We should better remove as the important considerations are emitted
        // by the lsps plugin's event sink.
        // -- For now I'll leave the function here but remove the subscripion (PN)
        p.state()
            .publish_bytes(encode_envelope(
                new_envelope(
                    "htlc_accepted_hook",
                    state.source_kind(),
                    state.source_node_id(),
                    state.producer_version(),
                    &v,
                )
                .await?,
            )?)
            .await
    }
    .await
    {
        warn!("htlc accepted hook failed: {:#}", e);
    }

    Ok(json!({"result": "continue"}))
}
