use rdkafka::producer::{FutureProducer, FutureRecord};
use std::time::Duration;

/// The producer is the repository's own type: the parameter does not trace to the library.
pub async fn shadowed(producer: &Outbox) {
    let _ = producer
        .send(FutureRecord::to("never.shadowed"), Duration::from_secs(0))
        .await;
}

pub struct Outbox;

impl Outbox {
    pub async fn send(&self, _record: FutureRecord<'_, str, str>, _wait: Duration) -> Result<(), ()> {
        Ok(())
    }
}

/// A producer no caller passes does not trace to the library either.
pub async fn unreached(producer: &FutureProducer) {
    let _ = producer
        .send(FutureRecord::to("never.unreached"), Duration::from_secs(0))
        .await;
}
