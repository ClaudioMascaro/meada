# Multiple nodes

Use multiple nodes to serve queries in parallel. Each node keeps its own index copy. A proxy distributes requests; the transport needs no sticky sessions. Clients on protocol 2026-07-28 do not create a session at all.

The [Compose example](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/README.en.md) includes two nodes, TLS and token admission at the proxy. The [Helm chart](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/README.en.md) uses **Deployment**, with init, readiness and graceful shutdown. The StatefulSet profile below requires manifest adaptation; the current chart does not generate it.

## StatefulSet with shared S3

This is a **reference profile**, using existing modules. StatefulSet gives replicas stable identities and supports a separate volume for each pod. Meada does not require it: the shipped chart uses Deployment and per-pod `emptyDir`.

Here, a Meada node means a server replica running in a pod. A Kubernetes node is the machine hosting pods. StatefulSet does not guarantee replicas run on different machines; configure anti-affinity or topology spread to distribute them across workers and zones, according to cluster capacity.

<!-- meada-graph:statefulset-s3 -->

The S3 bucket and prefix are shared. Active SQLite files are not. Each replica has its own `local_dir`, with extracted bundles, indexes and the `current` pointer. A per-replica PVC can preserve these files across recreation; an ephemeral volume requires another initial pull.

| Component | Responsibility |
| --- | --- |
| Refresh job | Fetches mirrors, indexes locally and publishes, without overlapping another job |
| S3 or compatible store | Holds immutable documents and bundles under the shared bucket/prefix |
| Valkey | Coordinates generation pointer, retention, lock, fence and heartbeats |
| Each pod's init | Performs the first `snapshot pull` before starting the server |
| Each pod's companion | Repeats pull in that pod's exclusive `local_dir` |
| MCP server | Queries local indexes with `--generations` |
| Proxy / Service | Distributes requests across ready replicas |

Configure the same bucket, prefix and Valkey domain on the job and replicas. Give every node and the console a unique `snapshot.node_name`; stable pod names can serve that purpose. The job keeps its own mirrors, `index_dir` and `local_dir`.

S3 is accessed through its object API, not as a mounted volume. The shipped snapshot backend uses an S3-compatible endpoint, with TLS outside loopback. There is no `file://` backend.

See [snapshots](snapshots.md) for store configuration, [generations](generations.md) for retention and [refresh](refresh.md) for lock and fence. The guarantee that any fetch or indexing failure aborts the entire publication is [planned in #316](https://github.com/ClaudioMascaro/meada/issues/316); job chaining and fence protection are already documented.

## Choose how to distribute indexes

<!-- meada-graph:node-layout -->

**Each node indexes its mirrors.** The node fetches code and runs `meada index --workspace` in a local `index_dir`. This layout needs no cloud modules. Nodes can answer with different commits until they have all updated their mirrors.

**One job publishes; nodes receive.** The job fetches and indexes mirrors. It then publishes a generation, the bundle set containing code and indexes. Each node downloads that generation into its own directory and switches its local pointer. [Snapshots](snapshots.md) explains publication and pull.

A rebuild renames `index.db.new.<pid>` over `index.db` under `index.db.lock`. Readers see the previous index or the complete new one. Replace an index only when no `meada index` is writing it: an active `index.db-journal` could be mistaken for the new file's journal.

## Local storage and reading

Do not share an active `index_dir` between a writer and readers over NFS or EFS. [SQLite on network filesystems has reliability constraints](https://sqlite.org/useovernet.html). Each node needs local storage or a volume only it writes.

The server can read a read-only `index_dir` and mirrors, for example filled by init or included in the image. It creates no files there. Mirrors owned by another user are accepted with `-c safe.directory`, only for manifest paths. This requires git 2.38 or later; on Ubuntu 22.04 and RHEL 8 with older git, configure `safe.directory` at system level.

## Configure proxy and readiness

The proxy preserves `MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name` and every `Mcp-Param-*` header. It may route by them but must not alter them: rmcp rejects disagreements between headers and body (SEP-2243). Preserve the `Meada-Generation` response header too.

Do not add `Origin`: MCP returns 403 for requests carrying it, so a browser page needs a backend to call it. The MCP server does not check `Host`. `--listen` accepts an IP or `localhost`, without resolving names.

Complete init before admitting traffic. For the local layout, use a TCP probe or `meada status --workspace <manifest>`: exit 0 means all repositories can be served; 1 means failure. Status waits up to 50 ms for a commit and counts a locked index as ready. For snapshots, use [status with `--generations`](snapshots.md#readiness).

`--listen` opens the port only after startup checks. The 404 at `/` lists identities and is not a health check.

## Run one writer and stop replicas

Prevent overlapping jobs, for example with `concurrencyPolicy: Forbid` on the CronJob, and use the [refresh lock](refresh.md). Set `terminationGracePeriodSeconds` above the 25 s SIGTERM drain limit. The `preStop` delay must allow the load balancer to remove the replica from traffic.
