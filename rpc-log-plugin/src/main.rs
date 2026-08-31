use anyhow::{Context, Result};
use cln_plugin::options::{ConfigOption, StringConfigOption};
use cln_plugin::{Builder, Plugin};
use cln_rpc::ClnRpc;
use cln_rpc::hooks::events::RpcCommandEvent;
use cln_rpc::model::requests::GetinfoRequest;
use cln_rpc::primitives::{JsonObjectOrArray, JsonScalar, PublicKey};
use google_cloud_storage::client::Storage;
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

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

// const SILENT_OPTION_NAME: &str = "silent";
// pub const SILENT_OPTION: DefaultBooleanConfigOption = ConfigOption::new_bool_with_default(
//     SILENT_OPTION_NAME,
//     false,
//     "If specified - plugin will not send logs to the GCS bucket",
// );

const BUCKET_OPTION_NAME: &str = "log-bucket";
const BUCKET_OPTION: StringConfigOption =
    ConfigOption::new_str_no_default(BUCKET_OPTION_NAME, "A GCS bucket to store logs in");

// According to google_cloud_storage::client::Storage::write_object documentation
const GCS_RESOURCE_NAME_PREFIX: &str = "projects/_/buckets/";

#[derive(Clone)]
struct State {
    pub peer_id: PublicKey,
    pub bucket_name: String,
    pub client: Storage,
}

#[tokio::main]
async fn main() -> Result<()> {
    let configured = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(BUCKET_OPTION)
        .hook("rpc_command", on_hook_rpc_command)
        .dynamic()
        .configure()
        .await?;

    let Some(configured) = configured else {
        // CLN shut down during configuration
        return Ok(());
    };

    let cfg = configured.configuration();
    let rpc_path = Path::new(&cfg.lightning_dir).join(&cfg.rpc_file);
    let peer_id = get_my_peer_id(rpc_path).await?;

    let state = State {
        bucket_name: format!(
            "{}{}",
            GCS_RESOURCE_NAME_PREFIX,
            configured
                .option(&BUCKET_OPTION)?
                .context("No bucket name provided")?
        ),
        // Automatically uses Application Default Credentials.
        // In GKE, these come from Workload Identity.
        client: Storage::builder().build().await?,
        peer_id,
    };

    let plugin = configured.start(state).await?;
    plugin.join().await?;

    Ok(())
}

async fn on_hook_rpc_command(p: Plugin<State>, v: JsonValue) -> Result<JsonValue> {
    let rpc_command_hook: RpcCommandEvent = serde_json::from_value(v)?;

    let body = match rpc_command_hook.rpc_command.params {
        JsonObjectOrArray::Object(object) => JsonObjectOrArray::Object(
            object
                .into_iter()
                .map(|(key, object)| (key, sanitize_json(object)))
                .collect(),
        ),
        JsonObjectOrArray::Array(array) => {
            JsonObjectOrArray::Array(array.into_iter().map(sanitize_json).collect())
        }
    };

    upload_rpc_log(
        &p.state().client,
        &p.state().bucket_name,
        &RpcLog {
            method: &rpc_command_hook.rpc_command.method,
            caller: None,
            request_id: rpc_command_hook.rpc_command.id,
            body,
            peer_id: &p.state().peer_id.to_string(),
        },
    )
    .await?;

    // We do not interrupt command execution
    Ok(json!({"result": "continue"}))
}

#[derive(Serialize)]
struct RpcLog<'a> {
    method: &'a str,
    request_id: JsonScalar,
    body: JsonObjectOrArray,
    caller: Option<&'a str>,
    peer_id: &'a str,
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

async fn get_my_peer_id(rpc_path: PathBuf) -> Result<PublicKey> {
    let mut cln_rpc = ClnRpc::new(&rpc_path).await?;
    let info = cln_rpc.call_typed(&GetinfoRequest {}).await?;
    Ok(info.id)
}

/// Recursively checks JSON for sensitive (danger) keys and fills their values with "***"
/// Credits to @erdoganishe
fn sanitize_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(map) => JsonValue::Object(
            map.into_iter()
                .map(|(k, v)| {
                    if is_sensitive_key(&k) {
                        (k, JsonValue::String("***".into()))
                    } else {
                        (k, sanitize_json(v))
                    }
                })
                .collect(),
        ),
        JsonValue::Array(arr) => JsonValue::Array(arr.into_iter().map(sanitize_json).collect()),
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
