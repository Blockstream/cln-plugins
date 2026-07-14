use anyhow::{Result, bail};
use lapin::options::BasicPublishOptions;
use lapin::{BasicProperties, Channel};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::error;

/// MessageBroker handles publishing events to RabbitMQ
/// Uses Arc<RwLock<Option<Channel>>> so the channel can be cleared and
/// replaced if the connection drops, enabling future reconnect logic
#[derive(Clone)]
pub struct MessageBroker {
    amqp_channel: Arc<RwLock<Option<Channel>>>,
    exchange_name: String,
    source_kind: i32,
    source_node_id: Vec<u8>,
    producer_version: String,
}

impl MessageBroker {
    pub fn new(
        amqp_channel: Arc<RwLock<Option<Channel>>>,
        exchange_name: String,
        source_kind: i32,
        source_node_id: Vec<u8>,
        producer_version: String,
    ) -> Self {
        Self {
            amqp_channel,
            exchange_name,
            source_kind,
            source_node_id,
            producer_version,
        }
    }

    /// Publishes an event to RabbitMQ.
    /// Returns Ok if the message was published successfully or an error if
    /// the channel is not initialized or publishing fails
    pub async fn publish_bytes(&self, payload: Vec<u8>) -> Result<()> {
        let guard = self.amqp_channel.read().await;
        let Some(ch) = guard.as_ref() else {
            error!("No AMQP channel — event will be lost");
            bail!(
                "No AMQP channel available, cannot publish to {}",
                self.exchange_name
            );
        };

        ch.basic_publish(
            self.exchange_name.as_str().into(),
            "".into(), // Will use an empty routing key since we are sending only events to a single queue.
            BasicPublishOptions::default(),
            &payload,
            BasicProperties::default(),
        )
        .await?
        .await?;

        Ok(())
    }

    pub fn source_kind(&self) -> i32 {
        self.source_kind
    }

    pub fn source_node_id(&self) -> &[u8] {
        &self.source_node_id
    }

    pub fn producer_version(&self) -> &str {
        &self.producer_version
    }
}
