#!/usr/bin/env python3
"""Independent neutral SDK probe, #660/#642/base ADR0025/26.

Only public CLI/MCP transport mechanics are reused. No feature code or test data is read.
Exit 0 = checks passed (checker judges); 1 = BROKEN; 2 = BLOCKED.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import traceback
from prove import Http, Stdio, answer, free_port, git, mcp_answers

BASE = "eae2a567ed250d0ac4a8b8e6f57b62f7a3041c62"
REMOTE = "example.com/zz-prove-aws-sdk/"
# SDK operation and argument directions are AWS request semantics. The coordinating brief
# supplies the inventory because issue #660's referenced ADR has no operation table.
# service, request operation, argument, side, argument shape
OPS = [
    ("sqs", "SendMessage", "QueueUrl", "providers", "one"),
    ("sqs", "SendMessageBatch", "QueueUrl", "providers", "one"),
    ("sqs", "ReceiveMessage", "QueueUrl", "consumers", "one"),
    ("sns", "Publish", "TopicArn", "providers", "one"),
    ("sns", "PublishBatch", "TopicArn", "providers", "one"),
    ("eventbridge", "PutEvents", "EventBusName", "providers", "entries"),
    ("kinesis", "PutRecord", "StreamName", "providers", "one"),
    ("kinesis", "PutRecords", "StreamName", "providers", "one"),
    *[("s3", op, "Bucket", role, "one") for op, role in [
        ("PutObject", "providers"), ("GetObject", "consumers"),
        ("DeleteObject", "providers"), ("HeadObject", "consumers"),
        ("ListObjects", "consumers"), ("ListObjectsV2", "consumers")]],
    *[("dynamodb", op, "TableName", role, shape) for op, role, shape in [
        ("PutItem", "providers", "one"), ("UpdateItem", "providers", "one"),
        ("DeleteItem", "providers", "one"), ("BatchWriteItem", "providers", "write-map"),
        ("GetItem", "consumers", "one"), ("Query", "consumers", "one"),
        ("Scan", "consumers", "one"), ("BatchGetItem", "consumers", "read-map")]],
    *[("ssm", op, field, role, shape) for op, field, role, shape in [
        ("GetParameter", "Name", "consumers", "one"),
        ("GetParameters", "Names", "consumers", "many"),
        ("GetParametersByPath", "Path", "consumers", "one"),
        ("PutParameter", "Name", "providers", "one"),
        ("DeleteParameter", "Name", "providers", "one"),
        ("DeleteParameters", "Names", "providers", "many")]],
    *[("secretsmanager", op, field, role, shape) for op, field, role, shape in [
        ("GetSecretValue", "SecretId", "consumers", "one"),
        ("BatchGetSecretValue", "SecretIdList", "consumers", "many"),
        ("PutSecretValue", "SecretId", "providers", "one"),
        ("UpdateSecret", "SecretId", "providers", "one"),
        ("CreateSecret", "Name", "providers", "one"),
        ("DeleteSecret", "SecretId", "providers", "one")]],
]
CLIENTS = {
    "sqs": ("SqsClient", "Amazon.SQS", "AmazonSQSClient"),
    "sns": ("SnsClient", "Amazon.SimpleNotificationService", "AmazonSimpleNotificationServiceClient"),
    "eventbridge": ("EventBridgeClient", "Amazon.EventBridge", "AmazonEventBridgeClient"),
    "kinesis": ("KinesisClient", "Amazon.Kinesis", "AmazonKinesisClient"),
    "s3": ("S3Client", "Amazon.S3", "AmazonS3Client"),
    "dynamodb": ("DynamoDbClient", "Amazon.DynamoDBv2", "AmazonDynamoDBClient"),
    "ssm": ("SsmClient", "Amazon.SimpleSystemsManagement", "AmazonSimpleSystemsManagementClient"),
    "secretsmanager": ("SecretsManagerClient", "Amazon.SecretsManager", "AmazonSecretsManagerClient"),
}


def write(root, name, content):
    target = root / name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content)


def prefix(service):
    return ("channel::" if service in ("sqs", "sns", "eventbridge", "kinesis") else "resource::") + service


def physical(service, stem):
    if service == "ssm":
        return "/neutral/" + stem
    if service == "secretsmanager":
        return "neutral/" + stem
    return "neutral-" + stem


def identifier(service, name):
    if service == "sqs":
        return "https://sqs.eu-west-1.amazonaws.com/123456789012/" + name
    if service == "sns":
        return "arn:aws:sns:eu-west-1:123456789012:" + name
    return name


def source(lang, service, operation, field, shape, mode, name, variable):
    """Public SDK spellings, authored before inspecting any candidate answers."""
    jclient, cnamespace, cclient = CLIENTS[service]
    marker = "probe_" + mode
    expr = json.dumps(identifier(service, name))
    expr2 = json.dumps(identifier(service, name + "-other"))
    constructor_mode = mode not in ("type_only", "hidden", "name_hop")
    if lang == "java":
        model = "software.amazon.awssdk.services." + service + ".model."
        if mode == "env":
            expr = 'System.getenv("' + variable + '")'
        setter = field[0].lower() + field[1:]
        val = expr
        if shape == "many":
            val = "java.util.List.of(" + expr + (", " + expr2 if mode == "multiple" else "") + ")"
        if shape == "entries":
            setter, val = "entries", model + "PutEventsRequestEntry.builder().eventBusName(" + expr + ").build()"
        if shape == "entries" and mode == "multiple":
            val += ", " + model + "PutEventsRequestEntry.builder().eventBusName(" + expr2 + ").build()"
        if shape.endswith("-map"):
            setter = "requestItems"
            entry = model + ("KeysAndAttributes.builder().build()" if shape == "read-map" else "WriteRequest.builder().build()")
            if shape == "write-map":
                entry = "java.util.List.of(" + entry + ")"
            val = "java.util.Map.of(" + expr + ", " + entry + (", " + expr2 + ", " + entry if mode == "multiple" else "") + ")"
        request = model + operation + "Request.builder()." + setter + "(" + val + ").build()"
        ctor = jclient + (".builder().build()" if mode == "builder" else ".create()")
        params, setup = "", jclient + " conduit = " + ctor + "; var alias = conduit;"
        if not constructor_mode:
            params, setup = jclient + " conduit", "var alias = conduit;"
        if mode == "exact_hop":
            setup = jclient + " conduit = makeClient(); var alias = conduit;"
        if mode == "name_hop":
            params, setup = "foreign.Factory opaque", jclient + " conduit = opaque.makeClient(); var alias = conduit;"
        extras = ""
        if mode in ("exact_hop", "name_hop"):
            extras = "static " + jclient + " makeClient() { return " + ctor + "; }\n"
        if mode == "hidden_constructor":
            extras = "static class " + jclient + " { static " + jclient + " create() { return new " + jclient + "(); } void " + operation[0].lower()+operation[1:] + "(Object request) {} }\n"
        if mode == "hidden":
            extras = "static " + jclient + " conduit = " + ctor + ";\n"
        body = ', software.amazon.awssdk.core.sync.RequestBody.fromString("neutral")' if service == "s3" and operation == "PutObject" else ""
        call = "alias." + operation[0].lower() + operation[1:] + "(" + request + body + ");"
        return ("import software.amazon.awssdk.services." + service + "." + jclient + ";\n"
                + "class Case {\n" + extras + "void " + marker + "(" + params + ") { " + setup + " " + call + " }\n}\n"), marker
    if lang == "csharp":
        # .NET S3 uses SDK-specific names; preserve the logical operation in the oracle.
        if service == "s3":
            field = "BucketName"
            if operation == "HeadObject":
                operation = "GetObjectMetadata"
        if mode == "env":
            expr = 'Environment.GetEnvironmentVariable("' + variable + '")'
        setter, val = field, expr
        if shape == "many":
            val = "new List<string> { " + expr + (", " + expr2 if mode == "multiple" else "") + " }"
        if shape == "entries":
            setter, val = "Entries", "new List<PutEventsRequestEntry> { new PutEventsRequestEntry { EventBusName = " + expr + " } }"
        if shape == "entries" and mode == "multiple":
            val = val[:-2] + ", new PutEventsRequestEntry { EventBusName = " + expr2 + " } }"
        if shape.endswith("-map"):
            setter = "RequestItems"
            typ = "KeysAndAttributes" if shape == "read-map" else "List<WriteRequest>"
            value = "new KeysAndAttributes()" if shape == "read-map" else "new List<WriteRequest> { new WriteRequest() }"
            val = "new Dictionary<string, " + typ + "> { [" + expr + "] = " + value + (", [" + expr2 + "] = " + value if mode == "multiple" else "") + " }"
        request = "new " + operation + "Request { " + setter + " = " + val + " }"
        params, setup = "", "var conduit = new " + cclient + "(); var alias = conduit;"
        if not constructor_mode:
            params, setup = cclient + " conduit", "var alias = conduit;"
        if mode == "exact_hop":
            setup = "var conduit = MakeClient(); var alias = conduit;"
        if mode == "name_hop":
            params, setup = "dynamic opaque", "var conduit = opaque.MakeClient(); var alias = conduit;"
        extras = ""
        if mode in ("exact_hop", "name_hop"):
            extras = "static " + cclient + " MakeClient() { return new " + cclient + "(); }\n"
        if mode == "hidden_constructor":
            extras = "class " + cclient + " { public void " + operation + "Async(object r) {} }\n"
        if mode == "hidden":
            extras = "static " + cclient + " conduit = new " + cclient + "();\n"
        return ("using System;\nusing System.Collections.Generic;\nusing " + cnamespace + ";\nusing " + cnamespace + ".Model;\n"
                + "class Case {\n" + extras + "void " + marker + "(" + params + ") { " + setup + " alias."
                + operation + "Async(" + request + "); }\n}\n"), marker
    if mode == "env":
        expr = 'os.Getenv("' + variable + '")'
    setter, val = field, "aws.String(" + expr + ")"
    if shape == "many":
        val = "[]string{" + expr + (", " + expr2 if mode == "multiple" else "") + "}"
    if shape == "entries":
        setter, val = "Entries", "[]types.PutEventsRequestEntry{{EventBusName: aws.String(" + expr + ")}}"
    if shape == "entries" and mode == "multiple":
        val = val[:-1] + ", {EventBusName: aws.String(" + expr2 + ")}}"
    if shape.endswith("-map"):
        setter = "RequestItems"
        typ = "types.KeysAndAttributes" if shape == "read-map" else "[]types.WriteRequest"
        val = "map[string]" + typ + "{" + expr + ": {}" + (", " + expr2 + ": {}" if mode == "multiple" else "") + "}"
    request = "&" + service + "." + operation + "Input{" + setter + ": " + val + "}"
    params, setup = "", "conduit := " + service + ".NewFromConfig(aws.Config{}); alias := conduit;"
    if not constructor_mode:
        params, setup = "conduit *" + service + ".Client", "alias := conduit;"
    if mode == "exact_hop":
        setup = "conduit := makeClient(); alias := conduit;"
    if mode == "name_hop":
        params, setup = "opaque foreign.Factory", "conduit := opaque.makeClient(); alias := conduit;"
    extras = ""
    if mode in ("exact_hop", "name_hop"):
        extras = "func makeClient() *" + service + ".Client { return " + service + ".NewFromConfig(aws.Config{}) }\n"
    if mode == "hidden":
        extras = "var conduit = " + service + ".NewFromConfig(aws.Config{})\n"
    if mode == "hidden_constructor":
        params = service + " localFactory"
        extras = "type localFactory struct{}\nfunc (localFactory) NewFromConfig(c aws.Config) *foreign.Client { return nil }\n"
    imports = ['"context"', '"os"', '"github.com/aws/aws-sdk-go-v2/aws"',
               '"github.com/aws/aws-sdk-go-v2/service/' + service + '"',
               '"github.com/aws/aws-sdk-go-v2/service/' + service + '/types"',
               'foreign "example.com/zz-prove-external/factory"']
    return ("package probe\nimport (\n" + "\n".join(imports) + "\n)\n" + extras
            + "func " + marker + "(" + params + ") { " + setup + " alias." + operation
            + "(context.Background(), " + request + ") }\n"), marker


def seed(run):
    roots, oracle = {}, []
    # Separate directories per case avoid accidental method-name collisions making an
    # intended exact wrapper a name match. There is one repository per language.
    for lang in ("java", "go", "csharp"):
        root = run / "work" / lang
        root.mkdir(parents=True)
        roots[lang] = root
        git("init", "-q", cwd=root)
        git("remote", "add", "origin", "https://" + REMOTE + lang + ".git", cwd=root)
        if lang == "go":
            write(root, "go.mod", "module " + REMOTE + lang + "\n\ngo 1.22\n")
        for i, (service, operation, field, role, shape) in enumerate(OPS):
            modes = ["literal", "env", "type_only"]
            if shape != "one":
                modes += ["multiple"]
            if not any(s == service for s, *_ in OPS[:i]):
                modes += ["exact_hop", "name_hop", "hidden", "hidden_constructor"]
                if lang == "java":
                    modes += ["builder"]
            for mode in modes:
                stem = service + "-" + operation.lower()
                name = physical(service, stem)
                variable = "ZZ_PROVE_" + lang.upper() + "_" + operation.upper()
                code, marker = source(lang, service, operation, field, shape, mode, name, variable)
                class_name = "Probe_" + service + "_" + operation + "_" + mode
                if lang != "go":
                    code = code.replace("class Case {", "class " + class_name + " {")
                extension = {"java": "java", "go": "go", "csharp": "cs"}[lang]
                path = service + "/" + operation.lower() + "/" + mode + "/Case." + extension
                if lang == "go":
                    path = path[:-7] + "case.go"
                write(root, path, code)
                oracle.append(dict(language=lang, service=service, operation=operation, mode=mode,
                                   path=path, marker=marker, class_name=class_name, name=name, variable=variable, role=role,
                                   contract=prefix(service) + "::" + name,
                                   accepted=mode in ("literal", "env", "exact_hop", "builder", "multiple"),
                                   unresolved=mode == "env" or operation == "GetParametersByPath",
                                   contracts=[prefix(service) + "::" + n for n in ([name, name + "-other"] if mode == "multiple" else [name])]))
        # A separate neutral provisioner gives each literal use one exact reference.
        infra = run / "work" / ("infra-" + lang)
        infra.mkdir()
        git("init", "-q", cwd=infra)
        git("remote", "add", "origin", "https://" + REMOTE + "infra-" + lang + ".git", cwd=infra)
        write(root, "meada.toml", '[[peer]]\nrepo = "' + REMOTE + 'infra-' + lang + '"\n')
        write(infra, "meada.toml", '[[peer]]\nrepo = "' + REMOTE + lang + '"\n')
        types = {"sqs": ("aws_sqs_queue", "name"), "sns": ("aws_sns_topic", "name"),
                 "eventbridge": ("aws_cloudwatch_event_bus", "name"), "kinesis": ("aws_kinesis_stream", "name"),
                 "s3": ("aws_s3_bucket", "bucket"), "dynamodb": ("aws_dynamodb_table", "name"),
                 "ssm": ("aws_ssm_parameter", "name"), "secretsmanager": ("aws_secretsmanager_secret", "name")}
        tf = ""
        for i, row in enumerate(r for r in oracle if r["language"] == lang and r["mode"] == "literal"):
            typ, attr = types[row["service"]]
            tf += 'resource "' + typ + '" "r_' + str(i) + '" { ' + attr + ' = ' + json.dumps(row["name"]) + ' }\n'
        for i, row in enumerate(r for r in oracle if r["language"] == lang and r["mode"] == "multiple"):
            typ, attr = types[row["service"]]
            tf += 'resource "' + typ + '" "extra_' + str(i) + '" { ' + attr + ' = ' + json.dumps(row["name"] + "-other") + ' }\n'
        write(infra, "neutral.tf", tf)
        git("add", ".", cwd=infra)
        git("add", ".", cwd=root)
    write(run, "oracle.json", json.dumps(oracle, indent=2))
    inventory = {str(p.relative_to(run / "work")): hashlib.sha256(p.read_bytes()).hexdigest()
                 for root in (run / "work").iterdir() for p in root.rglob("*") if p.is_file() and ".git" not in p.parts}
    write(run, "inputs.sha256.json", json.dumps(inventory, indent=2, sort_keys=True))
    return roots, oracle


class Probe:
    def __init__(self, run):
        self.run, self.checks = run, []

    def check(self, name, passed, detail=None):
        self.checks.append(dict(check=name, passed=bool(passed), detail=detail))
        write(self.run, "checks.json", json.dumps(self.checks, indent=2))

    def cli(self, binary, root, label, *args):
        p = subprocess.run([binary, *args], cwd=root, capture_output=True, timeout=900)
        write(self.run, label + ".out", p.stdout.decode(errors="replace"))
        write(self.run, label + ".err", p.stderr.decode(errors="replace"))
        write(self.run, label + ".exit", str(p.returncode))
        if p.returncode:
            raise RuntimeError("command failed; see " + label + ".err")
        return p.stdout.decode()


def rows_for(data, row):
    needle = "::" + row["path"] + "::"
    return [(role, side) for role in ("providers", "consumers") for side in data.get(role, [])
            if needle in side["site"] and row["marker"] in side["site"]]


def judge(probe, lang, data, oracle):
    expected_count = sum(len(row["contracts"]) for row in oracle if row["accepted"])
    all_sides = [(role, side) for role in ("providers", "consumers") for side in data.get(role, [])
                 if (side.get("contract") or "").startswith(("channel::", "resource::"))
                 or side.get("contract") is None]
    probe.check(lang + " complete side count", len(all_sides) == expected_count,
                dict(expected=expected_count, actual=len(all_sides), sides=all_sides))
    for row in oracle:
        got = rows_for(data, row)
        label = "/".join([lang, row["service"], row["operation"], row["mode"]])
        if row["mode"] in ("name_hop", "exact_hop"):
            hops = [c for c in data.get("calls", []) if c.get("span", {}).get("path") == row["path"]
                    and "makeclient" in c["id"].lower() and row["marker"] in c.get("caller", "")]
            wanted_levels = ("inferred", "ambiguous") if row["mode"] == "name_hop" else ("exact",)
            probe.check(label + " control really exercises " + row["mode"], len(hops) == 1
                        and hops[0].get("level") in wanted_levels, hops)
        if not row["accepted"]:
            probe.check(label + " contributes no side", got == [], got)
        elif row["unresolved"]:
            # The public unresolved representation may live in contract or via. Never
            # accept a literal contract, a missing provenance, or a wrong direction.
            combined = json.dumps(got)
            unresolved = len(got) == 1 and (got[0][1].get("contract") in (None, "")
                or "*" in (got[0][1].get("contract") or "")
                or "unresolved" in (got[0][1].get("contract") or "").lower()
                or "env:" in combined
                or got[0][1].get("contract") == prefix(row["service"]))
            probe.check(label + (" unresolved and names variable" if row["mode"] == "env" else " hierarchy prefix stays unresolved"), unresolved and got[0][0] == row["role"]
                        and (got[0][1].get("contract") in (None, "") or got[0][1]["contract"].startswith(prefix(row["service"])))
                        and (row["mode"] != "env" or row["variable"] in combined)
                        and row["contract"] not in combined, got)
        else:
            probe.check(label + " exact contract and direction",
                        len(got) == len(row["contracts"]) and all(role == row["role"] for role, _ in got)
                        and {side.get("contract") for _, side in got} == set(row["contracts"]), got)
    for row in oracle:
        if row["accepted"] and not row["unresolved"]:
            got = rows_for(data, row)
            for _, side in got:
                links = [p for p in data.get("provisions", []) if p["side"] == side["site"]]
                probe.check(lang + " exact provisioning " + row["path"], len(links) == 1
                            and links[0]["contract"] == side["contract"] and links[0]["level"] == "exact"
                            and "infra-" + lang + "::neutral.tf::" in links[0]["provisioner"], links)
    probe.check(lang + " complete provisions, unresolved sides never linked",
                len(data.get("provisions", [])) == sum(len(r["contracts"]) for r in oracle if r["accepted"] and not r["unresolved"]),
                data.get("provisions", []))
    allowed_sites = {side["site"] for row in oracle if row["accepted"] for _, side in rows_for(data, row)}
    probe.check(lang + " all SDK sides belong to expected call or handler",
                all(side["site"] in allowed_sites for _, side in all_sides), all_sides)
    probe.check(lang + " export format stays 1", data.get("format") == 1, data.get("format"))


def queries_for(lang, oracle):
    queries = []
    for row in oracle:
        # Qualified file plus function disambiguates the repeated local names.
        symbol = REMOTE + lang + "::" + row["path"] + "::" + row["class_name"] + "." + row["marker"]
        if lang == "go":
            symbol = REMOTE + lang + "::" + row["path"] + "::" + row["marker"]
        if row["accepted"]:
            queries.append({"tool": "context", "arguments": {"symbol": symbol, "budget": 30000}})
        if row["mode"] == "literal" and not row["unresolved"]:
            queries.append({"tool": "search", "arguments": {"query": row["contract"], "kind": "contract", "budget": 30000}})
            queries.append({"tool": "trace", "arguments": {"from": symbol, "direction": "up", "budget": 30000}})
    queries += [{"tool": "search", "arguments": {"query": "zz_prove_missing_sdk", "budget": 300}},
                {"tool": "search", "arguments": {"query": "neutral", "kind": "zz-invalid-sdk-kind"}}]
    return queries


def transports(probe, binary, root, lang, oracle, http):
    queries = queries_for(lang, oracle)
    tools, texts, _ = mcp_answers(binary, root, queries, probe.run / (lang + ".stdio.err"))
    write(probe.run, lang + ".queries.json", json.dumps(queries, indent=2))
    write(probe.run, lang + ".stdio.json", json.dumps(dict(tools=tools, answers=texts), indent=2))
    probe.check(lang + " MCP has tools and complete answer count", bool(tools) and len(texts) == len(queries))
    index = 0
    for row in oracle:
        if row["accepted"]:
            text = texts[index]
            wanted = [row["variable"]] if row["mode"] == "env" else (["unresolved"] if row["unresolved"] else row["contracts"])
            probe.check(lang + " context " + row["path"], all(w in text for w in wanted) and not text.startswith(("ERROR", "TOOL ERROR")), text)
            index += 1
        if row["mode"] == "literal" and not row["unresolved"]:
            probe.check(lang + " search " + row["contract"], row["contract"] in texts[index] and "no matches" not in texts[index], texts[index])
            index += 1
            probe.check(lang + " trace up reaches provisioner " + row["path"], row["marker"] in texts[index]
                        and "neutral.tf" in texts[index] and "provisions" in texts[index]
                        and "exact" in texts[index] and not texts[index].startswith(("ERROR", "TOOL ERROR")), texts[index])
            index += 1
    probe.check(lang + " missing search exactly empty", texts[-2] == "no matches for `zz_prove_missing_sdk`\n", texts[-2])
    probe.check(lang + " bad kind refused", texts[-1].startswith(("ERROR", "TOOL ERROR")), texts[-1])
    if not http:
        return
    manifest = probe.run / (lang + ".workspace.toml")
    write(probe.run, manifest.name, 'index_dir = "' + lang + '-indexes"\n[[repository]]\nrepo = "' + REMOTE + lang + '"\npath = "' + str(root) + '"\n[[repository]]\nrepo = "' + REMOTE + 'infra-' + lang + '"\npath = "' + str(root.parent / ("infra-" + lang)) + '"\n')
    probe.cli(binary, probe.run, lang + ".server-index", "index", "--workspace", str(manifest))
    port = free_port()
    with open(probe.run / (lang + ".http.out"), "w") as out, open(probe.run / (lang + ".http.err"), "w") as err:
        server = subprocess.Popen([binary, "mcp", "--listen", "127.0.0.1:" + str(port), "--workspace", str(manifest)],
                                  cwd=probe.run, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        write(probe.run, lang + ".http.pid", str(server.pid))
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError("HTTP process exited before readiness")
                try:
                    socket.create_connection(("127.0.0.1", port), timeout=.2).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http("http://127.0.0.1:" + str(port) + "/" + REMOTE + lang)
            http_tools = client.call("tools/list", {})["result"]["tools"]
            http_texts = [answer(client.call("tools/call", {"name": q["tool"], "arguments": q["arguments"]})) for q in queries]
            write(probe.run, lang + ".http.json", json.dumps(dict(tools=http_tools, answers=http_texts), indent=2))
            probe.check(lang + " HTTP tools equal stdio", http_tools == tools)
            probe.check(lang + " HTTP all answers byte equal to stdio", http_texts == texts, len(texts))
        finally:
            server.terminate()
            server.wait(timeout=30)
            write(probe.run, lang + ".http.stopped", str(server.returncode))


def refresh_probe(probe, binary, root, lang, oracle, original):
    row = next(r for r in oracle if r["service"] == "s3" and r["operation"] == "PutObject" and r["mode"] == "literal")
    path = root / row["path"]
    before = path.read_text()
    changed_name = row["name"][:-1] + "x"
    changed_contract = prefix(row["service"]) + "::" + changed_name
    query = {"name": "search", "arguments": {"query": changed_contract, "kind": "contract", "budget": 30000}}
    server = Stdio(binary, root, probe.run / (lang + ".refresh.stdio.err"))
    captures = {}
    try:
        # Initialize the persistent session and establish its old indexed answer.
        old_query = {"name": "search", "arguments": {"query": row["contract"], "kind": "contract", "budget": 30000}}
        captures["before"] = answer(server.call("tools/call", old_query))
        path.write_text(before.replace(row["name"], changed_name))
        captures["edited"] = answer(server.call("tools/call", query))
        captures["old_after_edit"] = answer(server.call("tools/call", old_query))
        edited = json.loads(probe.cli(binary, root, lang + ".edited-export", "export"))
        got = rows_for(edited, row)
        probe.check(lang + " same-size source edit changes exact SDK side", len(before) == len(path.read_text())
                    and len(got) == 1 and got[0][1].get("contract") == changed_contract, got)
        probe.check(lang + " persistent MCP sees edit and drops old side",
                    changed_contract in captures["edited"] and "no matches" not in captures["edited"]
                    and row["path"] not in captures["old_after_edit"], captures)
        path.write_text(before)
        captures["restored"] = answer(server.call("tools/call", old_query))
        restored = json.loads(probe.cli(binary, root, lang + ".restored-export", "export"))
        probe.check(lang + " persistent MCP restore returns original answer", captures["restored"] == captures["before"], captures)
        probe.check(lang + " SDK edit reverse restores complete export", restored == original)
    finally:
        path.write_text(before)
        server.close()
        write(probe.run, lang + ".refresh.json", json.dumps(captures, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--head-bin")
    parser.add_argument("--run", required=True)
    parser.add_argument("--http", action="store_true")
    parser.add_argument("--seed-only", action="store_true")
    args = parser.parse_args()
    if not args.seed_only and not args.head_bin:
        parser.error("--head-bin required except with --seed-only")
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    probe = Probe(run)
    os.environ["LANG"] = "C.UTF-8"
    try:
        roots, oracle = seed(run)
        if args.seed_only:
            print("inputs authored; no product proof run: " + str(run))
            return 0
        # A tempting process value must not resolve an SDK side. Only repository
        # workload configuration is allowed to supply one (ADR0025/26).
        for row in oracle:
            if row["mode"] == "env":
                os.environ[row["variable"]] = identifier(row["service"], row["name"])
        binary = str(Path(args.head_bin).resolve())
        probe.cli(binary, run, "head-identity", "--version")
        write(run, "binary.sha256", hashlib.sha256(Path(binary).read_bytes()).hexdigest())
        for lang, root in roots.items():
            rows = [row for row in oracle if row["language"] == lang]
            export = json.loads(probe.cli(binary, root, lang + ".export", "export"))
            judge(probe, lang, export, rows)
            probe.cli(binary, root, lang + ".status", "status")
            transports(probe, binary, root, lang, rows, args.http)
            refresh_probe(probe, binary, root, lang, rows, export)
        failures = [c for c in probe.checks if not c["passed"]]
        result = "differences found" if failures else "checks passed; independent checker must judge"
        write(run, "sdk.md", "# AWS SDK neutral probe\n\n" + result + "\n\n" + str(len(probe.checks)) + " checks; " + str(len(failures)) + " failures.\n\n" + "\n".join("- " + c["check"] for c in failures) + "\n")
        print(result + ": " + str(len(failures)) + " failures, " + str(len(probe.checks)) + " checks")
        return int(bool(failures))
    except Exception:
        write(run, "blocked.txt", traceback.format_exc())
        print("BLOCKED: see " + str(run / "blocked.txt"))
        return 2


if __name__ == "__main__":
    sys.exit(main())
