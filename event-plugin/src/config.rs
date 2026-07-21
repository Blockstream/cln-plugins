use anyhow::{Context, Result, bail};
use config::builder::{ConfigBuilder, DefaultState};
use config::{Config, Environment, File, FileFormat};
use serde::Deserialize;
use std::path::PathBuf;

const ENV_PREFIX: &str = "EVENT_PLUGIN";
const ENV_EVENTS_KEY: &str = "events";
const CONFIG_EVENTS_KEY: &str = "event-plugin.events_list";

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
    let environment = read_environment(environment_source())?;
    let mut builder = Config::builder().set_default(CONFIG_EVENTS_KEY, DEFAULT_EVENTS.to_vec())?;

    if let Some(path) = environment.config {
        builder = builder.add_source(File::from(path).format(FileFormat::Toml));
    }

    resolve_event_types_from(builder, environment.events)
}

fn environment_source() -> Environment {
    Environment::with_prefix(ENV_PREFIX)
        .try_parsing(true)
        .list_separator(",")
        .with_list_parse_key(ENV_EVENTS_KEY)
}

fn read_environment(environment: Environment) -> Result<EnvironmentSettings> {
    Config::builder()
        .add_source(environment)
        .build()
        .context("failed to read event-plugin environment")?
        .try_deserialize()
        .context("failed to parse event-plugin environment")
}

fn resolve_event_types_from(
    builder: ConfigBuilder<DefaultState>,
    environment_events: Option<Vec<String>>,
) -> Result<Vec<String>> {
    let event_types = builder
        .set_override_option(CONFIG_EVENTS_KEY, environment_events)?
        .build()
        .context("failed to load event-plugin configuration")?
        .get::<Vec<String>>(CONFIG_EVENTS_KEY)
        .context("failed to parse configured event types")?
        .into_iter()
        .map(|event_type| event_type.trim().to_owned())
        .filter(|event_type| !event_type.is_empty())
        .collect::<Vec<_>>();

    if event_types.is_empty() {
        bail!("event subscription list must contain at least one event type");
    }
    if event_types.iter().any(|event_type| event_type == "*") {
        bail!("wildcard event subscriptions are not supported");
    }

    Ok(event_types)
}

#[derive(Deserialize)]
struct EnvironmentSettings {
    config: Option<PathBuf>,
    events: Option<Vec<String>>,
}

#[cfg(test)]
mod test {
    use super::{
        CONFIG_EVENTS_KEY, DEFAULT_EVENTS, environment_source, read_environment,
        resolve_event_types_from,
    };
    use config::{Environment, File, FileFormat};
    use std::collections::HashMap;

    fn environment(values: &[(&str, &str)]) -> Environment {
        environment_source().source(Some(
            values
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<HashMap<_, _>>(),
        ))
    }

    #[test]
    fn defaults_when_sources_do_not_configure_events() {
        assert_eq!(
            resolve_event_types_from(
                config::Config::builder()
                    .set_default(CONFIG_EVENTS_KEY, DEFAULT_EVENTS.to_vec())
                    .unwrap()
                    .add_source(File::from_str("", FileFormat::Toml)),
                None,
            )
            .unwrap(),
            DEFAULT_EVENTS
        );
    }

    #[test]
    fn reads_config_path_from_environment() {
        let settings = read_environment(environment(&[(
            "EVENT_PLUGIN_CONFIG",
            "/etc/lightning/event-plugin.toml",
        )]))
        .unwrap();

        assert_eq!(
            settings.config.unwrap(),
            std::path::PathBuf::from("/etc/lightning/event-plugin.toml")
        );
    }

    #[test]
    fn reads_events_from_toml() {
        let file = File::from_str(
            r#"
            [event-plugin]
            events_list = ["connect", "disconnect", "invoice_payment"]
            "#,
            FileFormat::Toml,
        );

        assert_eq!(
            resolve_event_types_from(config::Config::builder().add_source(file), None).unwrap(),
            ["connect", "disconnect", "invoice_payment"]
        );
    }

    #[test]
    fn environment_overrides_toml_and_parses_the_list() {
        let file = File::from_str(
            r#"
            [event-plugin]
            events_list = ["connect"]
            "#,
            FileFormat::Toml,
        );

        assert_eq!(
            resolve_event_types_from(
                config::Config::builder().add_source(file),
                read_environment(environment(&[(
                    "EVENT_PLUGIN_EVENTS",
                    " disconnect, invoice_payment , ,",
                )]))
                .unwrap()
                .events,
            )
            .unwrap(),
            ["disconnect", "invoice_payment"]
        );
    }

    #[test]
    fn rejects_empty_and_wildcard_event_lists() {
        let empty = resolve_event_types_from(
            config::Config::builder(),
            read_environment(environment(&[("EVENT_PLUGIN_EVENTS", "")]))
                .unwrap()
                .events,
        )
        .unwrap_err();
        assert!(empty.to_string().contains("at least one event type"));

        let wildcard = resolve_event_types_from(
            config::Config::builder(),
            read_environment(environment(&[("EVENT_PLUGIN_EVENTS", "connect, *")]))
                .unwrap()
                .events,
        )
        .unwrap_err();
        assert!(wildcard.to_string().contains("wildcard"));
    }
}
