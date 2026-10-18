use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer, StreamConsumer};
use rdkafka::producer::{BaseProducer, BaseRecord, FutureProducer, FutureRecord};
use std::time::Duration;

const REGION: &str = "eu";

/// A producer built by a function: the caller's `producer` traces back to the config.
fn make_producer() -> FutureProducer {
    ClientConfig::new()
        .set("bootstrap.servers", "kafka:9092")
        .create()
        .expect("producer")
}

pub async fn produce_all() {
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", "kafka:9092")
        .create()
        .expect("producer");
    let wait = Duration::from_secs(0);
    // The record is built in place, with its payload and key after the topic.
    let _ = producer
        .send(FutureRecord::to("orders").payload("o").key("k"), wait)
        .await;
    // The record is a `let`, and the topic a `format!`.
    let record = FutureRecord::to("payments").payload("p").key("k");
    let _ = producer.send(record, wait).await;
    let refunds = format!("refunds.{REGION}");
    let _ = producer
        .send(FutureRecord::to(&refunds).payload("r").key("k"), wait)
        .await;
    // A producer a function made.
    let other = make_producer();
    let _ = other
        .send(FutureRecord::to("invoices").payload("i").key("k"), wait)
        .await;
}

pub fn base_produce() {
    let producer: BaseProducer = ClientConfig::new().create().expect("producer");
    let _ = producer.send(BaseRecord::to("audits").payload("a").key("k"));
}

/// The producer is a parameter: each caller passes one it built, and names the topic.
async fn emit(producer: &FutureProducer, topic: &str) {
    let _ = producer
        .send(
            FutureRecord::to(topic).payload("e").key("k"),
            Duration::from_secs(0),
        )
        .await;
}

pub async fn emit_refund() {
    let producer = make_producer();
    emit(&producer, "retries").await;
}

pub async fn produce_unknown() {
    let producer = make_producer();
    let topic = std::env::var("EVENTS_TOPIC").unwrap();
    let _ = producer
        .send(
            FutureRecord::to(&topic).payload("u").key("k"),
            Duration::from_secs(0),
        )
        .await;
}

pub async fn consume_orders() {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("group.id", "orders")
        .create()
        .expect("consumer");
    consumer.subscribe(&["orders", "payments"]).expect("subscribe");
    loop {
        let _ = consumer.recv().await;
    }
}

pub fn consume_refunds() {
    let consumer: BaseConsumer = ClientConfig::new()
        .set("group.id", "refunds")
        .create()
        .expect("consumer");
    let topics = ["refunds.eu", "retries", "invoices", "audits"];
    consumer.subscribe(&topics).expect("subscribe");
    let _ = consumer.poll(Duration::from_secs(1));
}

/// A topic that starts with `^` is a regular expression: it names no topic.
pub fn consume_pattern() {
    let consumer: BaseConsumer = ClientConfig::new().create().expect("consumer");
    consumer.subscribe(&["^audit-.*"]).expect("subscribe");
}

/// Not the library's: a channel's `send`, and a transport of the repository that is told a
/// topic.
pub struct Transport;

impl Transport {
    pub fn send(&self, _topic: &str) {}
}

pub fn not_rdkafka() {
    let (tx, _rx) = std::sync::mpsc::channel::<String>();
    tx.send("never".to_string()).ok();
    let transport = Transport;
    transport.send("never.local");
}
