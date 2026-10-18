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

/// A name an inner scope binds again hides the connection: a closure's parameter, a `let` in a
/// closure given to a call, a nested function's parameter.
pub async fn hidden_names(logs: Vec<Log>) -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    logs.iter().for_each(|con| con.xadd("never.param"));
    logs.iter().for_each(|l| {
        let con = l;
        con.xadd("never.let");
    });
    fn nested(con: &Log) {
        con.xadd("never.nested");
    }
    let _: String = con.xadd("kept", "*", &[("id", "5")]).await?;
    Ok(())
}

/// An assignment in a closure given to a call changes the connection: it is no longer the one
/// the constructor made.
pub async fn reassigned(logs: Vec<Log>, other: redis::aio::MultiplexedConnection) -> redis::RedisResult<()> {
    let client = redis::Client::open("redis://localhost")?;
    let mut con = client.get_multiplexed_async_connection().await?;
    logs.iter().for_each(|_| {
        con = other.clone();
    });
    let _: String = con.xadd("never.assigned", "*", &[("id", "6")]).await?;
    Ok(())
}

pub struct Log;

impl Log {
    pub fn xadd(&self, _key: &str) {}
}

pub fn not_redis(log: &Log) {
    log.xadd("never.local");
}
