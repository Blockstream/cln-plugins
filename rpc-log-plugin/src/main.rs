mod rune;

use crate::rune::rune_tag;
use anyhow::{Context, Result};
use cln_plugin::options::{ConfigOption, DefaultStringConfigOption, StringConfigOption};
use cln_plugin::{Builder, Plugin};
use cln_rpc::hooks::events::RpcCommandEvent;
use cln_rpc::primitives::{JsonObjectOrArray, JsonScalar};
use google_cloud_auth::credentials::anonymous::Builder as AnonymousCredentials;
use google_cloud_storage::client::Storage;
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use std::collections::HashSet;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

const RUNE_KEY: &str = "rune";

// A list of fields to remove from logs.
const DANGER_FIELDS: [&str; 13] = [
    "rune",
    "hsmsecret",
    "passphrase",
    "preimage",
    "payment_preimage",
    "payment_secret",
    "session_key",
    "shared_secrets",
    "payer_metadata",
    "secret",
    "private_key",
    "seed",
    "mnemonic",
];

const EMULATOR_OPTION_NAME: &str = "log-emulator-host";
pub const EMULATOR_OPTION: StringConfigOption = ConfigOption::new_str_no_default(
    EMULATOR_OPTION_NAME,
    "If specified - plugin will send logs to the local bucket emulator (see tests)",
);

const RPC_LIST_OPTION_NAME: &str = "log-rpc-list";
const RPC_LIST_OPTION: DefaultStringConfigOption = ConfigOption::new_str_with_default(
    RPC_LIST_OPTION_NAME,
    "checkrune",
    "A list of comma separated rpc commands to log",
);

const BUCKET_OPTION_NAME: &str = "log-bucket";
const BUCKET_OPTION: StringConfigOption =
    ConfigOption::new_str_no_default(BUCKET_OPTION_NAME, "A GCS bucket to store logs in");

// According to google_cloud_storage::client::Storage::write_object documentation
const GCS_RESOURCE_NAME_PREFIX: &str = "projects/_/buckets/";

#[derive(Clone)]
struct State {
    bucket_name: String,
    client: Storage,
    rpc_list: HashSet<String>,
}

impl State {
    fn method_in_log_list(&self, method: &str) -> bool {
        if self.rpc_list.is_empty() {
            return true;
        }

        self.rpc_list.contains(method)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let configured = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(BUCKET_OPTION)
        .option(RPC_LIST_OPTION)
        .option(EMULATOR_OPTION)
        .hook("rpc_command", on_hook_rpc_command)
        .dynamic()
        .configure()
        .await?;

    let Some(configured) = configured else {
        // CLN shut down during configuration
        return Ok(());
    };

    let rpc_list = configured
        .option(&RPC_LIST_OPTION)?
        .split(',')
        .map(str::trim)
        .filter(|method| !method.is_empty())
        .map(str::to_owned)
        .collect();

    let state = State {
        bucket_name: format!(
            "{}{}",
            GCS_RESOURCE_NAME_PREFIX,
            configured
                .option(&BUCKET_OPTION)?
                .context("No bucket name provided")?
        ),
        client: storage_client(configured.option(&EMULATOR_OPTION)?).await?,
        rpc_list,
    };

    let plugin = configured.start(state).await?;
    plugin.join().await?;

    Ok(())
}

async fn storage_client(emulator_endpoint: Option<String>) -> Result<Storage> {
    let builder = Storage::builder();

    if let Some(endpoint) = emulator_endpoint {
        // Using local emulator without auth
        return Ok(builder
            .with_endpoint(endpoint)
            .with_credentials(AnonymousCredentials::new().build())
            .build()
            .await?);
    }

    // Uses Application Default Credentials.
    // In GKE, these come from Workload Identity.
    Ok(builder.build().await?)
}

async fn on_hook_rpc_command(p: Plugin<State>, v: JsonValue) -> Result<JsonValue> {
    let rpc_command_hook: RpcCommandEvent = serde_json::from_value(v)?;

    if p.state()
        .method_in_log_list(&rpc_command_hook.rpc_command.method)
    {
        let operator = rune_from_json_array(&rpc_command_hook.rpc_command.params);
        let body = sanitize_json_array(rpc_command_hook.rpc_command.params);

        upload_rpc_log(
            &p.state().client,
            &p.state().bucket_name,
            &RpcLog {
                method: &rpc_command_hook.rpc_command.method,
                caller: operator,
                request_id: rpc_command_hook.rpc_command.id,
                body,
            },
        )
        .await?;
    }

    // We do not interrupt command execution
    Ok(json!({"result": "continue"}))
}

#[derive(Serialize)]
struct RpcLog<'a> {
    method: &'a str,
    request_id: JsonScalar,
    body: JsonObjectOrArray,
    #[serde(skip_serializing_if = "Option::is_none")]
    caller: Option<String>,
}

async fn upload_rpc_log(client: &Storage, bucket: &str, log: &RpcLog<'_>) -> Result<String> {
    let timestamp = OffsetDateTime::now_utc();
    let id = Uuid::new_v4();
    let object_name = format!("rpc/{}-{id}.json", timestamp.format(&Rfc3339)?,);

    client
        .write_object(bucket, &object_name, serde_json::to_string(log)?)
        .send_buffered()
        .await?;

    Ok(object_name)
}

/// Recursively checks JSON (JsonObjectOrArray) for rune and outputs its operator tag if present
/// (We need this because rpc_command hook defines params as JsonObjectOrArray)
fn rune_from_json_array(value: &JsonObjectOrArray) -> Option<String> {
    match value {
        JsonObjectOrArray::Object(map) => map.iter().find_map(|(k, v)| rune_from_json_value(k, v)),
        JsonObjectOrArray::Array(arr) => arr.iter().find_map(|v| rune_from_json_value("", v)),
    }
}

/// Recursively checks JSON (Value) for rune and outputs its operator tag if present
fn rune_from_json_value(key: &str, value: &JsonValue) -> Option<String> {
    if key.to_lowercase().contains(RUNE_KEY)
        && let Some(rune) = value.as_str()
        && let Ok(result) = rune_tag(rune)
        && let Some(operator) = result
    {
        return Some(operator);
    }

    match value {
        JsonValue::Object(map) => map.iter().find_map(|(k, v)| rune_from_json_value(k, v)),
        JsonValue::Array(arr) => arr.iter().find_map(|v| rune_from_json_value("", v)),
        _ => None,
    }
}

/// Recursively checks JSON (JsonObjectOrArray) for sensitive (danger) keys and fills their values with "***"
/// (We need this because rpc_command hook defines params as JsonObjectOrArray)
fn sanitize_json_array(value: JsonObjectOrArray) -> JsonObjectOrArray {
    match value {
        JsonObjectOrArray::Object(map) => JsonObjectOrArray::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let result = sanitize_json_value(&k, v);
                    (k, result)
                })
                .collect(),
        ),
        JsonObjectOrArray::Array(arr) => JsonObjectOrArray::Array(
            arr.into_iter()
                .map(|v| sanitize_json_value("", v))
                .collect(),
        ),
    }
}

/// Recursively checks JSON (Value) for sensitive (danger) keys and fills their values with "***"
/// Credits to @erdoganishe
fn sanitize_json_value(key: &str, value: JsonValue) -> JsonValue {
    if is_sensitive_key(key) {
        return JsonValue::String("***".into());
    }

    match value {
        JsonValue::Object(map) => JsonValue::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let result = sanitize_json_value(&k, v);
                    (k, result)
                })
                .collect(),
        ),
        JsonValue::Array(arr) => JsonValue::Array(
            arr.into_iter()
                .map(|v| sanitize_json_value("", v))
                .collect(),
        ),
        other => other,
    }
}

fn is_sensitive_key(key_to_check: &str) -> bool {
    for key in DANGER_FIELDS.iter() {
        if key_to_check.to_lowercase().contains(key) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::{rune_from_json_value, sanitize_json_value};
    use anyhow::Result;

    #[test]
    fn test_sanitize_json() -> Result<()> {
        let example = r#"{"rune":"12345"}"#;
        let sanitize_example = sanitize_json_value("", serde_json::from_str(example)?);
        let result_json = sanitize_example.to_string();
        assert_eq!(result_json, r#"{"rune":"***"}"#);
        Ok(())
    }

    #[test]
    fn test_sanitize_json_arr() -> Result<()> {
        let example = r#"{"key":[{"rune_1":"12345"},{"rune_2":"12345"}]}"#;
        let sanitize_example = sanitize_json_value("", serde_json::from_str(example)?);
        let result_json = sanitize_example.to_string();
        assert_eq!(
            result_json,
            r#"{"key":[{"rune_1":"***"},{"rune_2":"***"}]}"#
        );
        Ok(())
    }

    #[test]
    fn test_sanitize_json_full_object() -> Result<()> {
        let example = r#"{"secret":{"key":"value"}}"#;
        let sanitize_example = sanitize_json_value("", serde_json::from_str(example)?);
        let result_json = sanitize_example.to_string();
        assert_eq!(result_json, r#"{"secret":"***"}"#);
        Ok(())
    }

    #[test]
    fn test_find_operator() -> Result<()> {
        let example =
            r#"{"params":{"rune":"_A7OO-xeVLnHX-zRLOhNGg3DDDMCvET1DZN-72WkbkVvcGVyYXRvciNPbGVn"}}"#;
        let result_json = rune_from_json_value("", &serde_json::from_str(example)?);
        assert_eq!(result_json, Some("Oleg".to_owned()));
        Ok(())
    }

    #[test]
    fn test_find_operator_in_arr() -> Result<()> {
        let example = r#"{"params":[{"rune":"_A7OO-xeVLnHX-zRLOhNGg3DDDMCvET1DZN-72WkbkVvcGVyYXRvciNPbGVn"}]}"#;
        let result_json = rune_from_json_value("", &serde_json::from_str(example)?);
        assert_eq!(result_json, Some("Oleg".to_owned()));
        Ok(())
    }
}
