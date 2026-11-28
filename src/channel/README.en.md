# Queues and topics in code

Extraction of queue or topic producers, consumers, publications and bindings from application code.

## Interface

C#, Go, Java, Python, Rust and JS/TS readers turn library calls into messages. AWS operations share a table and destination names pass through common argument readers. `[csharp.rs:messages]` `[go.rs:messages]` `[java.rs:messages]` `[python.rs:messages]` `[rust.rs:messages]` `[ts.rs:messages]` `[aws.rs:OPERATIONS]` `[message.rs:Message]`

In Go and JS/TS, `[[channel_client]]` identifies a repository function or method and specifies the argument or field that holds the Kafka topic. In Go it can also identify a function by its import path. A declaration that produces no side appears in `status`. `[go.rs:declared]` `[go.rs:client_sides]` `[ts.rs:declared_sides]` `[../channel.rs:status]`

## Invariants

Each language requires evidence for the expected library or receiver. The AMQP publication and binding readers keep exchange, routing key and queue distinct. `[python.rs:messages]` `[ts.rs:messages]` `[message.rs:publish]` `[message.rs:bind]`

The KafkaJS reader recognizes clients held in private fields and looks up literal `app-config` values for a direct topic-key read. Multiple values or an incomplete configuration cap confidence at `inferred`. C# receiver lookup stops at 32 steps. `[ts.rs:messages]` `[ts.rs:configured_topic]` `[csharp.rs:CHANNEL_CLIENT_DEPTH]`

## Side Effects

Readers produce messages in memory from extracted facts without opening broker connections. `[message.rs:Read]` `[ts.rs:messages]`

## Verification

From the repository root, run `cargo test -p meada --test it -- channels:: channel_clients:: kafkajs_classes:: kafka_env::`.
