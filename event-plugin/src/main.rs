mod broker;
mod config;
mod events;
mod proto;

use anyhow::{Context, Result, bail};
use broker::MessageBroker;
use cln_plugin::Builder;
use cln_plugin::options::ConfigOption;
use config::resolve_event_types;
use events::*;
use lapin::options::{ExchangeDeclareOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::{Connection, ConnectionProperties, ExchangeKind};
use serde::Deserialize;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::RwLock;
use tracing::info;

const DEFAULT_EXCHANGE_NAME: &str = "cln.events";
const DEFAULT_QUEUE_NAME: &str = "events";

#[tokio::main]
async fn main() -> Result<()> {
    let amqp_channel: Arc<RwLock<Option<lapin::Channel>>> = Arc::new(RwLock::new(None));
    let event_types = resolve_event_types();

    let mut builder = Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(ConfigOption::new_str_no_default(
            "rabbitmq-url",
            "AMQP URL: user:pass@host:port/vhost",
        ))
        .option(ConfigOption::new_str_with_default(
            "rabbitmq-exchange",
            DEFAULT_EXCHANGE_NAME,
            "RabbitMQ exchange name for CLN events",
        ))
        .option(ConfigOption::new_str_with_default(
            "rabbitmq-queue",
            DEFAULT_QUEUE_NAME,
            "RabbitMQ queue name for CLN events",
        ))
        .option(ConfigOption::new_str_no_default(
            "source-kind",
            "Event source kind: 'gateway' or 'lsp'",
        ));

    for event_type in event_types.iter().flatten() {
        let handler_event_type = event_type.clone();
        builder = builder.subscribe(event_type, move |p, v| {
            on_event(p, handler_event_type.clone(), v)
        });
    }

    let configured = builder
        .dynamic()
        .configure()
        .await?
        // Fail early and loud, we don't want to run the node without this plugin.
        .context("failed to configure event-plugin")?;

    if let Err(e) = &event_types {
        let msg = format!("{e:#}");
        _ = configured.disable(&msg).await;
        bail!(msg);
    }

    info!("Collecting events={}", event_types?.join(","));

    // Fail if RabbitMQ is not configured, we can not operate without.
    let Some(url) = get_configured_string_option(&configured, "rabbitmq-url") else {
        let msg = "'rabbitmq-url' option is required but not set";
        _ = configured.disable(msg).await;
        bail!(msg);
    };

    // Url contains username and a password to rabbitmq.
    // We need to wrap it in order to prevent possible leakage.
    let url = Secret(url);

    let exchange_name = get_configured_string_option(&configured, "rabbitmq-exchange")
        .unwrap_or_else(|| DEFAULT_EXCHANGE_NAME.to_string());

    let queue_name = get_configured_string_option(&configured, "rabbitmq-queue")
        .unwrap_or_else(|| DEFAULT_QUEUE_NAME.to_string());

    let config = configured.configuration();
    let node_info = rpc_getinfo_node_id(&config.lightning_dir, &config.rpc_file).await?;
    let Some(source_kind_str) = get_configured_string_option(&configured, "source-kind") else {
        let msg = "'source-kind' option is required but not set";
        _ = configured.disable(msg).await;
        bail!(msg);
    };

    let source_kind: i32 = match source_kind_str.as_str() {
        "gateway" => 1, // SOURCE_KIND_GATEWAY
        "lsp" => 2,     // SOURCE_KIND_LSP
        other => {
            configured
                .disable(&format!(
                    "'source-kind' must be 'gateway' or 'lsp', got '{}'",
                    other
                ))
                .await?;
            return Ok(());
        }
    };

    init_event_id_generator(&node_info.node_id);

    let conn = match connect_rabbitmq(url, &exchange_name, &queue_name, amqp_channel.clone()).await
    {
        Ok(conn) => conn,
        Err(e) => {
            configured.disable(&format!("{:#}", e)).await?;
            return Ok(());
        }
    };

    // Keep a handle to clear the channel on shutdown before the connection closes
    let amqp_for_cleanup = amqp_channel.clone();
    let broker = MessageBroker::new(
        amqp_channel,
        exchange_name.clone(),
        source_kind,
        node_info.node_id,
        node_info.version,
    );

    info!(
        "Event collector plugin started: exchange={}, queue={}",
        &exchange_name, &queue_name
    );

    let plugin = configured.start(broker).await?;
    plugin.join().await?;

    // Shut down AMQP before the process exits so the lapin IO-loop thread
    // finishes cleanly while the cln-plugin logging subscriber is still alive
    *amqp_for_cleanup.write().await = None;
    let _ = conn.close(0, "".into()).await;

    Ok(())
}

struct NodeInfo {
    node_id: Vec<u8>,
    version: String,
}

async fn rpc_getinfo_node_id(lightning_dir: &str, rpc_file: &str) -> Result<NodeInfo> {
    let socket_path = PathBuf::from(lightning_dir).join(rpc_file);
    let mut stream = UnixStream::connect(&socket_path).await.with_context(|| {
        format!(
            "failed to connect to RPC socket at {}",
            socket_path.display()
        )
    })?;

    let mut request_bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getinfo",
        "params": {}
    }))?;
    request_bytes.extend_from_slice(b"\n\n");
    stream.write_all(&request_bytes).await?;

    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    let response: serde_json::Value = loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("RPC socket closed before response");
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf) {
            break val;
        }
    };

    stream.shutdown().await.ok();
    drop(stream);

    let id_hex = response
        .pointer("/result/id")
        .and_then(|v| v.as_str())
        .context("getinfo response missing result.id")?;

    let version = response
        .pointer("/result/version")
        .and_then(|v| v.as_str())
        .context("getinfo response missing field result.version")?
        .to_string();

    // Decode hex node_id to raw bytes (33 bytes for compressed secp256k1 pubkey)
    let node_id = hex::decode(id_hex).context("failed to decode node_id hex")?;

    Ok(NodeInfo { node_id, version })
}

fn get_configured_string_option(
    plugin: &cln_plugin::ConfiguredPlugin<MessageBroker, tokio::io::Stdin, tokio::io::Stdout>,
    option_name: &str,
) -> Option<String> {
    plugin
        .option_str(option_name)
        .ok()
        .flatten()
        .and_then(|v| match v {
            cln_plugin::options::Value::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
}

async fn connect_rabbitmq(
    url: Secret,
    exchange_name: &str,
    queue_name: &str,
    amqp_channel: Arc<RwLock<Option<lapin::Channel>>>,
) -> Result<Connection> {
    let conn = Connection::connect(url.inner(), ConnectionProperties::default())
        .await
        .with_context(|| format!("failed to connect to rabbit_mq at {}", url))?;
    let ch = conn.create_channel().await?;

    ch.exchange_declare(
        exchange_name.into(),
        ExchangeKind::Direct,
        ExchangeDeclareOptions {
            passive: true,
            ..Default::default()
        },
        FieldTable::default(),
    )
    .await
    .with_context(|| format!("failed to verify exchange '{}' in RabbitMQ", exchange_name))?;

    ch.queue_declare(
        queue_name.into(),
        QueueDeclareOptions {
            passive: true,
            ..Default::default()
        },
        FieldTable::default(),
    )
    .await
    .with_context(|| format!("failed to verify queue '{}' in RabbitMQ", queue_name))?;

    *amqp_channel.write().await = Some(ch);

    Ok(conn)
}

/// A wrapper around a String variable that is intended to hold a secret value.
/// It implements [`Display`] which hides the secret for being accidentally
/// printed.
#[derive(Deserialize)]
pub struct Secret(String);

impl Secret {
    pub fn inner(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for Secret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "*****")
    }
}

impl Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "*****")
    }
}

#[cfg(test)]
mod test {
    use crate::Secret;

    #[test]
    fn test_secret_display() {
        let secret = Secret("foobar".to_owned());

        assert_eq!(format!("{}", secret), "*****");
    }

    #[test]
    fn test_secret_debug() {
        let secret = Secret("foobar".to_owned());

        assert_eq!(format!("{:?}", secret), "*****");
    }
}
