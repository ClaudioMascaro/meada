# Public smoke workspaces

The three original repository names and origins are required. The app's CASES.md
and the issue specifications are the answer keys, together with the provider and
caller files in each case. Inputs from six pinned corpora are named in inputs.json
and checkouts.json, with the source trees under sources/. An index belongs to one
repository. The ordinary CLI starts from app and discovers its siblings. Workspace
status reads the published manifest and per-repository indexes.

| Corpus | App | Lib | Shared | Answers |
|---|---|---|---|---|
| Original | 280efe9d | a36abd39 | 51703923 | #933/#946 upgrade and #855/#934 controls. Only neutral on #941. With #943 only the warm index stays at neutral. After the #946 fix the three recorded provider links, including sites, levels and reasons, agree with cold. |
| Published #947 | fffac4a | cb746acf | 0aabcee | Both peers serve POST /orders. Application config maps SMOKE_ORDERS_URL to lib. Relative has both candidates. Changing the declaration must change the selected peer. Reordering peers preserves the answers. Removing the lib route drops its links and restoring the file recovers them. |
| Expanded app #3 | 3d5c209 | cb746acf | 0aabcee | Ten HTTP call sites plus three SQS sites. Keep this denominator separate from the published corpus. |
| Python class authority app #4 | ed13044 | cb746acf | 0aabcee | #962. Six site/state keys for the two class clients. Read the actual URLs from the app's recorder. |
| Symlink branches | d4d7605 or ad48b4c | a36abd39 | 51703923 | #932 read/write controls. Use the tracked link and actual target before testing a dangling link or an unavailable peer. |

The Go and Python recording transports run the pinned sources with
SMOKE_ORDERS_URL=https://lib.smoke.example. The ordinary clients send POST to
lib, and the Go deferred result and Python decorator replacement send POST to
shared. No HTTP request leaves the process. #948 and #949 permit leaving the
overridden caller open if the host cannot be established. The consumer still
appears at the source site, with no selected peer. A link to shared keeps the
host, method and route from the recording. Selecting lib is a wrong link.
The HTTP checks join consumers, links and decision items by the full repository, path,
qualified definition (with its signature), and call identity. An unknown URL needs a
matching dynamic-url item. A relative route with two ambiguous links needs an
ambiguous-link item listing both canonical targets. A configured or literal-host link
requires the consumer's matching route and provenance and no item at that site.
`SendShared` and `send_shared` append the literal `/orders` to the return from
`sharedBase` and `shared_base`, respectively. When they are open, both peers'
canonical `POST /orders` candidates must remain and the limit must name the helper.
An empty candidate list is valid where the method and known suffix match no provider.

For #961, MCP search must locate both OpenAPI documents and the app's
source locations. The expanded pin has six stable HTTP clients plus five linked
requests in `orders/evidence/client.go` and the three neutral/key constructor sites.
The two configured JVM clients and the two overridden clients have independent
export keys. On each revision, search includes those four sites when their export
has the route contract. All these callers discard the result or leave it unread,
so context must show the two providers, the exact number of distinct linked
sites and the trace-up guidance. The captured predecessor and corrected product
both omit 16 sites, with two different pairs of consumers in search. The same
workspace must trace the named app job through the exact lib and shared jobs.
Keep the three MCP source keys separate and retain the raw `answers.json`.

For #810, the same spelling of a private method in unrelated Java and Kotlin
classes leaves a unique exact code target in each class. The reachability of
that code hop and the HTTP link are separate checks. Each isolated client,
and each one-helper rename, keeps its own configured request and lib provider.
The separate #952 fixture gives `Shadowing.hidden()` and `Typed.typed()` independent
keys. Both call the literal factory for `https://typed.example.com`. The first
requests `/persons/hidden` and the second requests `/persons/typed`. An absent
shadowed consumer also fits the historical receiver ceiling. The pending #952 decision
is recorded even when a candidate matches an admissible outcome.

`denominator.json` counts the 13 expanded cases independently per executable. The
separate Kotlin observations and raw candidate sets are in `checks.json` and
`transitions.json`. Rejected answers from the base are historical observations. A
rejected candidate fails the driver even when both binaries agree. To replay the
same scorer on mutated and unchanged copies of the published #953 JSON, run
`python3 .claude/skills/prove-meada/scripts/smoke_stack.py --selftest --oracle-archive /path/to/proof.zip`.
The #960 archive exercises the helper/candidate and MCP mutations on the real
13/13 export and its answers. The selftest also drives real Git fetch and cache
checkout of a missing required pin. Missing pins report blocked / 2 with the
origin, revision and Git stderr. Other execution failures and timeouts remain
distinct.

The expanded upgrade starts from the old binary's existing indexes and swaps only
the executable. The first export, explicit index and reopened export compare the
same ten HTTP site observations, SQS provisions and the complete export with the
candidate's cold index.
The neighbor checks record transport and export after replacing the Go write with
an empty deferred literal and after directly, indirectly and conditionally rebinding
the Python helper. Changing an unrelated variable and restoring the undecorated
file preserve both configured sites. These overlays live in disposable copies of
the pinned app, and the changed sources accompany the raw answers.

The class recorder runs `orders/clients/python/class_probe.py` with
`SMOKE_ORDERS_URL=https://lib.smoke.example`. `Changed.send` sends to shared, lib
and shared as `@install` is present, removed and restored. `Ordinary.send` sends
to lib throughout. When the override cannot be folded, the item must name
`self.changed_origin` and list both POST candidates. The `674b7ee` reference
selects lib for both callers in all states. A second untouched checkout is indexed
with that binary and opened by the candidate before an explicit index or reopen.
All three candidate exports must match cold and the SQLite compatibility
fingerprint must change on first export. Read the per-state runtime and export
captures under `commands/class/`, and the two independent counts in
`auxiliary-denominators.json`.

In the SQS probe, publishLiteral and publishOrder write smoke-orders and
provision at exact to lib's queues.tf::aws_sqs_queue.orders. publishSelected
has no selected provisioner. The historical 68f1c0b drops the single-return helper, a regression relative
to the literal control. The current path restores that helper across an
executable upgrade and keeps the runtime choice unresolved.

The three DynamoDB writers in app's orders/mutation/store.go are #891
controls. Current alone names orders. Archive and Choose must not name
orders unconditionally. The separate DynamoDB transaction probe covers #894.

The seven oracle roles checked in status are no runner, no builder, no matcher,
no emitter, no provisioner, no use and no CI consumer. The isolated variant
copies the matching files into the app checkout and removes its peers. The CI
orphan keeps its route within the isolated app repo. In three repositories it
keeps the shared identity. The workspace qualifier must match the scope.

The tracked valid link reads like a regular file with the same bytes. The
malformed target reports a decision parse error on direct reads. Every
saving mode refuses a valid, malformed, dangling or waiting link before
asking anything and keeps the link and its target. In a manifest, absent and
old-schema indexes are diagnosed without writes. Only lib loses symbols
in the corrupted variant. Its name and the SQLite cause must survive status.

The #937 fixture in smoke_lifecycle.py supplies three numbered requests,
each with the same one declared candidate. The loopback endpoint accepts
choice 1 at 0.9. Just before request 3 there are two saved calls, also valid
after interruption. Resumption sends only the last prompt. The continuous
run supplies the complete byte and prompt controls. Declining the third
response or making the directory read-only at the second request retains
the decisions already written. Previews write nothing.

For #945, lib's ci.yml test step 2 installs a path and has no named package
side. Step 3 runs smoke-cli and links at inferred to the entry point
declared by lib's pyproject.toml. The updated CASES.md came through app
PR #3. The exact CI trace across app, lib and shared has two hops.

The routing matrix overlays three additional paths per configuration on
copies of the published corpus. Its five dimensions are the environment
owner, one or two route providers, peer declaration order, a literal query
suffix, and a cold or populated index. The 32 configurations give 96
distinct paths and 192 callers. The configured caller uses the declared
owner. The relative caller has both candidates at ambiguous when the
path is shared, or the sole candidate at inferred. Each row keeps its
site, provider, level, reason and decision candidates.
