# Shape and document readers

- `lang/ts/uses.rs`: following call results (`Call::uses`) charges every node it visits and
  every step it copies into a `const`'s uses or past a method call to `USES_BUDGET` (2^20 per
  file), so a long method chain spends it; past it, the
  file's remaining calls have no uses, so their consumers read nothing and give no drift. A
  value is followed only in the function making the call, through the names a `const`
  declaration, a `for...of` variable or an array method's callback parameter binds (patterns
  included), when nothing else in that function binds or assigns the name; a `let`
  declaration, `.then(...)`, a callback given by name, `reduce` and a value passed to a
  function end it (no marker). A destructuring pattern nested deeper than `PATTERN_DEPTH` (32)
  reads nothing below that depth (`ponytail:`; a unit test feeds it a deeper pattern). A wrapper's caller is read only when the
  wrapper returns one thing (the response, the body or one path below it) from the function
  holding its call; whether the body was awaited is not checked (no marker).

- Shape readers (`zod.rs`, `declared.rs`, `literals.rs`): each per-file budget below is one
  `shape::Budget` per extraction (`Http::extract`, `exported_shapes`), shared by every
  reader of that extraction, so a file spends each at most once per extraction.

- `zod.rs`: reading the schemas of a file's handlers charges every schema expression examined
  to `ZOD_BUDGET` (2^14, over all the handlers of the file, and the `z.infer` schemas its
  types name); past it, the remaining handlers
  give no shape from a schema. Nesting, constants followed and methods chained count together
  against `ZOD_DEPTH` (32): a part deeper is open. A schema is read only when every 2xx body a
  handler sends is typed with one; otherwise the literals are read (no marker). A name imported
  from a package is a schema the query fills (issue #175) where a schema stands (the receiver
  of `parse`, a field, an element, a member, a `z.infer`), or under the methods `KEEPS` and
  `SCHEMA_METHODS` list; `.pick`, `.omit`, `.partial` and `.required` on it open the part,
  since its fields are not known here (no marker). `.passthrough`, `.catchall`, `z.record`, `z.any`, `z.unknown`, `.transform`, `z.lazy`
  and any call Meada does not read open the part; so does a `pick` / `omit` with a computed
  key and a `z.union([])`. A schema or a parse result held in a constant of the handler (a
  function-local `const out = Schema.parse(..)`) is followed, unlike in `literals.rs`, and
  nothing records a later write to it (`Object.assign(out, { extra: 1 })`), so it is still
  read as the schema's shape (no marker).

- Declared types (spec #23, `declared.rs`, `lang/ts/types.rs`): a type is read to at most
  `TYPE_DEPTH` (32) levels, both when it is written (`lang/mod.rs`, the nesting of an object or
  array type) and when a consumer's cast, type argument or annotation is followed through
  `extends`, aliases and intersections; a level deeper adds no fields. Reading the types of one
  file (its consumers' and its handlers') charges every type examined to `TYPES_BUDGET` (2^14); past it, the type being
  read adds no fields on a consumer, and opens the value on a provider. A declaration that leads
  back to itself (`interface A extends B`, `interface B extends A`) adds what it did the first
  time. A type parameter hides a declared type of its name only when it sits within
  `HIDING_CLIMB` (64) levels of the use; one declared farther up leaves the name unknown, so
  it adds no fields. Only interfaces (with `extends`), aliases to object types, intersections,
  arrays and `z.infer<typeof S>` give fields; a union, a mapped or conditional type
  add none (no marker); a type from a package is filled at query time (issue #175). A generic of the repository
  (`Paged<Row>`) adds its own fields, its type arguments not substituted (its `T` adds none).
  An optional field (`?`) adds no drift, nor does what lies below it; the fields of a
  `z.infer` schema that `.optional()`, `.nullish()` or `.partial()` make may be absent are
  optional too (`.required()` undoes it), and a schema union adds no fields. A type argument
  given to a wrapper types the body only when the wrapper's return type is its own type
  parameter (`Promise<T>`) or it casts the body to it (`as T`, `json<T>()`); `json<T>()` and
  the like type their result when they have exactly one type argument. A cast or an annotation is read at the top of a value (not
  inside an object literal), and a value that goes through a function, a `let` or a rebinding
  is not followed (no marker). On a provider, an index signature in the type of a returned
  constant, a cast body or the handler's return annotation opens that part of the shape.

- `literals.rs`: reading a handler's returned literals charges every call examined and every
  value read or constant followed, over all the handlers of a file, to `LITERALS_BUDGET` (2^16);
  past it, the file's remaining handlers give their routes no shape. Nested literals and
  constant hops count together against `LITERALS_DEPTH` (32): a part deeper is present and open,
  and a constant chain longer is unknown. Only a body Meada reads (an object or array literal,
  a constant naming one) gives fields; a call, a `map` over rows, a parameter or a spread of
  anything but a known object is open (no marker). The handler is the function `res.json` and
  its kin sit in or in a callback inside it. A response with a literal status outside 2xx is
  left out whatever its form (`res.status(404).json(..)`, `res.status(404).end()`,
  `res.sendStatus(404)`, `new Response(null, { status: 404 })`), and so is Express's error path,
  `next(err)` (`next` called with an argument other than `'route'` or `'router'`). A response
  sent another way opens the root of the shape (no marker): in Next.js, a call whose result the
  handler itself returns and that is no `NextResponse.json` / `Response.json` (`return
  ok(user)`, `new Response(..)` with no such status, `redirect`); in Express, `res` passed to
  another call, a `res` method other than `json`, `send` and the ones that only set a header or
  the status (`end`, `render`, `redirect`, `sendFile`, `sendStatus`, ...) with no such status,
  or `next()`, `next('route')` and `next('router')`. A `res` stored in another name or a status set in a separate
  statement (`res.status(404);` then `res.json(...)`) is not followed, so such a send counts as
  2xx (no marker). Only a module-level constant is followed: nothing records a later write to
  a function-local one (`body.next = cursor`, `Object.assign(body, x)`), so it is unknown and
  open; a module-level object mutated after its declaration is still read as written. A constant
  held by another repository (a package import the repository's `Resolver` does not map to one
  of its files) is filled at query time (issue #175), and so are the index signatures of a
  package type a returned value, a cast or the return annotation names.

- Shapes through package imports (issue #175, `shape.rs`, `package.rs`): `shape::fill` follows
  an import into the shape the package keeps, and the imports of that shape in turn, at most
  `PACKAGE_HOPS` (8) packages deep, and charges every import looked up and every path grafted
  to `GRAFT_BUDGET` (2^16) per side; past either, the import is unresolved: open on a provider,
  nothing on a consumer. So is an import that resolves nowhere, `ambiguous`, to several names,
  or in two providers to different shapes, and a name no reader keeps a shape for (a call, a
  union type). Only a module-level constant or type a file of a package's public surface
  (`surface`) exports itself keeps a shape (a re-export reaches it through the
  `exports` rows): the files the entries of each named manifest (the bare name and each
  `exports` subpath without a `*`, as an import resolves them) lead to through the `exports`
  rows. Those are re-read, their files parsed again, on every refresh that extracts, so the
  cost is bounded by the packages' surface, not the repository. A subpath pattern (`./x/*`) and
  a deep import into a package without `exports` (`pkg/src/x`) reach files off the surface,
  which keep no shape, so such an import is unresolved (no marker). An edited `package.json` or
  `tsconfig.json` extracts again. A type through a namespace import (`C.Receipt`) is not read
  (no marker); a schema through one (`C.Quote`) is. Since constants and types are `exports`
  targets, two `export *` that give a name differently (a function and a constant) export
  neither, as in ES modules.

- `document/`: a document never fails indexing, so its readers have fixed limits; deeper or
  larger parts are unreadable or open. `mod.rs`: `DOCUMENT_BYTES_MAX` (16 MiB, above the largest public
  single-file descriptions, GitHub's REST API and Stripe's SDK spec at 10 to 15 MB): a larger
  document is unreadable at its first line. `events.rs`, the YAML and JSON event reader:
  `MAX_NESTING` (128 nested collections, as `serde_json` has for JSON, an alias's node counted at
  the depth it lands) and the alias budget (`EXPANSION` times the source plus `SLACK`, no
  marker), which also charges every document of a stream and every node an anchor keeps (anchors
  nested around one node keep a copy each), before it is copied; the count of documents per file is
  config values' (`config_values::DOCUMENTS_MAX`). `schema.rs`: `MAX_DEPTH` (128 levels per shape), `WORK` (64 MiB per document) and
  `BUDGET` (10,000 schemas per shape, no marker). `openapi.rs`: a path item reference behind
  another one that is external is dropped without a status line (no marker).
