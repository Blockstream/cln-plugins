use anyhow::Result;
use cln_plugin::Plugin;
use cln_rpc::notifications::{ChannelStateChangedNotification, ForwardEventNotification};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::PluginState;

#[derive(Deserialize)]
struct ForwardEventNotificationWrapper {
    forward_event: ForwardEventNotification,
}

#[derive(Deserialize)]
struct ChannelStateChangedWrapper {
    channel_state_changed: ChannelStateChangedNotification,
}

pub async fn on_channel_opened(plugin: Plugin<PluginState>, _v: Value) -> Result<()> {
    plugin.state().counters.channel_opened_total.inc();
    Ok(())
}

pub async fn on_forward_event(plugin: Plugin<PluginState>, v: Value) -> Result<()> {
    let n: ForwardEventNotificationWrapper = serde_json::from_value(v)?;

    plugin
        .state()
        .counters
        .forward_events_total
        .with_label_values(&[serde_json::to_string(&n.forward_event.status)?])
        .inc();
    Ok(())
}

// CLN hook - must return {"result": "continue"} or the HTLC will be rejected by the node
pub async fn hook_htlc_accepted(plugin: Plugin<PluginState>, _v: Value) -> Result<Value> {
    plugin.state().counters.htlc_accepted_total.inc();
    Ok(json!({ "result": "continue" }))
}

pub async fn on_channel_state_changed(plugin: Plugin<PluginState>, v: Value) -> Result<()> {
    let n: ChannelStateChangedWrapper = serde_json::from_value(v)?;
    let c = n.channel_state_changed;

    let old = c
        .old_state
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let new = c.new_state.to_string();

    plugin
        .state()
        .counters
        .channel_state_changes_total
        .with_label_values(&[&old, &new])
        .inc();

    tracing::info!(
        peer = %c.peer_id,
        old_state = old,
        new_state = new,
        "channel_state_changed"
    );
    Ok(())
}
