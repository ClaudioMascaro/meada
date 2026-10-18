use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};

use crate::pricing;

fn apply_shipment() {}

/// A stream client: the function that subscribes to the topic handles it.
pub async fn consume_shipments() {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", "kafka:9092")
        .set("group.id", "api")
        .create()
        .expect("consumer");
    consumer.subscribe(&["orders.shipped"]).expect("subscribe");
    loop {
        if consumer.recv().await.is_ok() {
            apply_shipment();
            let _ = pricing::promotions().await;
        }
    }
}
