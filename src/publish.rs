//! Publish consumed donations to RabbitMQ.

use anyhow::{bail, Context, Result};
use lapin::options::{BasicPublishOptions, ConfirmSelectOptions, ExchangeDeclareOptions};
use lapin::types::FieldTable;
use lapin::{
    BasicProperties, Channel, Confirmation, Connection, ConnectionProperties, ExchangeKind,
    PublisherConfirm,
};
use serde_json::Value;

/// The durable fanout exchange consumed donations are published to.
pub const EXCHANGE: &str = "donations";

/// Fanout ignores the routing key; keep it empty.
pub const ROUTING_KEY: &str = "";

/// The message body for a donation: the raw gateway JSON, re-encoded without
/// any field remapping so empty/null and unknown fields pass through unchanged.
pub fn message_body(donation: &Value) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(donation)
}

/// Properties marking a published message persistent (delivery mode 2), so it
/// survives a broker restart.
pub fn publish_properties() -> BasicProperties {
    BasicProperties::default().with_delivery_mode(2)
}

/// A sink for donation message bodies.
pub trait Publisher {
    /// Publish one message body. An `Err` means the message was not published.
    async fn publish(&mut self, body: &[u8]) -> Result<()>;
}

/// Publish a donation, reporting and retrying once on failure.
///
/// A failed publish is reported on standard error and retried; if the retry
/// fails too, the donation is reported as unpublished. This never panics, so a
/// broker hiccup cannot tear down the donation stream.
pub async fn publish_with_retry<P: Publisher>(publisher: &mut P, body: &[u8]) {
    if let Err(e) = publisher.publish(body).await {
        eprintln!("publish failed: {e:#}; retrying");
        if let Err(e) = publisher.publish(body).await {
            eprintln!("publish failed again: {e:#}; donation not published");
        }
    }
}

/// Publishes donations to a RabbitMQ fanout, (re)connecting on demand.
pub struct RabbitPublisher {
    dsn: String,
    connection: Option<Connection>,
    channel: Option<Channel>,
}

impl RabbitPublisher {
    /// Build a publisher for `dsn`. The broker is not contacted until the
    /// first publish (or [`RabbitPublisher::preflight`]), so a broker outage
    /// at startup cannot tear down the donation stream.
    pub fn new(dsn: impl Into<String>) -> Self {
        Self {
            dsn: dsn.into(),
            connection: None,
            channel: None,
        }
    }

    /// Connect up front so a broker problem is visible before the first
    /// donation. A failure is not fatal: the next publish retries.
    pub async fn preflight(&mut self) -> Result<()> {
        self.ensure_connected().await
    }

    /// Open a connection and channel, declare the fanout, and enable publisher
    /// confirms — unless a live channel already exists.
    async fn ensure_connected(&mut self) -> Result<()> {
        if self.channel.is_some() {
            return Ok(());
        }
        let properties = ConnectionProperties::default().enable_auto_recover();
        let connection = Connection::connect(&self.dsn, properties)
            .await
            .context("connecting to RabbitMQ")?;
        let channel = connection
            .create_channel()
            .await
            .context("opening a RabbitMQ channel")?;
        channel
            .exchange_declare(
                EXCHANGE.into(),
                ExchangeKind::Fanout,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .context("declaring the donations fanout")?;
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .context("enabling publisher confirms")?;
        self.connection = Some(connection);
        self.channel = Some(channel);
        Ok(())
    }

    /// Drop the connection and channel so the next publish reconnects.
    fn disconnect(&mut self) {
        self.channel = None;
        self.connection = None;
    }

    async fn try_publish(&mut self, body: &[u8]) -> Result<()> {
        self.ensure_connected().await?;
        let channel = self.channel.as_ref().expect("channel exists after connect");
        let confirm: PublisherConfirm = channel
            .basic_publish(
                EXCHANGE.into(),
                ROUTING_KEY.into(),
                BasicPublishOptions::default(),
                body,
                publish_properties(),
            )
            .await
            .context("publishing to RabbitMQ")?;
        match confirm.await.context("awaiting the broker confirmation")? {
            Confirmation::Ack(_) => Ok(()),
            Confirmation::Nack(_) => bail!("the broker nacked the published donation"),
            Confirmation::NotRequested => {
                bail!("the broker did not confirm the published donation")
            }
        }
    }
}

impl Publisher for RabbitPublisher {
    async fn publish(&mut self, body: &[u8]) -> Result<()> {
        let result = self.try_publish(body).await;
        if result.is_err() {
            self.disconnect();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use serde_json::json;

    #[test]
    fn fanout_name_and_empty_routing_key() {
        assert_eq!(EXCHANGE, "donations");
        assert_eq!(ROUTING_KEY, "", "fanout ignores the routing key");
    }

    #[test]
    fn published_messages_are_persistent() {
        assert_eq!(publish_properties().delivery_mode(), &Some(2));
    }

    #[test]
    fn body_preserves_null_empty_and_unknown_fields() {
        let donation = json!({
            "id": 1,
            "name": null,
            "message": "",
            "amount": 333,
            "custom_unknown": { "a": 1 }
        });
        let body = message_body(&donation).unwrap();
        let round_tripped: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(round_tripped, donation);
        assert!(round_tripped["name"].is_null());
        assert_eq!(round_tripped["message"], "");
        assert_eq!(round_tripped["custom_unknown"]["a"], 1);
    }

    /// A publisher that records bodies and fails until `fail_first` calls have
    /// happened, so retry behaviour can be exercised without a broker.
    struct FakePublisher {
        calls: Vec<Vec<u8>>,
        fail_first: usize,
    }

    impl FakePublisher {
        fn new(fail_first: usize) -> Self {
            Self {
                calls: Vec::new(),
                fail_first,
            }
        }
    }

    impl Publisher for FakePublisher {
        async fn publish(&mut self, body: &[u8]) -> Result<()> {
            self.calls.push(body.to_vec());
            if self.calls.len() <= self.fail_first {
                return Err(anyhow!("broker unavailable"));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn retry_republishes_after_a_failure() {
        let mut publisher = FakePublisher::new(1);
        publish_with_retry(&mut publisher, b"donation").await;
        assert_eq!(
            publisher.calls.len(),
            2,
            "the failed publish must be retried"
        );
        assert_eq!(publisher.calls[1], b"donation");
    }

    #[tokio::test]
    async fn a_healthy_publisher_is_called_once() {
        let mut publisher = FakePublisher::new(0);
        publish_with_retry(&mut publisher, b"donation").await;
        assert_eq!(publisher.calls.len(), 1);
    }

    #[tokio::test]
    async fn a_permanently_failing_broker_does_not_panic_the_loop() {
        let mut publisher = FakePublisher::new(usize::MAX);
        publish_with_retry(&mut publisher, b"donation").await;
        assert_eq!(
            publisher.calls.len(),
            2,
            "one retry is attempted, then the donation is reported as unpublished"
        );
    }
}
