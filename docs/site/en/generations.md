# Generations and retention

**Generation pointer and pins** (feature `generation-pointer`, which includes `snapshot`;
`cargo install meada --features generation-pointer`). With the `[generations]` section, which
needs `[snapshot]` and `[valkey]`, the current generation's pointer moves from the store to
Valkey, and the whole fleet holds the same set of generations.


<!-- meada-graph:generation-lifecycle -->

```toml
[valkey]
url = "rediss://meada@cache.example.internal:6379"   # TLS outside loopback
password_file = "/run/secrets/valkey"                 # or password_env
key_prefix = "meada:prod"                             # default "meada"

[generations]
keep = 3                                              # generations retained fleet-wide, at least 2
```

## Pointer and retention

<details markdown="1" class="reference-details">
<summary>Store retention, local trimming and rollback</summary>

`meada snapshot publish` ends by writing, in one script, `{<key_prefix>}:current` (a hash
`{id, fence}`) and `{<key_prefix>}:retained` (a list of `<id>:<fence>`, newest first, each id
once, trimmed to `keep`), and no longer writes the store's `current`; the fence is `0` without
the refresh lock. Each `meada snapshot pull` reads both from Valkey and checks every id (16
lowercase hex characters) and every fence (decimal) before it changes the node; it then
downloads every retained generation the node lacks and deletes the ones that left the list,
never the current one or the one it just replaced. The node's pointer file carries the fence
read with the id. A malformed entry, a missing pointer or an empty list stops the pull with an
error naming the key, and the node stays as it was. The current generation is downloaded and
swapped in first: a retained generation that fails after it becomes a warning on stderr naming
the id and the cause, blocks nothing, and the next pull retries it. Use the same `keep` on the
job and the nodes. `meada status --generations` reads the current generation from Valkey and
checks that the store holds its document; the `store: unreachable (...)` line says whether
Valkey or the store failed.

</details>

## Pin a generation

A workflow run pins the commits it started on in the path: `/<identity>@<generation>`, the
generation being what follows the last `@`, serves that held generation on any node while it
is retained, and `/<identity>` keeps serving the current one. A generation the node does not
hold answers 404 `text/plain`, naming the requested generation, the current one and the held
ones. Every HTTP answer names its generation in the `Meada-Generation` header (a 404 names the
current one) and every tool result in its `_meta`, `{"meada/generation": "<id>"}`: the run reads
the generation of its first answer and pins the next ones to it. Tools, parameters and the text
of the answers do not change. The reverse proxy forwards `Meada-Generation` in answers. A
pinned call still open on a generation that a pull's retention deletes may fail mid-call.
