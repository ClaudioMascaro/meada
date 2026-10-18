use lapin::options::{BasicConsumeOptions, BasicPublishOptions, QueueBindOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Connection, ConnectionProperties};

pub async fn publish_all(url: &str) -> lapin::Result<()> {
    let connection = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    let options = BasicPublishOptions::default();
    // The default exchange: the queue the routing key names.
    channel
        .basic_publish("", "jobs", options, b"j", BasicProperties::default())
        .await?;
    // A named exchange: the queues bound to it whose pattern the key matches.
    channel
        .basic_publish("events", "order.created", options, b"o", BasicProperties::default())
        .await?;
    channel
        .basic_publish("events", "invoice.paid", options, b"i", BasicProperties::default())
        .await?;
    Ok(())
}

pub async fn bind_all(url: &str) -> lapin::Result<()> {
    let channel = Connection::connect(url, ConnectionProperties::default())
        .await?
        .create_channel()
        .await?;
    channel
        .queue_bind(
            "audit",
            "events",
            "order.#",
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    // A binding that cannot be evaluated is dropped.
    let pattern = std::env::var("PATTERN").unwrap();
    channel
        .queue_bind(
            "mail",
            "events",
            &pattern,
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await?;
    Ok(())
}

pub async fn work_jobs(url: &str) -> lapin::Result<()> {
    let connection = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    let _consumer = channel
        .basic_consume(
            "jobs",
            "worker",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    Ok(())
}

pub async fn audit(url: &str) -> lapin::Result<()> {
    let connection = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    let queue = std::env::var("AUDIT_QUEUE").unwrap();
    let _consumer = channel
        .basic_consume(
            "audit",
            "auditor",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    // A queue read from the environment is an unresolved consumer.
    let _other = channel
        .basic_consume(
            &queue,
            "auditor",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    Ok(())
}

/// A publish whose exchange is not known is an unresolved producer.
pub async fn publish_unknown(url: &str) -> lapin::Result<()> {
    let connection = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    let exchange = std::env::var("EXCHANGE").unwrap();
    channel
        .basic_publish(
            &exchange,
            "anything",
            BasicPublishOptions::default(),
            b"u",
            BasicProperties::default(),
        )
        .await?;
    Ok(())
}
