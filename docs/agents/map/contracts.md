# Contract kinds

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `contract.rs`: `ContractKind` trait (`prefix`, `extract`, `declare`, `sides`, `link`),
  `KINDS`, `Level`, `Link`, `Names`, `Role`. A `Found` carries its `role` (`Provider`,
  `Consumer`, `Binding`, `Publish` or `Import`),
  stored in `contracts.role`; the site does not imply it (a channel producer is a `Site::Call`, a channel
  consumer a `Site::Def` or `Site::Inline`). A `Site::Def` name is folded through `Names::scope`
  in the index: unnamed inline segments drop, and an empty result is the file's top level. `http.rs`: `fetch` consumers (also a call to a parameter defaulting to
  `fetch`; each with `Found::shape` from `response_reads`, spec #23: over the frames
  `Code::expand_frames` climbed, the field paths read from the `.json()` of the response in the
  function making the `fetch` and in each wrapper up to the site, a wrapper passing on the
  response, the body or a path below it when it returns that one thing from the function its
  caller reaches), Next.js app-router and Express providers (each with a `Found::shape` from
  `http::provider_shape`, the one place that picks a route's code source: `zod::Zod::returned`
  first, else `literals::Literals::shape` (spec #23; both read the 2xx bodies `provider_shape`
  lists once with `Literals::responses`): one `Readers { zod, literals }` per `extract`, holding the budget of
  each handler file; Express reads a handler once for every prefix it is mounted under),
  `normalize_path`, and the link rules (host,
  `base_url_env`, unique provider). `http::drift(links, exports)` sets `Link::drift` on every
  HTTP link but an ambiguous one whose consumer (by site and the link's contract, or by site
  alone for an unresolved one; several of one contract count only when their shapes agree) and
  provider (by site, serving the link's contract) both have a shape, through `shape::drift`. An Express route path or mount prefix with a part that cannot
  be read (a template-literal hole, a parameter, an environment read) adds no route: a router
  mounted only under such a prefix is served under no prefix, never at the root; a literal `:id`
  stays a parameter. `declare` first adds the routes of the repository's
  documents (spec #21): a document route whose contract a code provider already has merges
  into it (one provider, the code's handler site); any other becomes a provider of its own at
  `<repository>::<document path>::<METHOD> <path>` (no handler), and a route an earlier
  document by path listed adds nothing. Each provider of a documented contract, merged or not,
  takes `Provider::shape` from the first document by path that lists it, when that
  document's route has one (else the provider keeps the shape its code gives, if any; none when that
  document's route has no shape). A document provider is a written contract: it lowers
  no level (a host match is `exact`). A route also matches by its gateway paths: `declare`
  fills `Provider::gateways` (`Gateway {path, reason}`, not exported; the `[serve]` form, which
  is also the exported `Provider::gateway`, first, then the forms of each document listing the
  route: one per server path of the document, with `[serve] strip`, or the `[serve]` form when
  the document has no server path; a code route always has its `[serve]` form, a route only
  documents list has it only through a document without server paths), `Provider::hosts` (not
  exported: `[serve] hosts` plus the host of every `servers` URL of the repository's documents,
  checked by `link` for a `host:` consumer). `shape`, `gateways` and `hosts` are set on HTTP
  providers only; `http::gateway_contracts(provider)` yields each as
  `(contract, reason)` and `http::serves(provider, contract)` is the one check of any path (also
  used by open items and decision replay, so a gateway candidate carries the provider's own
  contract). That link is `inferred` with the form's reason (`gateway prefix <p>`, `gateway
  strip <s>` or both), read from the provider, not recomputed from the configuration; within
  one repository a code path match wins over gateway path matches. `resource.rs` (spec
  #19): reads no code; `declare` turns each `[[resource]]` `via` symbol the repository has into a
  provider (writer) or consumer (reader) of `resource::<kind>::<id>`, and `link` joins every
  reader to every writer of another repository at `declared`, reason the contract. A `via` that
  names no symbol is dropped. An entry without `via`, or with none matched, makes the repository
  the side: the `site` is the bare repository identity (no `::`), the **repository-level** side.
  `report` gives the status lines per repository: each unmatched `via`, the fallback to repository
  level, and a resource with no link in the workspace (unlinked). `channel.rs` (spec #20):
  `extract` calls `code.pack().messages(code)`; a producer is a provider at its call site, a
  consumer a consumer at its handler (an inline handler is named `<broker> <names in order>`,
  such as `kafka orders.created`). `contract(broker, name)` is `channel::<broker>::<name>`, or
  `channel::<broker>` for an unresolved side (`channel::parse`, `is_unresolved`, and `shown`, which
  prints it with `(unresolved)`), which `link` skips: it is in the export and never
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
  `ContractKind::link` takes the workspace's `Routes` beside its providers. `declare` adds the
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
- `package.rs` (spec #22): packages as contracts. `read(bytes)` turns a `package.json` into a
  `Summary {name, version, dependencies: [Entry {field, name, range}], exports, types, module,
  main}` (the four dependency fields, sorted by field then name; `exports` as written, `null`
  counting as absent) and never fails: a file over `MAX_BYTES` (1 MiB), not a JSON
  object or nested past `serde_json`'s depth limit reads as empty, a field of another type or an
  empty string counts as absent, and a name that is empty, over `MAX_NAME` (214), or holds a
  control character, whitespace or `:` is no name (a dependency entry with such a name is dropped).
  The kind `Packages` (`package::npm::<name>`, sides `package` and `dependent`) `extract`s an
  `Import` row (`Role::Import`, never exported) for each call whose callee's head names an import
  of a bare specifier from outside the repository (`Code::imported`: specifier in `via`, the
  chain spelled from the imported name in `method`); `declare` makes a provider at `<repository>::<manifest path>` (`Provider::version` for the
  reason) and a `packages` row `{name, version, repository, directory}` (`.` at the root) for each
  named manifest, private ones too. `consume(exports, providers)` is the workspace-wide step `Engine::exports`
  runs after every repository's `declare` (one repository cannot see another's providers; the
  `Providers` index of package names to their manifests is built once there and shared with
  `import`, and `reaches` is the one place the `workspace:` rule lives): for
  each dependency entry whose package some repository of the workspace provides (a `workspace:`
  range needs one in its own repository), it adds a consumer at `<repository>::<manifest
  path>::<field>::<name>` (the range travels in `Consumer::method`, which is `serde(skip)`) and a
  `dependencies` row `{manifest, field, name, range}`. An entry on a package outside the workspace
  appears nowhere. `link` gives one candidate `exact` and several `ambiguous` (one link per
  candidate, `workspace:` keeping the consumer's repository), with the reason `<identifier>
  <range|none> (local <version|none>)`, prefixed `<n> providers fit; ` when ambiguous.
  `reason()` is the one place it is built; it ends with `, range mismatch` or `, range
  unchecked` when `flag(range, version) -> Option<Flag>` (public; `context` calls it too, and
  nobody parses the reason back) says so. `flag` checks with `nodejs-semver` (npm syntax: `^ ~
  x - ||`, prereleases as npm): `Mismatch` when the range does not accept the local version,
  `Unchecked` for a missing range or version, a `workspace:`/`file:`/`link:`/`npm:` range, a git
  or URL source, a tag such as `latest`, a range past `MAX_RANGE`, and any range with a token
  that is not a comparator (the crate parses `foo 1.2.3` as `1.2.3`). The flag is computed at
  query time and stores nothing. `import(exports, providers)` is the second workspace step, after
  `consume`: for each import row whose package the importing file's nearest manifest
  (`nearest`: the deepest directory holding it, named or not) depends on in any field, it
  resolves the call in every provider of that package (a `workspace:` range: its own repository
  only) through `Provided::resolve`: `ways` gives the ways in, each with the files it names and
  its `How` (`exports` through `exported`, the table-tested lookup of spec #22: exact key, then
  the `*` pattern with the longest prefix, the first of `types`, `import`, `default`, `require`
  at each level, `null` blocking, a key or a target with two `*` not read, the only way in when
  present; without `exports`, for the bare name one way for each of `types`, `module`, `main`
  present, tried in that order (an empty or non-string `types`, `module` or `main` counts as
  absent), or, with none of the three, `index.<ext>` for each pack extension, as Node's default
  does (`path`, never `index` alone or `index/index.<ext>`); for any other subpath, the subpath
  itself, as a path with the trunk's extension and `index` rules, `path`). A file must stay
  inside the package directory (`inside`), be a source file the provider indexes, not a
  `.d.ts`, and have this package's manifest as its nearest (`owned_source`: a file of a nested
  package is not the outer package's); the first way that leads to one, directly or through the
  mapping, wins, so a field that leads nowhere does not stop the search. One that is build output
  maps back to source: `mapped` takes the `tsconfig.json` in the package directory; only when that
  file is absent or invalid (stored as `null`, no `Config`) the repository root's, and a valid
  one without a usable `outDir` maps nothing, with no fallback. `roots` turns its `outDir` and `rootDir` (relative to that config;
  `rootDir` defaults to the package directory) into repository directories, and `sources`
  (the pure, table-tested mapping) swaps the entry's `.d.ts`, `.js`, `.mjs`, `.cjs` or `.jsx` for
  each pack extension in `EXTENSIONS` order under `rootDir`; the first the provider indexes wins,
  `inferred` with `how` `outDir`. When no way finds a file, directly or through the mapping, the
  import is `ambiguous` and `how` names the declared field or rule whose entry could not be
  followed to a source file (the first way tried: `exports`, the first of `types`, `module`,
  `main` present, or `path`): `candidates` gives the
  symbol the chain names in every source file whose nearest manifest is this package's (not a
  nested package's, not one under a `node_modules` segment, not a `.d.ts`) and that exports its
  head (sorted, one hop each; none, nothing). The files by owning manifest are grouped once per
  provider (`Provided::owned`), not once per call. `lookup` then reads the chain's
  head from the `exports` rows (the file's own export of the name decides; else its `export *`
  files, never for `default`, two that give it differently making it ambiguous and so not
  exported, as in ES modules; `seen` cuts cycles) and each further name as a member of that
  definition or a name of a whole file. A `tsconfig.json` has a row in `manifests` too, read
  by `read_tsconfig` (`out_dir`, `root_dir`; `Export::tsconfigs`, `serde(skip)`; `extends` is not
  followed, as the trunk follows none), so an edit relinks like a `package.json` one. `import` skips a specifier no repository provides before
  reading anything else, and builds a provider's tables (`Provided`) the first time an import
  reaches it; `index::export` reads the `import` rows only for a repository with a manifest and
  the `exports` rows only for one with a named manifest. A call resolved in one provider at least
  becomes a
  consumer of the package at its call site (range in `Consumer::method`, where it resolves in
  `Consumer::hops`, both `serde(skip)`), and each resolution an `imports` row `{site, specifier,
  name, level, how, target}`. `link` then gives a call with hops, instead of the manifest, the
  symbol in each provider whose package link it would have, at the weaker of the two levels,
  with the reason `<package reason>; import <how>` (`How::name`: `exports`, `types`, `module`,
  `main`, `path` or `outDir`; on an `ambiguous` link, the field or rule whose entry could not be
  followed to a source file). The hop is derived at query time from stored facts and never stored; an import of a
  package outside the workspace, or that resolves nowhere, appears in no export. A re-export of
  another package inside the provider ends the lookup (no second hop). No lockfile is read and
  no registry queried. Shapes cross the same way (issue #175): `surface(manifests, configs,
  exported, files)` is the public surface, the entry files of each named manifest's bare name
  and `exports` subpaths without a `*` (`Provided::entry`, the part of `resolve` that follows
  the ways in) and every file their `exports` rows name, in turn; `exported_shapes(code,
  surface)` (run by `index::refresh`) gives, for the files of the surface, a `Role::Shape` row (`site`
  the file, `method` the name, `via` the `shape::Want`) for each module-level constant or type
  a file exports: `Types::exported` for an interface or alias (`type`), `Zod::exported` for a
  schema (`zod` and `infer`), else `Literals::exported` for an object or array literal
  (`literals`); `index::export` loads them as `Export::shapes` (`ExportedShape`). `shapes(exports,
  providers)`, the third workspace step, fills every provider and consumer shape holding a
  `shape::Import` through `shape::fill`, whose lookup `kept` resolves the import like a call
  (`nearest` manifest of the importing file, its dependency entry, `reaches`, then
  `Provided::resolve` with the chain the import names, `known` being the kept shapes of the
  import's view instead of the symbols) and gives the resolution's level (the weaker of two
  providers keeping the same shape); none, `ambiguous`, several names or two providers keeping
  different shapes is unresolved. A consumer left with no field has no shape.
  `Resolver::exported` names constants and types as targets too, which no call reaches since
  they are no symbols.
