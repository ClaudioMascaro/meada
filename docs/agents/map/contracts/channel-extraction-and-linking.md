# Channel extraction and linking

`channel.rs` (spec #20):
  `extract` calls the six readers (`channel/{ts,python,rust,go,java,csharp}.rs`), each answering for its own pack only (`code.pack().extensions()`), in that order; the message types and broker helpers live in `channel/message.rs`; a producer is a provider at its call site, a
  consumer a consumer at its handler (an inline handler is named `<broker> <names in order>`,
  such as `kafka orders.created`). `contract(broker, name)` is `channel::<broker>::<name>`, or
  `channel::<broker>` for an unresolved side (`channel::parse`, `Channel::unresolved`, and the seam's `shown`, which
  prints it with `(unresolved)`, and `typed`, which reads it back), which `link` skips: it is in the export and never
  linked, and since its contract is not `None` it is no open item (`search` kind `unresolved`
  and `meada decide` never see it). `link` joins every consumer to every producer that
  `reaching(contract, providers, routes)` gives, in any repository including its own. Name
  matches are `fits(produced, consumed)`: the same contract at `exact`, reason the contract,
  and a consumer whose name is a wildcard pattern to the producers its pattern matches
  (`wildcard(broker, pattern, name)`, the one place a broker's matching rules live: NATS `*` one
  token and `>` the rest, a subject holding a wildcard matching nothing, table-tested), at
  `inferred`, reason `<producer contract> wildcard <pattern>`. RabbitMQ (`amqp`): a binding is a
  `Found` with role `Binding`, stored in `contracts` (`contract` the queue's channel, `via` the
  exchange, `method` the pattern, `site` the `bindQueue` call) and exported as `bindings`
  (absent when empty). A publish to a named exchange holds no channel: it is a `Found` with
  role `Publish` and no contract (`via` the exchange, `method` the routing key, `site` the
  `publish` call), exported as `publishes: [{exchange, key, site}]` (absent when empty), so no
  queue named like its exchange or its key ever equals it; a publish to the empty exchange is
  a plain producer of the queue `key`, and one whose key cannot be evaluated an unresolved
  producer `channel::amqp`. `Routes` holds the workspace's bindings and publishes (the broker
  is shared); `routed(publish, binding)` is the one routing rule: same exchange and the
  pattern `pattern_matches` the key (topic-exchange rules whatever the exchange type), `exact`
  when the pattern is the key, else `inferred`, reason `<queue contract> binding <exchange>
  <pattern>`. `reaching` adds those publishes to a queue's producers, and `Routes::reached`
  gives the queues one publish reaches; of several matches to one producer the strongest wins.
  `ContractKind::link` takes the workspace's exports and builds its `Routes` from them. `declare` adds the
  channel sides of the repository's AsyncAPI documents (`Summary::channels`, in path order): a
  document side whose contract equals a code side's of the same direction merges into it (every
  such code provider or consumer keeps its site and takes the document's shape unless it has
  one), any other is a side without a handler, a provider at `<repository>::<document>::<broker>
  send <name>` or a consumer (`via` null) at `<repository>::<document>::<broker> receive
  <name>`; a side an earlier document by path already added merges the same way, so adds
  nothing. Both carry `shape` (`Consumer::shape` is new, exported when present). `link` is
  unchanged by this: the documented sides are ordinary providers and consumers, so a documented
  producer reaches the code consumers of other repositories at `exact`, and a code producer
  reaches the documented consumer (an `amqp` side of a document is a queue name, never an
  exchange, so it is never a publish). It sorts `providers` and `consumers` into the index's
  order afterwards.
