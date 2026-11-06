# GCP SDK use sides (#668)

This extends the permanent application proof through `/create-prove-it`; the existing CLI
and MCP surfaces remain. The answer keys are maintainer #668 and parent #642, base ADR
0026's origin/direction rules, and the official SDK samples below. The referenced base ADR
has no concrete SDK operation inventory. The coordinating SDK research supplies this adopted
minimum; say so in the receipt. The amendment and user docs from the implementation are
descriptions, never independent answer keys. No SDK or cloud service is contacted.

| Group | Pub/Sub publish / subscription receive | GCS write / read | Secret Manager read |
| --- | --- | --- | --- |
| JS and TS independently | `@google-cloud/pubsub`, `new PubSub().topic(name).publishMessage` / `.subscription(name).on("message", handler)` | `@google-cloud/storage`, `new Storage().bucket(name).file(key).save` / `.download` | `@google-cloud/secret-manager`, `new SecretManagerServiceClient().accessSecretVersion({name})` |
| Python | `google.cloud.pubsub_v1`, `PublisherClient().publish` / `SubscriberClient().subscribe` | `google.cloud.storage.Client().bucket(name).blob(key).upload_from_string` / `.download_as_bytes` | `google.cloud.secretmanager.SecretManagerServiceClient().access_secret_version(request={"name": name})` |
| Java | `com.google.cloud.pubsub.v1.Publisher.newBuilder(name).build().publish` / `Subscriber.newBuilder(name, handler).build().startAsync` | `StorageOptions.newBuilder().build().getService()`, `create(BlobInfo.newBuilder(BlobId.of(bucket,key)).build(), data)` / `readAllBytes(bucket,key)` | `SecretManagerServiceClient.create().accessSecretVersion(name)` |
| Go | `cloud.google.com/go/pubsub/v2.NewClient`, `.Publisher(name).Publish` / `.Subscriber(name).Receive`; v1 compatibility `.Topic` / `.Subscription` | `cloud.google.com/go/storage.NewClient`, `.Bucket(name).Object(key).NewWriter` / `.NewReader` | `cloud.google.com/go/secretmanager/apiv1.NewClient`, `AccessSecretVersion(ctx, &secretmanagerpb.AccessSecretVersionRequest{Name: name})` |
| C# | `Google.Cloud.PubSub.V1.PublisherClient.CreateAsync(name)`, `PublishAsync` / `SubscriberClient.CreateAsync(name)`, `StartAsync` | `Google.Cloud.Storage.V1.StorageClient.Create`, `UploadObject` / `DownloadObject` and their `Async` counterparts | `Google.Cloud.SecretManager.V1.SecretManagerServiceClient.Create`, `AccessSecretVersion` and `AccessSecretVersionAsync` |

Official references: [publishing](https://docs.cloud.google.com/pubsub/docs/publisher),
[receiving](https://docs.cloud.google.com/pubsub/docs/samples/pubsub-subscriber-async-pull),
[Go v2](https://docs.cloud.google.com/go/docs/reference/cloud.google.com/go/pubsub/v2/latest),
[Go v1](https://docs.cloud.google.com/go/docs/reference/cloud.google.com/go/pubsub/latest),
[storage writes](https://docs.cloud.google.com/storage/docs/uploading-objects-from-memory),
[storage reads](https://docs.cloud.google.com/storage/docs/reading-objects), and
[secret access](https://docs.cloud.google.com/secret-manager/docs/access-secret-version).
Java `TopicName.of`, `SubscriptionName.of`, `SecretVersionName.of`, `BlobId.of`/`BlobInfo`,
C# typed `TopicName`/`SubscriptionName`/`SecretVersionName` and static Pub/Sub name factories,
and Python `topic_path`/`subscription_path` require verified outside origins. A same-spelled
local helper supplies no literal name. No claim covers every overload.

## Drive

After coordinated builds, use the immutable binary whose hash, `.from` identity and commit
are recorded by the receipt. Run from the repository root:

```bash
source /workspace/meada-setup/env.sh
python3 .claude/skills/prove-meada/scripts/gcp-sdk.py \
  --head-bin "$RUN/bin/meada-head" --run "$RUN/gcp-sdk" --http
```

`--run` must be new. `--seed-only --run DIR` writes and hashes the inputs and oracle without
driving a binary. It proves nothing. Sources use the documented minimum in six independent
source languages; JS and TS count as one requested group. The script drives literal and
environment names for all five operation categories, hidden constructors, reassignment and
name-matched-hop controls. Each generated repository also supplies independent Terraform
resources, a filtered subscription, and a shared bucket writer/reader. Inspect complete side,
provision and delivery inventories; environment reads stay unresolved even when tempting
values are present in the process environment. A receive target is always
`channel::pubsub::subscriptions/<id>` and never a guessed topic.

The script saves raw CLI exits/stdout/stderr, complete exports, source hashes, oracle,
queries and stdio/HTTP answers. Check query content against source before comparing every
HTTP answer and tool list byte-for-byte with stdio. A same-size publish-name edit must change
the complete provider inventory; restoring source restores the whole export. Start/stop
uses run-owned loopback servers only. No push, external messages or cloud writes occur.

The independent checker additionally drives exact factory frames with two distinct returned
handle names, a valid origin beside a same-spelled reassigned/shadowed scope in the same
file, typed helpers (including hidden helpers), all documented adjacent async/v1 forms, and
plain/structured/typed Cloud Run environment values. Use the official inventory and newly
authored neutral sources for those additions, never copy implementation fixtures as an
oracle. The base `gcp.py` and `environment.py` flows must preserve #667 and #663 metadata,
filter reasons and workload scope. Run relevant existing AWS/native-broker flows against
the exact base; the shared constructor walk retains the boolean API. Shared resolver/pack
changes also reach the corresponding corpus entries and shared-code base parity. Full
integration proof is coordinated in CI by the parent; a local smoke is explicitly narrower.

## Scope and verdict

The executable minimum does not enumerate every SDK overload, every shadowing syntax or the
adjacent C# async and Go v1 forms. A pass alone cannot claim the whole ticket: the additional
checker probes above are required, with their own saved source and answers. Neither actual
cloud delivery nor SDK runtime execution is tested. There is no Rust GCP inventory here.

The checker uses the permanent receipt template and judges PROVED/BROKEN/BLOCKED/UNPROVEN.
A crash, missing binary, unreachable server or missing answer key is never a pass. Any
unexpected extra side, lost old metadata, missing scope control or new exactness without
constructor evidence is BROKEN. Keep failing captures and rerun the same probe after a fix;
do not relax the oracle to fit the binary. Record this script's same-agent check separately
from the final independent verdict. After inspection remove only this run's generated
`work/` and server index directories; preserve source/oracle hashes and all raw evidence.
