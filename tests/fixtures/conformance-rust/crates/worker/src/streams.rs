use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::AsyncCommands;

pub async fn append_all() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let _: String = con.xadd("orders", "*", &[("id", "1")]).await?;
    let _: String = con.xadd("shipments", "*", &[("id", "2")]).await?;
    let stream = format!("events.{}", "audit");
    let _: String = con.xadd(stream, "*", &[("id", "3")]).await?;
    let key = std::env::var("STREAM").unwrap();
    let _: String = con.xadd(key, "*", &[("id", "4")]).await?;
    Ok(())
}

/// A group in the options: a consumer of every key.
pub async fn read_group() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let options = StreamReadOptions::default().group("workers", "w1").count(10);
    let _reply: StreamReadReply = con
        .xread_options(&["orders", "shipments"], &[">", ">"], &options)
        .await?;
    Ok(())
}

pub async fn read_inline() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let _reply: StreamReadReply = con
        .xread_options(
            &["events.audit"],
            &[">"],
            &StreamReadOptions::default().group("auditors", "a1"),
        )
        .await?;
    Ok(())
}

pub async fn read_unknown() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let key = std::env::var("STREAM").unwrap();
    let options = StreamReadOptions::default().group("workers", "w1");
    let _reply: StreamReadReply = con.xread_options(&[key], &[">"], &options).await?;
    Ok(())
}

/// A read without a consumer group is not a consumer.
pub async fn read_plain() -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    let options = StreamReadOptions::default().count(10);
    let _reply: StreamReadReply = con.xread_options(&["orders"], &["$"], &options).await?;
    Ok(())
}

pub struct Log;

impl Log {
    pub fn xadd(&self, _key: &str) {}
}

pub fn not_redis(log: &Log) {
    log.xadd("never.local");
}
