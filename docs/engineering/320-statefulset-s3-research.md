# Plain technical prose and StatefulSet with shared S3

Research for #320, checked on 2026-10-23 against the deployment/runtime sources at
`e8d315c1285352644e37ba4a745de268008e18e3` and the primary sources linked below. The user
corrected the storage requirement to S3. This note describes the requested topology and
the current Helm gap; it adds no chart, runtime or ADR changes.

## Editorial reference

Peter Yang's
[no-ai-slop skill](https://github.com/petergyang/no-ai-slop/blob/main/skills/no-ai-slop/SKILL.md)
and its [evaluation checklist](https://github.com/petergyang/no-ai-slop/blob/main/skills/no-ai-slop/eval.md)
address editing and detecting unclear prose. Their relevant rules preserve meaning and
concrete facts, make the minimum effective edit, use direct verbs, explain jargon and split
tangled sentences. They also remove filler, repeated conclusions and rhetorical contrasts.
Installation is unnecessary for reading or applying these rules. The user requested this
editorial pass for reader prose in Portuguese, English and Spanish.

Explain these terms before the first deployment diagram:

| Term | Plain definition |
| --- | --- |
| Refresh job | A separate process that fetches repository mirrors, indexes them and publishes the result. |
| Snapshot store | The shared S3-compatible bucket and prefix that hold published generations. |
| Meada node | One serving pod that answers MCP requests from its own downloaded files. A Kubernetes worker node is the machine that runs pods. |
| Generation | One published set of repository mirrors and SQLite indexes, each at its recorded commit. |

The refresh sequence and generation behavior come from
[ADR 0012](../adr/0012-cloud-modules-for-server-mode.md). The snapshot store and generation
names follow the [glossary](../../GLOSSARY.md). Any editorial revision must preserve
commands, configuration keys, errors and qualifications in all three documentation languages.

## Requested topology

All serving pods and the refresh job use the same snapshot endpoint, region, bucket and
prefix. The refresh job writes published bundles and generation documents there. Each
serving pod downloads them into its own `local_dir` and serves its own SQLite files.
The job also has separate mirrors and writable `index_dir`. These roles and the immutable
publication format already match
[ADR 0012](../adr/0012-cloud-modules-for-server-mode.md),
[`snapshot::Store`](../../src/snapshot/store.rs) and
[`snapshot::pull`](../../src/snapshot/pull.rs).

With `[generations]`, Valkey stores the current generation and retained list for the
workspace. Every pod polls that state and keeps its own copies. With `[refresh_lock]`,
`meada snapshot lock` runs the whole refresh under one Valkey lock and gives its child
a monotonically increasing fence. Publishing checks that fence before moving the pointer,
so a stale refresh cannot replace a newer result. Sources:
[`snapshot::generations`](../../src/snapshot/generations.rs) and
[`snapshot::lock`](../../src/snapshot/lock.rs).

The following is a reference topology. StatefulSet pod names and per-pod volumes show the
requested layout, not resources the current chart renders. The worker-machine placement
also requires scheduling configuration.

The production diagram is the versioned `statefulset-s3` graph in
[`graphs.json`](../site/assets/graphs.json), rendered as a React Flow island and static SVG.
It connects one refresh job to shared S3 and Valkey, then to two replicas with private
local generation directories. A Service routes admitted queries to those replicas.

A generation is published as one bundle per repository plus a generation document. Publish
uploads the required objects before changing the current pointer. Pull verifies sizes and
hashes, extracts into a temporary local directory, renames the finished generation into
place and replaces that pod's pointer. A request already serving a generation keeps it
through completion. Sources:
[`snapshot`](../../src/snapshot.rs), [`pull`](../../src/snapshot/pull.rs) and
[`serve`](../../src/snapshot/serve.rs).

All pods converge by polling; a publication does not switch every pod at the same instant.
A workflow that needs several calls to use the same generation pins its id in
`/<identity>@<generation>`. Serving and readiness depend on the generation already held
locally, so an unavailable store or Valkey does not invalidate an otherwise servable local
generation. Source:
[ADR 0012, Generations](../adr/0012-cloud-modules-for-server-mode.md#generations).

## S3 behavior and one active writer

Amazon S3 stores objects in buckets, and a key identifies an object within its bucket.
S3 provides strong read-after-write consistency and atomic updates to a single key.
It does not provide atomic updates across several keys or an application refresh lock.
Source:
[Amazon S3 data consistency model](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Welcome.html#ConsistencyModel).
Meada's publish order, immutable documents and Valkey refresh lock supply the workspace
coordination described above. Other S3-compatible services must satisfy Meada's store
contract; AWS's consistency guarantees alone do not verify another vendor's implementation.

Keep one active refresh writer for a workspace, with all refresh entry points using the
same Valkey prefix and lock. The chart's `concurrencyPolicy: Forbid` prevents overlap
scheduled by that CronJob. The refresh lock also coordinates refreshes launched through
other entry points that use it. Sources:
[refresh CronJob](../../deploy/helm/meada/templates/cronjob.yaml),
[`snapshot::lock`](../../src/snapshot/lock.rs) and
[Kubernetes CronJobs](https://kubernetes.io/docs/concepts/workloads/controllers/cron-jobs/#concurrency-policy).

## Current Helm behavior and StatefulSet gap

The chart currently creates a `Deployment`, with two replicas by default. Each pod receives
its own `emptyDir` mounted at `/data`. With the snapshot module, its init container runs
`snapshot pull`, and its companion container runs `snapshot pull --every`. The refresh
CronJob has its own storage. The inspected chart has no StatefulSet or
`volumeClaimTemplates` option. Sources:
[Deployment](../../deploy/helm/meada/templates/deployment.yaml),
[volume helpers](../../deploy/helm/meada/templates/_helpers.tpl),
[CronJob](../../deploy/helm/meada/templates/cronjob.yaml) and
[values](../../deploy/helm/meada/values.yaml).

`values-generations.yaml` enables the cloud image, snapshot store, Valkey, generation
pointer and refresh lock. The operator supplies the real endpoint, region, bucket, prefix
and credentials. `_modules.tpl` writes those values into each bootstrap manifest and
receives that process's `local_dir` separately. Sources:
[generation profile](../../deploy/helm/meada/values-generations.yaml),
[module template](../../deploy/helm/meada/templates/_modules.tpl) and
[Helm guide](../../deploy/helm/meada/README.en.md).

A StatefulSet preserves each pod's identity across replacement. Its
`volumeClaimTemplates` create a separate claim for each pod; `serviceName` names its
governing headless Service. Persistent per-pod storage can preserve downloaded generations
when a pod is replaced. An `emptyDir` instead requires a new initial pull after replacement.
The runtime generation flow can use either lifecycle because each serving pod owns its
directory. Sources:
[Kubernetes StatefulSets](https://kubernetes.io/docs/concepts/workloads/controllers/statefulset/),
[Kubernetes emptyDir](https://kubernetes.io/docs/concepts/storage/volumes/#emptydir) and
[`snapshot::pull`](../../src/snapshot/pull.rs). The conclusion about the runtime flow is an
inference from these contracts and the current per-pod pull implementation.

StatefulSet identity alone does not put each pod on a different worker machine. Node or
zone placement needs suitable affinity or topology spread rules and enough matching
workers. Kubernetes documents `kubernetes.io/hostname` for spreading by worker and
`topology.kubernetes.io/zone` for spreading by zone. Source:
[Pod topology spread constraints](https://kubernetes.io/docs/concepts/scheduling-eviction/topology-spread-constraints/).

An executable StatefulSet guide must identify the actual manifest or chart implementation
that provides the headless Service, matching selectors, `serviceName`, storage lifecycle,
initial/periodic pull commands and placement rules. Documenting this reference topology
does not imply that current Helm values produce it. No new runtime backend or ADR is
needed merely to keep S3 publication and private per-pod SQLite storage; a chart change
would still need its own implementation and render validation.

Keep SQLite access on each process's local storage. ADR 0012 rejects live databases shared
by a writer and serving pods over a network filesystem, following SQLite's synchronization
and locking guidance. Sources:
[ADR 0012](../adr/0012-cloud-modules-for-server-mode.md) and
[SQLite over a network](https://www.sqlite.org/useovernet.html).

## Evidence limits

This was source and documentation research. No software was installed, no cluster or S3
bucket was provisioned and no Kubernetes workload was tested. S3 publication, Valkey
coordination and private local generation directories are verified implementation behavior.
The StatefulSet topology remains a documented reference until an actual manifest is supplied
and validated.
