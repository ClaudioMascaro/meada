# Provider documents and shapes

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `document/` (spec #21): provider documents. `mod.rs`: `is_document(path)` (discovery by file
  name: `{openapi,asyncapi}.{yaml,yml,json}` and `*.{openapi,asyncapi}.{yaml,yml,json}`),
  `read(path, bytes) ->
  Summary` (never fails: JSON by extension through `serde_json`, else YAML; what cannot be read
  becomes a `Problem`, with the line clamped to the file's last line; the content decides,
  since a listed document can have any name: a top-level `openapi` field goes to `openapi.rs`,
  an `asyncapi` one to `asyncapi.rs`, and with neither the name only picks which format it is
  reported as not being, `asyncapi.rs` for an `asyncapi` name, `openapi.rs` for any other),
  `Summary {format: Option<Format>,
  version, title, routes: Vec<Route {path, method, shape}>, servers: Vec<Server {host, path}>,
  channels: Vec<Side {broker, name, role, shape}>, problems: Vec<Problem>}` (stored as JSON; `Route::contract`, `Route::label` = `GET
  /users/{}`; `servers` are the root `servers[].url` entries with variables replaced by their
  defaults in one pass over the URL (a default naming a variable stays as written), a URL
  still holding one dropped, a relative URL giving only its path, host lower
  case, path normalized with no trailing `/`; `Side::contract`, `Side::label` = `kafka send
  orders.created`, `Role::{Producer, Consumer}`; sorted by broker kind, name, role),
  `Problem::Unreadable {line, error}`,
  `Problem::Format {found}`, `Problem::AsyncFormat {found}`, `Problem::Protocol {protocol}`,
  `Problem::Parameters {channel}`, `Problem::NoServers`, `Problem::NoProtocol {server}`,
  `Problem::UnknownServer {channel, server}`, `Problem::ExternalRef {reference}` and
  `Problem::ExternalPathItem {reference}` (structured, rendered through `Msg`), `Format::{Openapi,
  Asyncapi}` (stored as `openapi` and `asyncapi`; `Msg::DocumentRead` takes it with the count),
  `Summary::shown()` (what `search` prints of a document read), `Summary::count(format)`
  (routes for OpenAPI, distinct channels of the sides for AsyncAPI) and
  `report(repo, export)`: the status lines, per document in path order (`Msg::DocumentRead`
  for one read, each problem of it, then each route a later document by path lists again:
  `Msg::DuplicateRoute`), then `Msg::DocumentMissing` for each `[serve] documents` path with
  no row. `openapi.rs`: `read(path,
  value)`: a top-level `openapi` of `3.0` or `3.1` (any patch; a YAML number counts), else
  `Problem::Format` naming the `openapi` or `swagger` field it found; every `paths` key starting
  with `/` and every method of `get put post delete options head patch trace` is a route (a
  path item that is a local `$ref`, such as `#/components/pathItems/...`, is read where it
  points; an external one is a `Problem::ExternalPathItem` and its routes are not read), its
  templates (`{id}`) written as `{}` before `http::normalize_path`, sorted and deduplicated (of
  two templates of one route, the first key gives the shape). A route's `shape` is the
  `application/json` schema of its lowest explicit 2xx response that has one, else of `2XX`
  (a response may be a local `$ref`; `application/*+json` is not read); every external `$ref`
  met, once, sorted, is a `Problem::ExternalRef`. `schema.rs`: `read(doc, schema, source) ->
  (Shape, external refs)`, the schema-to-shape reader: `properties` give fields, `items` the
  `[]` elements (an array with no `items` has open elements), `allOf`, `oneOf` and `anyOf` add
  their members' fields (an `allOf` member with no structure keyword adds nothing; anywhere
  else such a schema, or `true`, is open); `additionalProperties` `true` or a schema opens the
  object, and an object type that lists no fields, no members and no `$ref` is open unless
  `additionalProperties` is `false`. Local `$ref`s (JSON pointers into the document) resolve; a
  reference already being read (a cycle), a missing target and an external reference leave
  that part open. Reading one shape visits at most 10,000 schemas (`BUDGET`) and goes at most
  `MAX_DEPTH` (128) levels deep, a property, element, member or reference being one level; all
  the shapes of one document share one `schema::Work` (`WORK`, 64 MiB: 32 per schema visited
  plus the bytes of every field path listed), which the reader of each document creates and
  passes to every `read`. Past any of them, every part not read is open, so a document's shapes
  have bounded memory and the recursion bounded depth. To read several schemas as one shape
  (AsyncAPI messages), read `{"oneOf": [...]}` of them. `asyncapi.rs`: `read(path, value)`: a top-level `asyncapi` of `2.x`
  or `3.0` (any patch), else `Problem::AsyncFormat` naming the `asyncapi`, `openapi` or `swagger`
  field it found. 2.x: the key of a channel is its name, `subscribe` is a producer and `publish`
  a consumer (the application's side, as the specification reads them), the payloads those of
  `message` (or each member of its `oneOf`), a message a local `$ref`; the channel's `servers`
  are server names. 3.0: the name is the channel's `address` (a channel with none is skipped),
  `action: send` is a producer and `receive` a consumer, the payloads those of the operation's
  `messages` when it lists them, else every message of its channel; the channel's `servers` are
  `$ref`s to `#/servers/<name>`. A channel uses the servers it lists, else every server of the
  document; the lower-cased `protocol` gives the broker kind (`kafka`, `kafka-secure` ->
  `kafka`; `amqp`, `amqps` -> `amqp`; `nats`; `redis`; `sqs`), any other is a `Problem::Protocol`
  once and its channels are ignored. A document with channels and no root `servers` is a
  `Problem::NoServers` (every channel ignored), a server a channel uses without `protocol` a
  `Problem::NoProtocol`, and a server name a channel lists that the document lacks a
  `Problem::UnknownServer` (the channel is ignored on it). A name holding `{` or `}` is a `Problem::Parameters` and
  ignored (AMQP bindings are not read). Sides with the same broker kind, name and role (several
  operations, several servers of one kind) are one, their payloads read together as one
  `oneOf` shape. `yaml.rs`: `parse(text) -> Result<Value,
  (line, reason)>` over `yaml-rust2`'s event parser, taken one event at a time with
  `Parser::next_token` (never `YamlLoader`, nor `Parser::load`, which recurses per level of
  nesting), the collections being read on an explicit stack, at most `MAX_NESTING` (128, as
  `serde_json`) deep (deeper, the document is unreadable at that line): first document only, keys as written, plain scalars typed by the YAML 1.2 core schema,
  quoted and block scalars strings, a non-scalar key an error, and every node built, aliases
  included, charged to a budget of 4 times the source length plus 64 KiB (over it, the
  document is unreadable). Readers only see `serde_json::Value`.
- `shape.rs` (spec #21, ADR 0009): `Shape {source, fields, open, scalars, declared}`, the one form of what a
  contract carries, whatever it is read from, so shape drift (spec #23) compares shapes without
  caring about their source. `fields` and `open` are sorted sets of field paths: dotted, `[]`
  for array elements (`items[].id`), `[]` first for a payload that is an array (`[].id`); every
  node below the root is a field (`items`, `items[]`, `items[].id`); the empty path is the root
  (`shape::shown` prints it `(root)`). An open path is a part whose fields Meada cannot list:
  anything below it may be present. Presence only: no types, formats, `required` or
  `nullable`; `scalars` are the paths known to hold a string, number or boolean (nothing below
  them), and `declared`, on a consumer, the paths a declared type gives rather than a field read
  (empty until spec #23's declared types). `Shape::merge` adds another shape's paths (one side
  reached by several Founds). `schema::read` marks a part scalar when its `type` is only
  `string`, `number`, `integer` or `boolean` (with `null` or not) and no other structure keyword
  is there, and drops the mark where another member gives the part fields or leaves it open. `Source::Document(path)` (exported `{"document": "openapi.yaml"}`) and
  `Source::Literals(path)` (`{"literals": "src/orders.ts"}`, the object literals a route
  handler returns, in the file holding the handler; spec #23) and
  `Source::Reads(path)` (`{"reads": "src/a.ts"}`, the field reads of an HTTP consumer, in the
  file of its call site; spec #23) and `Source::Zod(path)` (`{"zod": "src/orders.ts"}`, the
  zod schema a route handler's response passes through, in the handler's file; declared, like
  a document) are the sources yet; the declared types of a consumer
  (`declared.rs`, spec #23) fill `Shape::declared` and `Shape::optional` of a `Reads` shape. `zod.rs` (spec #23) reads the schema into a `Shape`:
  `Zod::returned(code, responses, handler)` takes the 2xx bodies `Literals::responses` lists
  and reads each as a schema, from `schema.parse(x)`, `schema.safeParse(x).data`, or a value
  or handler return type annotated `z.infer<typeof schema>` (`lang/ts/types.rs` names the
  schema; `Def::returns` and `Value::Schema` carry it), following local and same-repository
  imported constants through `Code::constant`; when any body is no schema, it gives `None`
  and the literals are read (all or nothing). A schema becomes a `Node` tree: `z.object` and
  `.extend/.merge/.pick/.omit` give fields, `z.array` and `.array()` an element `[]`, unions
  (`z.union`, `discriminatedUnion`, `.or`) the union of the members' fields, the primitives
  scalar leaves, `.optional/.nullable/.nullish` and the like keep the part; `.passthrough`,
  `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`, `z.lazy` and anything unreadable
  are open. `literals.rs` (spec #23) reads a handler's returned literals into a `Shape`:
  `Literals::responses(code, path, handler, Handler::Express | Next)` takes the calls in the
  handler's byte range (and its inline callbacks), keeps the ones that send a response (Express:
  `<res>.json|send`, `res` being the handler's second parameter, also after `<res>.status(n)`
  in a chain; Next: `NextResponse.json` imported from `next/server` and the global
  `Response.json`, status from the options' `status`), drops a literal status outside 2xx, and
  unions the bodies. It follows constants one at a time through `Code::constant` (imports
  included), not through `Code::value`, so `LITERALS_DEPTH` bounds nesting and constant hops
  alike and `LITERALS_BUDGET` the work of a file; a body it cannot read is open at that path
  (an empty array and a spread of anything but a known object included), a string with more
  than one part, a number and a boolean are scalar leaves. `Literals::responses` lists the 2xx
  bodies (with the type each is cast to and the handler's return type, whose index signatures
  open the part they type, as do those of a typed constant it follows; and whether the handler answers another way, which opens the root; see the Known
  ceilings), `Literals::shape` reads them; only module-level constants are followed. `Source::at`
  names it in `context`; `Source::declared` tells a declared source (a document) from one read
  from code. `drift(provider, consumer) -> Option<Drift {missing, unchecked}>` is the
  one drift comparison: each consumer path is checked from the root one step at a time
  (`prefixes`), the first step below an open path of the provider is not checked
  (`unchecked`, mapped to that open path), the first step the provider's fields and open paths
  lack is `missing` (so only the shallowest missing path of a branch is listed), nothing past
  a provider scalar is checked (`name.length`; a scalar root checks nothing), and
  `<path>.length` below a provider array (`<path>[]` present) is no field. Each missing path
  maps to its level: `exact` when the provider's source is declared and the path is in the
  consumer's `declared`, else `inferred`; `None` when nothing is missing or unchecked.
