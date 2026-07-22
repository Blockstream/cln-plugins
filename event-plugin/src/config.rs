use anyhow::Result;
use config::{Config, Environment, File};
use std::path::PathBuf;

const ENV_PREFIX: &str = "EVENT_PLUGIN";
const ENV_CONFIG_VAR: &str = "EVENT_PLUGIN_CONFIG";
const ENV_EVENTS_VAR: &str = "EVENT_PLUGIN_EVENTS";
const CFG_EVENTS_KEY: &str = "events";

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
///
/// 1. `EVENT_PLUGIN_EVENTS`, as a comma-separated list:
///
/// ```text
/// EVENT_PLUGIN_EVENTS=connect,disconnect,invoice_payment
/// ```
///
/// 2. `events` in the TOML file selected by `EVENT_PLUGIN_CONFIG`:
///
/// ```toml
/// events = [
///     "connect",
///     "disconnect",
///     "invoice_payment",
/// ]
/// ```
///
/// 3. [`DEFAULT_EVENTS`].
pub fn resolve_event_types() -> Result<Vec<String>> {
    let env = Environment::with_prefix(ENV_PREFIX)
        .try_parsing(true)
        .list_separator(",")
        .with_list_parse_key(CFG_EVENTS_KEY)
        .ignore_empty(true);

    let mut builder = Config::builder().set_default(CFG_EVENTS_KEY, DEFAULT_EVENTS.to_vec())?;

    if let Some(path) = std::env::var_os(ENV_CONFIG_VAR) {
        builder = builder.add_source(File::from(PathBuf::from(path)).required(false));
    }

    let events: Vec<String> = builder.add_source(env).build()?.get(CFG_EVENTS_KEY)?;
    Ok(events)
}

#[cfg(test)]
mod test {
    use super::{DEFAULT_EVENTS, ENV_CONFIG_VAR, ENV_EVENTS_VAR, resolve_event_types};
    use std::fs;
    use std::path::PathBuf;
    fn clear_environment() {
        // SAFETY: these tests are expected to run serially.
        unsafe {
            std::env::remove_var(ENV_CONFIG_VAR);
            std::env::remove_var(ENV_EVENTS_VAR);
        }
    }

    fn set_environment(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: these tests are expected to run serially.
        unsafe { std::env::set_var(key, value) }
    }

    fn test_config_file(contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join("event-plugin-config-test.toml");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn defaults_when_sources_do_not_configure_events() {
        clear_environment();

        assert_eq!(resolve_event_types().unwrap(), DEFAULT_EVENTS);
    }

    #[test]
    fn reads_events_from_toml_file() {
        clear_environment();
        let file = test_config_file(r#"events = ["connect", "disconnect", "invoice_payment"]"#);
        set_environment(ENV_CONFIG_VAR, &file);

        assert_eq!(
            resolve_event_types().unwrap(),
            ["connect", "disconnect", "invoice_payment"]
        );
        clear_environment();
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn environment_overrides_toml_and_parses_the_list() {
        clear_environment();
        let file = test_config_file(r#"events = ["connect"]"#);
        set_environment(ENV_CONFIG_VAR, &file);
        set_environment(ENV_EVENTS_VAR, "disconnect,invoice_payment");

        assert_eq!(
            resolve_event_types().unwrap(),
            ["disconnect", "invoice_payment"]
        );
        clear_environment();
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn ignores_an_empty_events_environment_variable() {
        clear_environment();
        let file = test_config_file(r#"events = ["connect", "warning"]"#);
        set_environment(ENV_CONFIG_VAR, &file);
        set_environment(ENV_EVENTS_VAR, "");

        assert_eq!(resolve_event_types().unwrap(), ["connect", "warning"]);
        clear_environment();
        fs::remove_file(file).unwrap();
    }

    #[test]
    fn missing_optional_config_file_falls_back_to_defaults() {
        clear_environment();
        let missing_path = std::env::temp_dir().join("missing-event-plugin-config.toml");
        set_environment(ENV_CONFIG_VAR, missing_path);

        assert_eq!(resolve_event_types().unwrap(), DEFAULT_EVENTS);
        clear_environment();
    }
}
