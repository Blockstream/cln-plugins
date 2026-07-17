use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;

const EVENT_TYPES_ENV: &str = "EVENT_PLUGIN_EVENTS";
const CONFIG_FILE_ENV: &str = "EVENT_PLUGIN_CONFIG";

/// Single source of truth for the events subscribed to when neither
/// `EVENT_PLUGIN_EVENTS` nor the TOML config file specifies a list.
const DEFAULT_EVENTS: &[&str] = &[
    "connect",
    "disconnect",
    "invoice_creation",
    "invoice_payment",
    "channel_opened",
    "channel_open_failed",
    "channel_state_changed",
    "forward_event",
    "block_added",
    "custommsg",
    "warning",
    "sendpay_success",
    "sendpay_failure",
    "coin_movement",
    "openchannel_peer_sigs",
    "onionmessage_forward_fail",
    "pay_part_start",
    "pay_part_end",
];

/// Resolves the list of events to subscribe to, in priority order:
/// 1. `EVENT_PLUGIN_EVENTS` environment variable (comma-separated)
/// 2. `events_list` in the TOML file pointed to by `EVENT_PLUGIN_CONFIG`
/// 3. [`DEFAULT_EVENTS`]
pub fn resolve_event_types() -> Result<Vec<String>> {
    let event_types = if let Ok(raw) = env::var(EVENT_TYPES_ENV) {
        parse_event_list(&raw)
    } else if let Some(list) = event_types_from_config_file()? {
        list
    } else {
        DEFAULT_EVENTS.iter().map(|s| s.to_string()).collect()
    };

    if event_types.is_empty() {
        bail!("event subscription list must contain at least one event type");
    }
    if event_types.iter().any(|event_type| event_type == "*") {
        bail!("wildcard event subscriptions are not supported");
    }

    Ok(event_types)
}

fn parse_event_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|event_type| !event_type.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    #[serde(rename = "event-plugin", default)]
    event_plugin: EventPluginSection,
}

#[derive(Deserialize, Default)]
struct EventPluginSection {
    events_list: Option<Vec<String>>,
}

/// Returns the `events_list` from the TOML config file, or `None` when no
/// config file is configured or the file does not set the field.
fn event_types_from_config_file() -> Result<Option<Vec<String>>> {
    let Ok(path) = env::var(CONFIG_FILE_ENV) else {
        return Ok(None);
    };
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file '{path}'"))?;
    let config: ConfigFile =
        toml::from_str(&raw).with_context(|| format!("failed to parse config file '{path}'"))?;
    Ok(config.event_plugin.events_list)
}

#[cfg(test)]
mod test {
    use super::{ConfigFile, parse_event_list};

    #[test]
    fn test_parse_event_list() {
        assert_eq!(
            parse_event_list(" connect, disconnect ,,invoice_payment"),
            vec!["connect", "disconnect", "invoice_payment"]
        );
        assert!(parse_event_list("").is_empty());
    }

    #[test]
    fn test_config_file_events_list() {
        let config: ConfigFile = toml::from_str(
            r#"
            [event-plugin]
            events_list = ["connect", "disconnect", "invoice_payment"]
            "#,
        )
        .unwrap();

        assert_eq!(
            config.event_plugin.events_list,
            Some(vec![
                "connect".to_string(),
                "disconnect".to_string(),
                "invoice_payment".to_string()
            ])
        );
    }

    #[test]
    fn test_config_file_without_events_list() {
        let config: ConfigFile = toml::from_str("").unwrap();
        assert_eq!(config.event_plugin.events_list, None);
    }
}
