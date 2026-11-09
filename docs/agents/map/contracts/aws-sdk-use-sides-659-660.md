# AWS SDK use sides (#659, #660)

- `channel/aws.rs` and `resource/aws.rs` (#660) own the AWS SDK operation tables for Java 2.x,
  Go v2 and .NET. `contract/sdk.rs` and `contract/sdk/{java,go,csharp}.rs` share request syntax,
  field selection, name normalization and site construction; `contract/sdk/origins.rs` accepts
  constructed clients through complete exact factory/caller evidence, memoized with cycle
  detection under the existing evaluation depth and per-file work budgets. An opaque alternative
  supplies no constructor evidence, nor does a declared type outside a service's
  `Service::declared` list (SQS's earlier injected clients, read by `Origins::declared`).
  `Code::constant_value` preserves a final/readonly field's constructor scope. The SDK readers
  replace Java, Go and C#'s earlier typed SQS forms, keeping their declared client types; Kotlin
  retains its existing form. Java returns, Go maps and C# collection initializers are pack
  syntax, with no service table in a pack. Producer `via` retains an unresolved environment
  variable in the export, as consumer `via` already does. `contracts.method` records a written
  SDK operation for both directions; `ContractKind::use_level` keeps SDK uses exact and
  declarations declared when linking each other or a provisioner. Unresolved resource sides
  never link. Readers and channel handlers belong to their enclosing symbols; writers remain
  operation calls. Java request folding shares Spring injection with channel forms. Runtime
  environment aliases keep verified provenance. C# selector writes are indexed lazily once
  across the group to invalidate named request objects even through another receiver alias; Go
  shares the existing field-write index. Both check a named value by its field name across the
  group: one `entry.EventBusName = ..` anywhere makes every named entry opaque, and a list holding
  one opaque entry names nothing for its inline entries either. A Java builder held in a local or
  a `final` field that a call outside the request's call sets again (a step setting a member of
  the name's path, before or after the request, anywhere in a chain off the builder or off a
  local holding it: `b.message(..).topicArn(..)`, `var e = b.message(..); e.topicArn(..)`,
  `var o = b`) or takes as an argument, itself or such a chain short of `build()`
  (`contract/sdk/java.rs` `rebuilt`; a `final` field is matched by name in every class of its
  file) names nothing, as does a request a step of which takes such a builder (`entries(e.build())`), in
  SQS's channel form and Azure's builder stages too, where a changed topic receiver keeps an
  unresolved side. The folded Java name reader rejects a value-bound head
  before considering on-demand type candidates. `request` returns `None` for a field that a
  request it sees whole (`complete`: a Go or C# literal, a Java chain from `builder()`) leaves
  out: EventBridge then names `default` and SNS no side (ADR 0026); other kinds and any request
  it cannot see whole keep an unresolved side.



`channel/aws.rs` and `resource/aws.rs` own their operation matrices and argument projections;
`channel/{ts,python}.rs` and `resource/{ts,python}.rs` apply each language's origin policy.
`names/ts.rs` shares the constant-construction spelling; `names/python.rs` shares external
constructors and one named factory level, using the pack's enclosing-call order to find an inline
receiver without a file scan. These helpers own no SDK operation table. Name reads use
`Code::expand`, so wrapper arguments follow exact callers only; TS commands, SQS's channel commands included,
use `Code::expand_written`, where a field of the input assigned or `Object.assign`ed after the
object is made, before the call, is unknown, and a field read off a local object
(`{Bucket: cfg.bucket}`) names it unless that field is written before the call. An SQS command
held in a variable reads its input where it is made: a write between
`new SendMessageCommand(input)` and `send(cmd)` is not seen (an S3 or SNS command sees it). TS SDK commands require both
a constant client from the corresponding SDK constructor and a command from that package.
Python requires a matching literal boto3 service, including a Session's client.
`names/python/arguments.rs` checks every return alternative with memoized constructor and
service evidence. Exact call frames bind parameters (omitted arguments include defaults), and
once-assigned class fields retain their constructor. Bare returns, implicit fallthrough,
unawaited coroutines, generators, opaque alternatives and conflicting services yield no SDK
side. Equivalent direct and Session constructions share their literal service evidence.
Python message clients use this same complete reader, so their origin walk cannot exhaust
another SDK reader's shared expansion budget. Factory parents are explicitly named by the
kind; calling a module does not construct a Session.

Java 2.x, Go v2 and .NET use the second kind-owned matrix in each AWS module. Their shared
constructor/request mechanics remain in `contract/sdk/`, with complete memoized origins; a type
annotation alone supplies no client, except SQS's declared client types. Their recorded
operation is `contracts.method`, while JavaScript/TypeScript/Python store `sdk:aws` in
`contracts.via`.

The operation list and its argument names are in the user guide's AWS SDK section. A batch
retains each named table, parameter or secret; an omitted EventBridge bus is `default`.
Supported ARNs use the existing IaC type table's normalization. Malformed or wrong-service
structured identifiers remain unresolved. Only literal named sides join provisioners.
An SDK side stores `sdk:aws` in `contracts.via`, or `env:<variable>` when unresolved through
an environment read; providers export optional `via` just as consumers do. No process
variable is read. Resource `use_level(via, operation)` recognizes either SDK evidence spelling, keeps
declarations at `declared` and SDK literals at `exact`; provisioning asks that seam before weakening a level for patterns or overrides.
SDK resource readers link to SDK writers in their repository as well as peers. Existing
configuration declarations keep their cross-repository rule and their level.

SSM hierarchy queries stay unresolved instead of naming the path as a parameter; DynamoDB
transaction condition checks are readers. Computed object keys are syntax facts (`[]` holding
the key and value), folded by `Code::expand`; batch readers retain literal and environment keys.
