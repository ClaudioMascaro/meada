#!/usr/bin/env python3
"""Source-authored #668 supplementary public-surface controls, no SDK runtime/network.

Expected roles/names below follow ADR0026 and the official inventory. Product fixtures,
implementation tables and previous answers are not used to author these sources.
"""
import argparse
from collections import Counter
import hashlib, json, os, socket, subprocess, sys, time, re, traceback
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from prove import Stdio, Http, answer, git, free_port

SOURCES = {}
CASES = []


def add(group, file, source, rows):
    SOURCES.setdefault(group, {})[file] = source
    for function, role, contracts in rows:
        CASES.append(
            {
                "group": group,
                "file": file,
                "function": function,
                "role": role,
                "contracts": contracts,
            }
        )


PUB = "channel::pubsub"
SEC = "resource::gcpsecretmanager"
GCS = "resource::gcs"


def secret(name):
    return SEC + "::" + name


def topic(name):
    return PUB + "::" + name


js = (
    """import {PubSub} from '@google-cloud/pubsub';
import {SecretManagerServiceClient} from '@google-cloud/secret-manager';
const client = new SecretManagerServiceClient();
function namedHandle({tag}) { return new PubSub().topic(tag); }
export function FrameCedar() { namedHandle({tag:'cedar-frame'}).publishMessage({data}); }
export function FrameReed() { namedHandle({tag:'reed-frame'}).publishMessage({data}); }
export function FrameEnvironment() { namedHandle({tag:process.env.FRAME_TOPIC}).publishMessage({data}); }
export function HiddenPubsub(PubSub) { new PubSub().topic('refused-hidden').publishMessage({data}); }
export function OpaqueReceiver() { let handle=new PubSub().topic('refused-reset'); handle=opaque;handle.publishMessage({data}); }
export function DirectWrite() { const req={name:'projects/p/secrets/direct/versions/latest'};req.name=opaque;client.accessSecretVersion(req); }
export function AliasWrite() { const req={name:'projects/p/secrets/alias/versions/latest'};const copy=req;copy.name=opaque;client.accessSecretVersion(req); }
function alter(req) { req.name=opaque; }
export function HelperWrite() { const req={name:'projects/p/secrets/helper/versions/latest'};alter(req);client.accessSecretVersion(req); }
export function MappingWrite() { const req={name:'projects/p/secrets/mapping/versions/latest'};Object.assign(req,{name:opaque});client.accessSecretVersion(req); }
export function DeleteWrite() { const req={name:'projects/p/secrets/deleted/versions/latest'};delete req.name;client.accessSecretVersion(req); }
export function PriorRead() { const req={name:'projects/p/secrets/prior/versions/latest'};client.accessSecretVersion(req);req.name=opaque; }
export function OtherField() { const req={name:'projects/p/secrets/other-field/versions/latest'};req.unrelated=opaque;client.accessSecretVersion(req); }
export function OtherObject() { const req={name:'projects/p/secrets/other-object/versions/latest'};const other={name:'projects/p/secrets/different/versions/latest'};other.name=opaque;client.accessSecretVersion(req); }
export function MemoSplit() { const first={name:'projects/p/secrets/memo-split/versions/latest'};client.accessSecretVersion(first);const second={name:'projects/p/secrets/memo-split/versions/latest'};second.name=opaque;client.accessSecretVersion(second); }
export function RepeatedRead() { const req={name:'projects/p/secrets/twice/versions/latest'};client.accessSecretVersion(req);client.accessSecretVersion(req); }
export function ExhaustedAlias() { const req={name:'projects/p/secrets/refused-budget/versions/latest'};
"""
    + "".join(
        f"const hop{i}=" + ("req" if i == 0 else f"hop{i-1}") + ";\n"
        for i in range(600)
    )
    + """hop599.name=opaque;client.accessSecretVersion(req); }
"""
)
rows = (
    [
        ("FrameCedar", "providers", [topic("cedar-frame")]),
        ("FrameReed", "providers", [topic("reed-frame")]),
        ("FrameEnvironment", "providers", [PUB]),
        ("HiddenPubsub", "providers", []),
        ("OpaqueReceiver", "providers", []),
    ]
    + [
        (name, "consumers", [SEC])
        for name in [
            "DirectWrite",
            "AliasWrite",
            "HelperWrite",
            "MappingWrite",
            "DeleteWrite",
            "ExhaustedAlias",
        ]
    ]
    + [
        (name, "consumers", [secret(value)])
        for name, value in [
            ("PriorRead", "prior"),
            ("OtherField", "other-field"),
            ("OtherObject", "other-object"),
        ]
    ]
    + [
        ("MemoSplit", "consumers", [secret("memo-split"), SEC]),
        ("RepeatedRead", "consumers", [secret("twice"), secret("twice")]),
    ]
)
for group in ["js", "ts"]:
    add(group, "controls." + group, js, rows)
add(
    "python",
    "controls.py",
    """from google.cloud import secretmanager
from google.cloud import pubsub_v1
client=secretmanager.SecretManagerServiceClient()
# Resource identity arguments, including exact helper effects.
def alter(req):
    req['name']=opaque
def DirectWrite():
    req={'name':'projects/p/secrets/direct-python/versions/latest'}
    req['name']=opaque
    client.access_secret_version(request=req)
def AliasWrite():
    req={'name':'projects/p/secrets/alias-python/versions/latest'}
    other=req
    other['name']=opaque
    client.access_secret_version(request=req)
def HelperWrite():
    req={'name':'projects/p/secrets/helper-python/versions/latest'}
    alter(req)
    client.access_secret_version(request=req)
def MappingWrite():
    req={'name':'projects/p/secrets/mapping-python/versions/latest'}
    req.update({'name':opaque})
    client.access_secret_version(request=req)
def DeleteWrite():
    req={'name':'projects/p/secrets/deleted-python/versions/latest'}
    req.pop('name')
    client.access_secret_version(request=req)
def PriorRead():
    req={'name':'projects/p/secrets/prior-python/versions/latest'}
    client.access_secret_version(request=req)
    req['name']=opaque
def OtherField():
    req={'name':'projects/p/secrets/other-field-python/versions/latest'}
    req['unrelated']=opaque
    client.access_secret_version(request=req)
def OtherObject():
    req={'name':'projects/p/secrets/other-object-python/versions/latest'}
    other={'name':'different'}
    other['name']=opaque
    client.access_secret_version(request=req)
def RepeatedRead():
    req={'name':'projects/p/secrets/twice-python/versions/latest'}
    client.access_secret_version(request=req)
    client.access_secret_version(request=req)
""",
    [
        (name, "consumers", [SEC])
        for name in [
            "DirectWrite",
            "AliasWrite",
            "HelperWrite",
            "MappingWrite",
            "DeleteWrite",
        ]
    ]
    + [
        (name, "consumers", [secret(value + "-python")])
        for name, value in [
            ("PriorRead", "prior"),
            ("OtherField", "other-field"),
            ("OtherObject", "other-object"),
        ]
    ]
    + [("RepeatedRead", "consumers", [secret("twice-python"), secret("twice-python")])],
)
add(
    "csharp",
    "Controls.cs",
    """using System.Threading.Tasks;
using Google.Cloud.PubSub.V1;
using Google.Cloud.Storage.V1;
using Google.Cloud.SecretManager.V1;
class AsyncControls {
 static async Task<PublisherClient> Make(string name) { return await PublisherClient.CreateAsync(name); }
 static async Task<PublisherClient> TypedClient(TopicName name) { return await PublisherClient.CreateAsync(name); }
 async Task TypedFactory() { var pub=await TypedClient(TopicName.FromProjectTopic("proof","cedar-typed-memo"));await pub.PublishAsync(data); }
 async Task WrongTypedFactory() { var pub=await TypedClient(SubscriptionName.FromProjectSubscription("proof","cedar-typed-memo"));await pub.PublishAsync(data); }
 async Task DirectAwait() { var pub=await PublisherClient.CreateAsync("cedar-direct");await pub.PublishAsync(data); }
 async Task ReturnedAwait() { var pub=await Make("cedar-returned");await pub.PublishAsync(data); }
 async Task LateAwait() { var pending=Make("cedar-late");var pub=await pending;await pub.PublishAsync(data); }
 void Unawaited() { var pub=Make("refused-unawaited");pub.PublishAsync(data); }
 async Task DoubleAwait() { var pub=await await Make("refused-double");await pub.PublishAsync(data); }
 async Task TypedTopic() { var pub=await PublisherClient.CreateAsync(TopicName.FromProjectTopic("proof","cedar-typed"));await pub.PublishAsync(data); }
 async Task WrongTypedKind() { var pub=await PublisherClient.CreateAsync(SubscriptionName.FromProjectSubscription("proof","refused-kind"));await pub.PublishAsync(data); }
 async Task HiddenTyped(object TopicName) { var pub=await PublisherClient.CreateAsync(TopicName.FromProjectTopic("proof","refused-helper"));await pub.PublishAsync(data); }
 void TypedOpaque(PublisherClient pub) { pub.PublishAsync(data); }
 async Task WriteAsyncStorage() { var store=StorageClient.Create();await store.UploadObjectAsync("cedar-async-bucket",opaque,data); }
 async Task ReadAsyncStorage() { var store=StorageClient.Create();await store.DownloadObjectAsync("cedar-async-bucket",opaque,stream); }
 async Task ReadAsyncSecret() { var secrets=SecretManagerServiceClient.Create();await secrets.AccessSecretVersionAsync("projects/proof/secrets/cedar-async-secret/versions/latest"); }
 void RepeatedRead() { var store=StorageClient.Create();store.DownloadObject("cedar-bucket",opaque,stream);store.DownloadObject("cedar-bucket",opaque,stream); }
}
""",
    [
        (name, "providers", [topic(value)])
        for name, value in [
            ("DirectAwait", "cedar-direct"),
            ("ReturnedAwait", "cedar-returned"),
            ("LateAwait", "cedar-late"),
            ("TypedTopic", "cedar-typed"),
        ]
    ]
    + [(name, "providers", []) for name in ["Unawaited", "DoubleAwait", "TypedOpaque"]]
    + [(name, "providers", [PUB]) for name in ["WrongTypedKind", "HiddenTyped"]]
    + [
        ("RepeatedRead", "consumers", [GCS + "::cedar-bucket", GCS + "::cedar-bucket"]),
        ("WriteAsyncStorage", "providers", [GCS + "::cedar-async-bucket"]),
        ("ReadAsyncStorage", "consumers", [GCS + "::cedar-async-bucket"]),
        ("ReadAsyncSecret", "consumers", [SEC + "::cedar-async-secret"]),
    ],
)
add(
    "go",
    "controls.go",
    """package supplementary
import (
 pubsub "cloud.google.com/go/pubsub"
 storage "cloud.google.com/go/storage"
)
func namedTopic(name string) *pubsub.Topic { c, _ := pubsub.NewClient(ctx,"p");return c.Topic(name) }
func FrameCedar() { namedTopic("cedar-go").Publish(ctx,msg) }
func FrameReed() { namedTopic("reed-go").Publish(ctx,msg) }
func V1Receive() { c, _ := pubsub.NewClient(ctx,"p");c.Subscription("reed-sub").Receive(ctx,handler) }
func RepeatedRead() { c, _ := storage.NewClient(ctx);c.Bucket("cedar-go-bucket").Object(opaque).NewReader(ctx);c.Bucket("cedar-go-bucket").Object(opaque).NewReader(ctx) }
func OpaqueReceiver() { c, _ := pubsub.NewClient(ctx,"p");t:=c.Topic("refused-reset");t=opaque;t.Publish(ctx,msg) }
""",
    [
        ("FrameCedar", "providers", [topic("cedar-go")]),
        ("FrameReed", "providers", [topic("reed-go")]),
        ("V1Receive", "consumers", [topic("subscriptions/reed-sub")]),
        (
            "RepeatedRead",
            "consumers",
            [GCS + "::cedar-go-bucket", GCS + "::cedar-go-bucket"],
        ),
        ("OpaqueReceiver", "providers", []),
    ],
)
add(
    "java",
    "Controls.java",
    """import com.google.cloud.pubsub.v1.Publisher;
import com.google.pubsub.v1.TopicName;
import com.google.pubsub.v1.SubscriptionName;
import com.google.cloud.storage.Storage;
import com.google.cloud.storage.StorageOptions;
class Controls {
 static Publisher named(String name) { return Publisher.newBuilder(name).build(); }
 static Publisher typedClient(TopicName name) { return Publisher.newBuilder(name).build(); }
 void TypedFactory() { typedClient(TopicName.of("proof","cedar-java-memo")).publish(message); }
 void WrongTypedFactory() { typedClient(SubscriptionName.of("proof","cedar-java-memo")).publish(message); }
 void FrameCedar() { named("cedar-java").publish(message); }
 void FrameReed() { named("reed-java").publish(message); }
 void TypedTopic() { Publisher.newBuilder(TopicName.of("proof","cedar-typed-java")).build().publish(message); }
 void WrongTypedKind() { Publisher.newBuilder(SubscriptionName.of("proof","refused-kind-java")).build().publish(message); }
 void TypedOpaque(Publisher pub) { pub.publish(message); }
 void HiddenTyped(Object TopicName) { Publisher.newBuilder(TopicName.of("proof","refused-hidden-java")).build().publish(message); }
 void RepeatedRead() { Storage store=StorageOptions.newBuilder().build().getService();store.readAllBytes("cedar-java-bucket",opaque);store.readAllBytes("cedar-java-bucket",opaque); }
}
""",
    [
        ("FrameCedar", "providers", [topic("cedar-java")]),
        ("FrameReed", "providers", [topic("reed-java")]),
        ("TypedTopic", "providers", [topic("cedar-typed-java")]),
        ("WrongTypedKind", "providers", [PUB]),
        ("HiddenTyped", "providers", [PUB]),
        ("TypedOpaque", "providers", []),
        (
            "RepeatedRead",
            "consumers",
            [GCS + "::cedar-java-bucket", GCS + "::cedar-java-bucket"],
        ),
    ],
)

# Same-valued but incompatible typed actuals must not collide in factory memoization.
CASES.extend(
    [
        {
            "group": "csharp",
            "file": "Controls.cs",
            "function": "TypedFactory",
            "role": "providers",
            "contracts": [topic("cedar-typed-memo")],
        },
        {
            "group": "csharp",
            "file": "Controls.cs",
            "function": "WrongTypedFactory",
            "role": "providers",
            "contracts": [PUB],
        },
        {
            "group": "java",
            "file": "Controls.java",
            "function": "TypedFactory",
            "role": "providers",
            "contracts": [topic("cedar-java-memo")],
        },
        {
            "group": "java",
            "file": "Controls.java",
            "function": "WrongTypedFactory",
            "role": "providers",
            "contracts": [PUB],
        },
    ]
)

# Exhausted evidence has a separate repository: its shared work ceiling can
# conservatively invalidate names throughout that input. Ordinary controls stay bounded.
for _group in ("js", "ts"):
    _file = "controls." + _group
    _source = SOURCES[_group][_file]
    _start = _source.index("export function ExhaustedAlias()")
    SOURCES[_group][_file] = _source[:_start]
    SOURCES[_group + "-hostile"] = {
        "exhausted."
        + _group: _source[: _source.index("function namedHandle")]
        + _source[_start:]
    }
    for _case in CASES:
        if _case["group"] == _group and _case["function"] == "ExhaustedAlias":
            _case["file"] = "exhausted." + _group
            _case["group"] = _group + "-hostile"


def http_copy(binary, root, group, run, queries, tools, texts):
    manifest = run / (group + ".workspace.toml")
    manifest.write_text(
        'index_dir = "'
        + group
        + '-indexes"\n[[repository]]\nrepo = "example.com/gcp-supplement/'
        + group
        + '"\npath = '
        + json.dumps(str(root))
        + "\n"
    )
    indexed = subprocess.run(
        [binary, "index", "--workspace", str(manifest)],
        cwd=run,
        capture_output=True,
        text=True,
        timeout=180,
    )
    (run / (group + ".index.out")).write_text(indexed.stdout)
    (run / (group + ".index.err")).write_text(indexed.stderr)
    indexed.check_returncode()
    port = free_port()
    with (run / (group + ".http.out")).open("w") as out, (
        run / (group + ".http.err")
    ).open("w") as err:
        server = subprocess.Popen(
            [
                binary,
                "mcp",
                "--listen",
                f"127.0.0.1:{port}",
                "--workspace",
                str(manifest),
            ],
            cwd=run,
            stdout=out,
            stderr=err,
        )
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError("HTTP server exited during readiness")
                try:
                    socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
                    break
                except OSError:
                    time.sleep(0.1)
            client = Http(f"http://127.0.0.1:{port}/example.com/gcp-supplement/{group}")
            actual_tools = client.call("tools/list", {})["result"]["tools"]
            responses = [
                client.call(
                    "tools/call", {"name": q["tool"], "arguments": q["arguments"]}
                )
                for q in queries
            ]
            actual = [answer(r) for r in responses]
            (run / (group + ".http.json")).write_text(
                json.dumps(
                    {
                        "queries": queries,
                        "responses": responses,
                        "answers": actual,
                        "tools": actual_tools,
                    },
                    indent=2,
                )
                + "\n"
            )
            assert (
                actual == texts and actual_tools == tools
            ), "Whole HTTP/stdio answers or schemas differ"
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)
            (run / (group + ".http.stopped")).write_text(str(server.returncode) + "\n")


def seed_cloud(run):
    folder = run / "cloud-run"
    folder.mkdir()
    app = folder / "app"
    infra = folder / "infra"
    for root, name, peer in [(app, "app", "infra"), (infra, "infra", "app")]:
        root.mkdir()
        (root / "meada.toml").write_text(
            '[[peer]]\nrepo = "example.com/gcp-supplement/cloud-'
            + peer
            + '"\npath = "../'
            + peer
            + '"\n'
        )
        git("init", "-q", cwd=root)
        git(
            "remote",
            "add",
            "origin",
            "https://example.com/gcp-supplement/cloud-" + name + ".git",
            cwd=root,
        )
    workflow = app / ".github/workflows/publish.yml"
    workflow.parent.mkdir(parents=True)
    workflow.write_text(
        "jobs:\n  build:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: ghcr.io/cedar-proof/gcp-worker:v1\n"
    )
    (app / "worker.js").write_text(
        """import {PubSub} from '@google-cloud/pubsub';
import {Storage} from '@google-cloud/storage';
const bus=new PubSub();const storage=new Storage();
export function Send() {bus.topic(process.env.SIGNAL).publishMessage({data});}
export function Read() {storage.bucket(process.env.OBJECTS).file(opaque).download();}
function OnMessage(message) {}
export function Listen() {bus.subscription(process.env.TARGET).on("message",OnMessage);}
export function SideOnly() {storage.bucket(process.env.ONLY_SIDE).file(opaque).download();}
"""
    )
    definitions = """resource "google_pubsub_topic" "cedar" { name = "cedar-topic" }
resource "google_pubsub_subscription" "cedar" {
 name = "cedar-sub"
 topic = google_pubsub_topic.cedar.id
}
resource "google_storage_bucket" "cedar" { name = "cedar-bucket" }
resource "google_pubsub_topic" "wrong" { name = "refused-kind" }
resource "google_pubsub_subscription" "wrong" {
 name = "refused-sub"
 topic = google_pubsub_topic.wrong.id
}
"""
    phases = {
        "plain": ['"cedar-topic"', '"cedar-bucket"', '"cedar-sub"'],
        "structured": [
            '"projects/proof/topics/cedar-topic"',
            '"gs://cedar-bucket/opaque"',
            '"projects/proof/subscriptions/cedar-sub"',
        ],
        "typed": [
            "google_pubsub_topic.cedar.id",
            "google_storage_bucket.cedar.name",
            "google_pubsub_subscription.cedar.id",
        ],
        "wrong-kind": [
            "google_pubsub_subscription.wrong.id",
            "google_pubsub_topic.wrong.id",
            "google_pubsub_topic.wrong.id",
        ],
    }
    sources = {}
    for phase, values in phases.items():
        env = "\n".join(
            '   env {\n     name = "' + name + '"\n     value = ' + value + "\n   }"
            for name, value in zip(["SIGNAL", "OBJECTS", "TARGET"], values)
        )
        sources[phase] = (
            definitions
            + """resource "google_cloud_run_v2_service" "worker" {
 name = "cedar-worker"
 template {
  containers {
   name = "main"
   image = "ghcr.io/cedar-proof/gcp-worker:v1"
"""
            + env
            + """
  }
  containers {
   name = "side"
   image = "ghcr.io/cedar-proof/unrelated:v1"
   env {
     name = "ONLY_SIDE"
     value = "refused-cross-container"
   }
  }
 }
}
"""
        )
    (infra / "main.tf").write_text(sources["plain"])
    for root in (app, infra):
        git("add", ".", cwd=root)
        git("commit", "-qm", "independent Cloud Run SDK environment input", cwd=root)
    (folder / "source-key.json").write_text(
        json.dumps(
            {
                "fixed_source_hashes": {
                    str(p.relative_to(folder)): hashlib.sha256(
                        p.read_bytes()
                    ).hexdigest()
                    for root in (app, infra)
                    for p in root.rglob("*")
                    if p.is_file() and ".git" not in p.parts
                },
                "phases": sources,
                "source_hashes": {
                    name: hashlib.sha256(text.encode()).hexdigest()
                    for name, text in sources.items()
                },
                "expected_positive": {
                    "providers": [
                        "image::ghcr.io/cedar-proof/gcp-worker",
                        "channel::pubsub::cedar-topic",
                    ],
                    "consumers": [
                        "resource::gcs::cedar-bucket",
                        "channel::pubsub::subscriptions/cedar-sub",
                        "resource::gcs",
                    ],
                    "provisions": 3,
                    "level": "inferred",
                },
                "expected_wrong_kind": {
                    "providers": [
                        "image::ghcr.io/cedar-proof/gcp-worker",
                        "channel::pubsub",
                    ],
                    "consumers": ["resource::gcs", "channel::pubsub", "resource::gcs"],
                    "provisions": 0,
                },
                "answer_key": "#663/#668, ADR0005/0026: workload-local env; Pub/Sub operation subtype; typed-resource compatibility. Side-only env never reaches the main image.",
            },
            indent=2,
        )
        + "\n"
    )
    return folder, app, infra, sources


def cloud_run(binary, cloud, run, http):
    folder, app, infra, sources = cloud
    original = (infra / "main.tf").read_bytes()
    stamp = (infra / "main.tf").stat()
    exports = {}
    queries = [
        {"tool": "context", "arguments": {"symbol": name, "budget": 200000}}
        for name in ["Send", "Read", "OnMessage", "SideOnly"]
    ]
    client = Stdio(binary, app, folder / "mcp.err")
    try:
        for phase, source in sources.items():
            (infra / "main.tf").write_text(source)
            responses = [
                client.call(
                    "tools/call", {"name": q["tool"], "arguments": q["arguments"]}
                )
                for q in queries
            ]
            texts = [answer(r) for r in responses]
            tools = client.call("tools/list", {})["result"]["tools"]
            (folder / (phase + ".mcp.json")).write_text(
                json.dumps(
                    {
                        "queries": queries,
                        "responses": responses,
                        "answers": texts,
                        "tools": tools,
                    },
                    indent=2,
                )
                + "\n"
            )
            result = subprocess.run(
                [binary, "export"], cwd=app, capture_output=True, text=True, timeout=180
            )
            (folder / (phase + ".export.json")).write_text(result.stdout)
            (folder / (phase + ".export.err")).write_text(result.stderr)
            result.check_returncode()
            data = json.loads(result.stdout)
            exports[phase] = data
            wrong = phase == "wrong-kind"
            expected_providers = Counter(
                [
                    "image::ghcr.io/cedar-proof/gcp-worker",
                    PUB if wrong else topic("cedar-topic"),
                ]
            )
            expected_consumers = Counter(
                [
                    GCS if wrong else GCS + "::cedar-bucket",
                    PUB if wrong else topic("subscriptions/cedar-sub"),
                    GCS,
                ]
            )
            assert (
                Counter(row["contract"] for row in data["providers"])
                == expected_providers
            ), (phase, "providers", data["providers"])
            assert (
                Counter(row["contract"] for row in data["consumers"])
                == expected_consumers
            ), (phase, "consumers", data["consumers"])
            provisions = data.get("provisions", [])
            assert len(provisions) == (0 if wrong else 3), (
                phase,
                "provisions",
                provisions,
            )
            for provision in provisions:
                assert (
                    provision["level"] == "inferred"
                    and provision["root"] == "."
                    and "google_cloud_run_v2_service.worker" in provision["reason"]
                ), provision
            for side in data["providers"] + data["consumers"]:
                if side["contract"].startswith("image::"):
                    continue
                via = side.get("via") or ""
                if "SideOnly" in side["site"]:
                    assert side["contract"] == GCS and via == "env:ONLY_SIDE", side
                elif not wrong:
                    assert (
                        "workload:" in via
                        and "google_cloud_run_v2_service.worker" in via
                        and any(name in via for name in ("SIGNAL", "OBJECTS", "TARGET"))
                    ), side
            assert "refused-cross-container" not in json.dumps(
                data
            ) and "must-not-read-runtime" not in json.dumps(data)
            if http:
                # Peer-aware server scope is exactly the same app+infra pair as local stdio.
                manifest = folder / (phase + ".workspace.toml")
                manifest.write_text(
                    'index_dir = "'
                    + phase
                    + '-indexes"\n[[repository]]\nrepo = "example.com/gcp-supplement/cloud-app"\npath = '
                    + json.dumps(str(app))
                    + '\n[[repository]]\nrepo = "example.com/gcp-supplement/cloud-infra"\npath = '
                    + json.dumps(str(infra))
                    + "\n"
                )
                indexed = subprocess.run(
                    [binary, "index", "--workspace", str(manifest)],
                    cwd=folder,
                    capture_output=True,
                    text=True,
                    timeout=180,
                )
                (folder / (phase + ".index.out")).write_text(indexed.stdout)
                (folder / (phase + ".index.err")).write_text(indexed.stderr)
                indexed.check_returncode()
                port = free_port()
                with (folder / (phase + ".http.out")).open("w") as out, (
                    folder / (phase + ".http.err")
                ).open("w") as err:
                    server = subprocess.Popen(
                        [
                            binary,
                            "mcp",
                            "--listen",
                            f"127.0.0.1:{port}",
                            "--workspace",
                            str(manifest),
                        ],
                        cwd=folder,
                        stdout=out,
                        stderr=err,
                    )
                    try:
                        for _ in range(100):
                            if server.poll() is not None:
                                raise RuntimeError(
                                    "Cloud Run HTTP server exited during readiness"
                                )
                            try:
                                socket.create_connection(
                                    ("127.0.0.1", port), timeout=0.2
                                ).close()
                                break
                            except OSError:
                                time.sleep(0.1)
                        http_client = Http(
                            f"http://127.0.0.1:{port}/example.com/gcp-supplement/cloud-app"
                        )
                        actual_tools = http_client.call("tools/list", {})["result"][
                            "tools"
                        ]
                        actual_responses = [
                            http_client.call(
                                "tools/call",
                                {"name": q["tool"], "arguments": q["arguments"]},
                            )
                            for q in queries
                        ]
                        (folder / (phase + ".http.json")).write_text(
                            json.dumps(
                                {
                                    "queries": queries,
                                    "responses": actual_responses,
                                    "tools": actual_tools,
                                },
                                indent=2,
                            )
                            + "\n"
                        )
                        assert [
                            answer(r) for r in actual_responses
                        ] == texts and actual_tools == tools, (
                            "Cloud Run full transport parity"
                        )
                    finally:
                        server.terminate()
                        try:
                            server.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            server.kill()
                            server.wait(timeout=10)
                        (folder / (phase + ".http.stopped")).write_text(
                            str(server.returncode) + "\n"
                        )
    finally:
        client.close()
        (infra / "main.tf").write_bytes(original)
        os.utime(infra / "main.tf", ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
        assert (infra / "main.tf").read_bytes() == original
    restored = subprocess.run(
        [binary, "export"], cwd=app, capture_output=True, text=True, timeout=180
    )
    (folder / "restored.export.json").write_text(restored.stdout)
    (folder / "restored.export.err").write_text(restored.stderr)
    restored.check_returncode()
    assert (
        json.loads(restored.stdout) == exports["plain"]
    ), "Cloud Run complete export restoration"
    (folder / "restoration.json").write_text(
        json.dumps(
            {
                "source_sha256": hashlib.sha256(original).hexdigest(),
                "mtime_ns_restored": (infra / "main.tf").stat().st_mtime_ns
                == stamp.st_mtime_ns,
                "whole_export_restored": True,
            },
            indent=2,
        )
        + "\n"
    )


def link_key(group):
    if group != "csharp":
        return []
    return [
        {
            "consumer": "example.com/gcp-supplement/csharp::Controls.cs::AsyncControls.ReadAsyncStorage()::store.DownloadObjectAsync#0",
            "provider": "example.com/gcp-supplement/csharp::Controls.cs::AsyncControls.WriteAsyncStorage()::store.UploadObjectAsync#0",
            "contract": GCS + "::cedar-async-bucket",
            "level": "exact",
            "reason": GCS + "::cedar-async-bucket",
        }
    ]


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--head-bin")
    p.add_argument("--run", required=True)
    p.add_argument("--groups", default=",".join(SOURCES))
    p.add_argument("--seed-only", action="store_true")
    p.add_argument("--http", action="store_true")
    a = p.parse_args()
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    cloud = seed_cloud(run)
    roots = {}
    inputs = {}
    for group, files in SOURCES.items():
        root = run / "work" / group
        root.mkdir(parents=True)
        roots[group] = root
        for file, src in files.items():
            (root / file).write_text(src)
            inputs[group + "/" + file] = hashlib.sha256(src.encode()).hexdigest()
        git("init", "-q", cwd=root)
        git(
            "remote",
            "add",
            "origin",
            "https://example.com/gcp-supplement/" + group + ".git",
            cwd=root,
        )
        git("add", ".", cwd=root)
        git("commit", "-qm", "source-authored GCP supplementary controls", cwd=root)
    for case in CASES:
        symbol = case["function"]
        if case["group"] == "csharp":
            parameter = {"TypedOpaque": "PublisherClient", "HiddenTyped": "object"}.get(
                symbol, ""
            )
            symbol = "AsyncControls." + symbol + "(" + parameter + ")"
        elif case["group"] == "java":
            parameter = {"TypedOpaque": "Publisher", "HiddenTyped": "Object"}.get(
                symbol, ""
            )
            symbol = "Controls." + symbol + "(" + parameter + ")"
        case["public_symbol"] = (
            "example.com/gcp-supplement/"
            + case["group"]
            + "::"
            + case["file"]
            + "::"
            + symbol
        )
    (run / "source-key.json").write_text(
        json.dumps(
            {
                "source_hashes": inputs,
                "cases": CASES,
                "links": {group: link_key(group) for group in SOURCES},
                "answer_key": "ADR0026/#668; official SDK operation and identity forms; no candidate result supplies the expected roles/names",
                "note": "Double-await and incompatible typed-call controls are deliberately invalid SDK uses; Meada parses syntax without compiling or executing it.",
            },
            indent=2,
        )
        + "\n"
    )
    if a.seed_only:
        return
    os.environ.update(
        FRAME_TOPIC="must-not-read-runtime",
        SIGNAL="must-not-read-runtime",
        OBJECTS="must-not-read-runtime",
        TARGET="must-not-read-runtime",
    )
    binary = str(Path(a.head_bin).resolve())
    failures = []
    checks = []
    for group in a.groups.split(","):
        root = roots[group]
        r = subprocess.run(
            [binary, "export"], cwd=root, capture_output=True, text=True, timeout=180
        )
        (run / (group + ".export.json")).write_text(r.stdout)
        (run / (group + ".export.err")).write_text(r.stderr)
        assert r.returncode == 0, r.stderr
        data = json.loads(r.stdout)
        cases = [c for c in CASES if c["group"] == group]
        for role in ("providers", "consumers"):
            expected = Counter(
                value for c in cases if c["role"] == role for value in c["contracts"]
            )
            actual = Counter(row["contract"] for row in data[role])
            checks.append((group, role, "whole inventory"))
            if expected != actual:
                failures.append(
                    {
                        "group": group,
                        "check": "whole " + role,
                        "expected": dict(expected),
                        "actual": dict(actual),
                        "rows": data[role],
                    }
                )
        for row in data["providers"] + data["consumers"]:
            assert set(row) <= set(("contract", "site", "via")) and row[
                "site"
            ].startswith("example.com/gcp-supplement/" + group + "::"), row
            assert row.get("via") == (
                "env:FRAME_TOPIC" if "::FrameEnvironment::" in row["site"] else None
            ), row
        assert data["links"] == link_key(group), (
            group,
            "whole source-declared links",
            data["links"],
        )
        for field in ("uses", "provisions", "subscriptions", "executions", "iac_nodes"):
            assert not data.get(field), (group, field, data.get(field))
        for case in cases:
            # Qualified symbols retain the source function name in every pack. Match the
            # function segment, never an arbitrary substring in a resource name.
            selected = [
                row
                for row in data[case["role"]]
                if "::" + case["file"] + "::" in row["site"]
                and any(
                    re.fullmatch(
                        r"(?:.*\.)?" + re.escape(case["function"]) + r"(?:\([^:]*\))?",
                        seg,
                    )
                    for seg in row["site"].split("::")[2:]
                )
            ]
            expected = Counter(case["contracts"])
            actual = Counter(row["contract"] for row in selected)
            checks.append((group, case["function"], "whole operation sites"))
            if expected != actual or len({r["site"] for r in selected}) != sum(
                expected.values()
            ):
                failures.append(
                    {
                        "group": group,
                        "check": case["function"],
                        "expected": dict(expected),
                        "actual": selected,
                    }
                )
        queries = [
            {
                "tool": "context",
                "arguments": {"symbol": case["public_symbol"], "budget": 200000},
            }
            for case in cases
        ]
        client = Stdio(binary, root, run / (group + ".mcp.err"))
        try:
            responses = [
                client.call(
                    "tools/call", {"name": q["tool"], "arguments": q["arguments"]}
                )
                for q in queries
            ]
            tools = client.call("tools/list", {})["result"]["tools"]
            texts = [answer(r) for r in responses]
            (run / (group + ".mcp.json")).write_text(
                json.dumps(
                    {
                        "queries": queries,
                        "responses": responses,
                        "answers": texts,
                        "tools": tools,
                    },
                    indent=2,
                )
                + "\n"
            )
        finally:
            client.close()
        assert all(
            "error" not in response and not response.get("result", {}).get("isError")
            for response in responses
        ), (group, responses)
        assert all(not text.startswith("no symbol matches") for text in texts), (
            group,
            texts,
        )
        if a.http:
            http_copy(binary, root, group, run, queries, tools, texts)
    try:
        cloud_run(binary, cloud, run, a.http)
        checks.append(
            (
                "cloud-run",
                "plain/structured/typed/wrong-kind",
                "whole inventories, scope and restoration",
            )
        )
    except Exception:
        failures.append(
            {
                "group": "cloud-run",
                "check": "live source-keyed environment flow",
                "error": traceback.format_exc(),
            }
        )
    result = {
        "script_result": "BROKEN" if failures else "CHECKS_PASSED",
        "checker_mode": "inline-source-authoring",
        "groups": a.groups.split(","),
        "checks": len(checks),
        "failures": failures,
        "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
        "not_claimed": "Fresh, blind or independent judgment; cloud delivery and SDK runtime execution.",
    }
    (run / "checks.json").write_text(json.dumps(result, indent=2) + "\n")
    print(result["script_result"], len(checks), "checks", len(failures), "differences")
    raise SystemExit(bool(failures))


if __name__ == "__main__":
    main()
