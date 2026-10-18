use async_nats::ConnectOptions;

pub async fn publish_all() -> Result<(), async_nats::Error> {
    // The crate is written in full, with no `use`.
    let client = async_nats::connect("nats://localhost:4222").await?;
    client.publish("orders.created", "o".into()).await?;
    client.publish("orders.shipped", "s".into()).await?;
    client.publish(format!("audit.{}", "login"), "a".into()).await?;
    Ok(())
}

pub async fn publish_with_options() -> Result<(), async_nats::Error> {
    let client = ConnectOptions::new()
        .name("publisher")
        .connect("nats://localhost:4222")
        .await?;
    client.publish("mail.sent", "m".into()).await?;
    let subject = std::env::var("SUBJECT").unwrap();
    client.publish(subject, "u".into()).await?;
    Ok(())
}

pub async fn listen_orders() -> Result<(), async_nats::Error> {
    let client = async_nats::connect("nats://localhost:4222").await?;
    let mut created = client.subscribe("orders.created").await?;
    let _ = created.next().await;
    // Wildcards match as `inferred`.
    let mut all = client.subscribe("orders.*").await?;
    let _ = all.next().await;
    Ok(())
}

pub async fn listen_queue() -> Result<(), async_nats::Error> {
    let client = async_nats::connect("nats://localhost:4222").await?;
    let mut audit = client.queue_subscribe("audit.>", "workers".into()).await?;
    let _ = audit.next().await;
    let mut mail = client.subscribe("mail.sent").await?;
    let _ = mail.next().await;
    Ok(())
}

pub fn not_nats(bus: &Bus) {
    bus.publish("never.local");
    bus.subscribe("never.local");
}

pub struct Bus;

impl Bus {
    pub fn publish(&self, _subject: &str) {}
    pub fn subscribe(&self, _subject: &str) {}
}
