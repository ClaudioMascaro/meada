# Channel clients

- `channel/go.rs` (spec #25, #82): a receiver that does not trace back to a library's
  constructor or type makes no side (a type of the repository with a `Publish` or
  `WriteMessages` method is never one). A confluent `Produce` takes the topic from
  `TopicPartition.Topic` of the message literal written in the call or held by one name; a
  message built elsewhere is unresolved. A topic starting with `^` is a pattern and names no
  topic. A Redis read without a consumer group (`XRead`) is no consumer, and `XReadGroup` gives
  the first half of `Streams` as the names (the ids are the second). A sarama `Consume` handler
  is found when its third argument is a literal or a name that holds one; any other leaves the
  consumer on the function that calls `Consume`. A queue name is the last segment of the SQS
  queue URL.
- `channel/java.rs` (spec #26, #92): a client counts only by the type the source writes for its
  receiver: one a call returns in place (`connection.sync().xadd(..)`), an interface the table
  does not list (`KafkaOperations`, `AmqpTemplate`, `RedisStreamCommands`) and a `var` with no
  readable type make no side. A listener annotation on a class (`@KafkaHandler`,
  `@RabbitHandler` methods), a composed annotation, `@KafkaListeners`, `topicPattern` and an
  `id`-only listener read no name (one unresolved consumer when the annotation is on a method).
  A placeholder that resolves to a comma-separated list stays one name. A `RabbitTemplate`
  whose exchange or routing key the code sets (`setExchange`, `setRoutingKey`) is not read:
  only the module's `spring.rabbitmq.template.exchange` replaces the default exchange; the
  one-argument forms and a JMS default destination are unresolved. `BindingBuilder` is read
  only as `bind(q).to(e).with(key)` with the queue and the exchange written as a `new Queue`,
  `new TopicExchange` or `new DirectExchange` (through constants and `final` fields); a `@Bean`
  method's parameter, `QueueBuilder`, `ExchangeBuilder` and a fanout `to(..)` bind nothing, and
  neither does a `@QueueBinding` without a key. An SQS request built in a lambda
  (`sendMessage(b -> b.queueUrl(..))`) or held in a local, and Redis streams held in a map
  variable, are unresolved. The same file reads Kotlin (see Kotlin channels in [map/seams.md](../map/seams.md)), with the
  same limits.
- `channel/csharp.rs` (spec #28, #115): `CHANNEL_CLIENT_DEPTH` (32) bounds how many links a
  receiver is followed through (a local bound to a call on a local bound to another); past it
  the call is no form, and the unit test holds a client behind thousands of locals. A client
  counts by its written type or its construction only: a value a repository method returns, a
  client built by a factory the table does not list, one injected as `object` or an interface of
  the repository, and a local assigned twice are no client. The pack's `name=` bounds (one per
  write, looked up by a map, never searched) are as many as the file's writes. Left out:
  MassTransit; `SubscribeSync`, `QueueSubscribe` and `Request` of NATS.Client, and NATS.Net v2 as
  a whole; `BasicGet` and a consumer class that overrides `HandleBasicDeliver` (the handler is
  the method holding `BasicConsume`); `CreateReceiver` and `CreateSessionProcessor`; a Service
  Bus sender made in another file than the one that sends; a stream read by an array of
  `StreamPosition` (unresolved); a `TopicPartition` as the Kafka target; a topic list built as
  `new List<string> { .. }` (unresolved); `request.QueueUrl = ..` set after the construction; a
  name read from `IConfiguration` or the environment (unresolved on both sides, so appsettings
  are not read); `String.Empty` passed by a wrapper's caller to the exchange of a `BasicPublish`
  (unresolved: only the exchange the form's own call writes is read). A `+=` handler is recorded
  only for a member target (`x.Event += h`), not for `Event += h` on `this`, and for a lambda, an
  anonymous method, a name or a member access.
- `channel/ts.rs`: a kafkajs handler passed through a wrapper's parameter is not followed
  (the function that subscribes handles); only the first `run` on a consumer binding counts. An
  amqplib `bindQueue` whose queue, exchange or pattern is not a literal is dropped.
  A BullMQ `Queue` is named by its own `new Queue(name)` binding: `new Queue(name).add(...)` with
  no binding, a `Queue` that is a class field or a parameter, and `addBulk` are not recognized. A
  node-redis client made as `await createClient().connect()` is not traced (the `connect` chain
  has no binding); a BullMQ processor given as a file path is not followed (the function that
  makes the `Worker` handles).
- `channel/python.rs`: a client is traced to its constructor by `Code::origins` (a name its
  scope binds once, an attribute assigned a call once, a parameter every exact caller passes, a
  value a function returns, or a call written in place: `boto3.client("sqs").send_message(...)`,
  `pika.BlockingConnection(p).channel().basic_publish(...)`), with one level of `channel()` or
  `client()` above it (a pika channel of a connection, a boto3 client of a session); deeper
  factories, a client held in a container and a module imported under another path
  (`kafka.producer.KafkaProducer`) are not recognized. A boto3 client counts for SQS when its
  service is the literal `"sqs"` (a name or constant is not followed). A topic that starts with
  `^` is a regular expression, so it is an unresolved name. A pika `queue_bind` whose queue, exchange or routing key is left out
  or not a literal is dropped. A callback is a handler when it is a definition of the repository
  (`on_message_callback=handler`, `cb=self.on_job`); a lambda or anything else leaves the
  function that subscribes. The streams of an `xreadgroup` are read from a dict literal (or a
  constant folded to one); a dict built otherwise is an unresolved stream.
- `channel/rust.rs`: a client is recognized when its receiver traces back to the library's
  constructor (`Code::origins` with `Follow::Values`, then `Code::outside`, one constructor per `LIBRARIES`
  entry); a client held in a struct field, passed as `&mut con` to a parameter, or rebound by
  assignment is not followed. `lapin` `set_delegate` closures are not followed (the function
  that holds `basic_consume` handles). A Kafka topic starting with `^` is unresolved; an
  `queue_bind` that cannot be fully evaluated is dropped; an SQS `queue_url` on a builder held
  in a `let` is not recognized. `Code::mutable` takes the value of a `let mut` bound once and
  never assigned (`Facts::mutable`, keyed by qualified name); a closure parameter, a `let` in a
  closure given to a call, a nested function's parameter or an assignment in such a closure hides it.
  `CHAIN` (64): a builder chain (`FutureRecord::to(t).payload(p)`, an SQS `send_message()...`,
  redis `StreamReadOptions`) is read to that many calls below the form (`Code::chain`, itself
  bounded by `FLUENT_CHAIN`); a longer one is not a producer or a consumer.
- `channel.rs`: broker clusters are not modelled (one name of one broker kind is one channel
  across the workspace), and an exchange's type is not read: every binding pattern follows
  topic-exchange rules.

- AWS SDKs in Java 2.x, Go v2 and .NET (#660): operation tables live in `channel/aws.rs` and
  `resource/aws.rs`; the mechanical readers and complete constructor evidence live in
  `contract/sdk/`. A type-only injected client, a name-matched factory, an opaque return branch,
  a reassigned local and a hidden constructor yield no side. Existing `EVAL_DEPTH` and
  `EVAL_BUDGET` bound the memoized walk; the SDK lattice test drives a full 1 MiB source file
  in each language. Java builder requests, Go input literals, .NET request initializers,
  lists, table-keyed maps and supported named/string overloads name sides. A named request
  whose name field is written is opaque, with C# writes followed through receiver aliases. Operations not in
  the documented table do not. A hierarchy query (`GetParametersByPath`) has no physical
  parameter name and stays unresolved. RDS and ElastiCache control-plane clients establish
  no data uses. Kotlin keeps the earlier typed SQS reader.
