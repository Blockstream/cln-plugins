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
    plugin.state().counters.on_channel_opened();
    Ok(())
}

pub async fn on_forward_event(plugin: Plugin<PluginState>, v: Value) -> Result<()> {
    let n: ForwardEventNotificationWrapper = serde_json::from_value(v)?;
    plugin.state().counters.on_forward_event(&n.forward_event);
    Ok(())
}

pub async fn hook_htlc_accepted(plugin: Plugin<PluginState>, _v: Value) -> Result<Value> {
    plugin.state().counters.on_htlc_accepted();
    // CLN hook - must return {"result": "continue"} or the HTLC will be rejected by the node
    Ok(json!({ "result": "continue" }))
}

pub async fn on_channel_state_changed(plugin: Plugin<PluginState>, v: Value) -> Result<()> {
    let n: ChannelStateChangedWrapper = serde_json::from_value(v)?;
    plugin
        .state()
        .counters
        .on_channel_state_changed(&n.channel_state_changed);
    Ok(())
}
