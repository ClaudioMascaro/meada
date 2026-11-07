"""The A7 CloudFormation and SAM oracle of prove-meada (#653): reads a repository's YAML and JSON
files with PyYAML (one constructor for every short-form tag, `yaml.compose` for the lines; never
Meada's code) and derives, by CloudFormation's own rules and ticket #653's criteria, the nodes
(logical id, type, physical name, file, lines, template as root) and the references between them,
in the shape `tf_oracle.py` gives, so `prove.py iac` checks `search` kind `iac`, `context` and
`trace` on both. Conventions where the ticket names no rule are listed in
references/config-iac.md ("CloudFormation oracle conventions").
"""

import re
import subprocess
from pathlib import Path, PurePosixPath

import yaml  # PyYAML: see SKILL.md's Gotchas

SAM = "AWS::Serverless-2016-10-31"
MAX_BYTES = 1 << 20
EVAL_DEPTH = 32
NAME_MAX = 1024
# type -> (the property that holds the physical name, whether `Ref` returns the name itself).
NAMED = {
    "AWS::DynamoDB::GlobalTable": ("TableName", True),
    "AWS::DynamoDB::Table": ("TableName", True),
    "AWS::Events::EventBus": ("Name", True),
    "AWS::Events::Rule": ("Name", False),
    "AWS::IAM::Role": ("RoleName", True),
    "AWS::Kinesis::Stream": ("Name", True),
    "AWS::Lambda::Function": ("FunctionName", True),
    "AWS::Logs::LogGroup": ("LogGroupName", True),
    "AWS::RDS::DBInstance": ("DBInstanceIdentifier", True),
    "AWS::S3::Bucket": ("BucketName", True),
    "AWS::SecretsManager::Secret": ("Name", False),
    "AWS::Serverless::Function": ("FunctionName", True),
    "AWS::Serverless::SimpleTable": ("TableName", True),
    "AWS::SNS::Topic": ("TopicName", False),
    "AWS::SQS::Queue": ("QueueName", False),
    "AWS::SSM::Parameter": ("Name", True),
    "AWS::StepFunctions::StateMachine": ("StateMachineName", False),
}


class Loader(yaml.SafeLoader):
    pass


def _tagged(loader, suffix, node):
    name = suffix if suffix in ("Ref", "Condition") else f"Fn::{suffix}"
    if isinstance(node, yaml.ScalarNode):
        value = loader.construct_scalar(node)
    elif isinstance(node, yaml.SequenceNode):
        value = loader.construct_sequence(node, deep=True)
    else:
        value = loader.construct_mapping(node, deep=True)
    return {name: value}


Loader.add_multi_constructor("!", _tagged)


def files(repo):
    """The YAML and JSON files the walk visits: tracked or untracked, not ignored, no hidden
    part, no symlink on the way."""
    listed = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=repo,
                            check=True, capture_output=True, text=True).stdout.split("\0")
    out = []
    for rel in sorted(p for p in listed if p.endswith((".yaml", ".yml", ".json"))):
        parts = rel.split("/")
        if any(p.startswith(".") for p in parts):
            continue
        if any(repo.joinpath(*parts[:i + 1]).is_symlink() for i in range(len(parts))):
            continue
        out.append(rel)
    return out


def is_template(doc):
    if not isinstance(doc, dict) or not isinstance(doc.get("Resources"), dict):
        return False
    transform = doc.get("Transform")
    sam = transform == SAM or (isinstance(transform, list) and SAM in transform)
    return "AWSTemplateFormatVersion" in doc or sam


def scalar_lines(node, out):
    """The line of every value scalar under `node`, keys left out."""
    stack = [node]
    while stack:
        n = stack.pop()
        if isinstance(n, yaml.MappingNode):
            stack.extend(v for _, v in n.value)
        elif isinstance(n, yaml.SequenceNode):
            stack.extend(n.value)
        elif n.value != "":
            out.append(n.start_mark.line + 1)


def no_echo(param):
    """`NoEcho` true: PyYAML types a plain `yes` or `on` as true (YAML 1.1, as CloudFormation's
    parser does); a string reads as true in any case for `true`, `yes`, `on` and `y`."""
    if not isinstance(param, dict):
        return False
    value = param.get("NoEcho")
    return value is True or (isinstance(value, str) and value.lower() in ("true", "yes", "on", "y"))


class Names:
    def __init__(self, path, params, resources, lines):
        self.path, self.params, self.resources, self.lines = path, params, resources, lines
        self.memo, self.open = {}, set()

    def unknown(self):
        return "*", True, set()

    def join(self, parts, between=""):
        text, pattern, through = "", False, set()
        for i, (t, p, th) in enumerate(parts):
            text += (between if i else "") + t
            pattern |= p
            through |= th
            if len(text) > NAME_MAX:
                return self.unknown()
        return text, pattern, through

    def name_of(self, rid, depth):
        if rid in self.memo:
            return self.memo[rid]
        if rid in self.open:
            return self.unknown()
        self.open.add(rid)
        res = self.resources[rid]
        prop = NAMED.get(res["Type"], (None,))[0]
        value = (res.get("Properties") or {}).get(prop) if prop else None
        if value is None or isinstance(value, list):
            found = None
        else:
            found = self.eval(value, depth)
        self.open.discard(rid)
        self.memo[rid] = found
        return found

    def ref(self, name, depth):
        if name in self.params:
            p = self.params[name]
            default = p.get("Default")
            kind = str(p.get("Type", "String"))
            listed = kind.startswith(("List<", "AWS::SSM::Parameter::Value<")) or kind == "CommaDelimitedList"
            if no_echo(p) or listed or not isinstance(default, (str, int, float, bool)):
                return self.unknown()
            text = str(default).lower() if isinstance(default, bool) else str(default)
            var = (self.path, self.lines.get(("param", name), 1), f"Parameters.{name}", False)
            return text, False, {var}
        res = self.resources.get(name)
        if res and NAMED.get(res["Type"], (0, False))[1]:
            return self.name_of(name, depth + 1) or self.unknown()
        return self.unknown()

    def attribute(self, rid, attr, depth):
        res = self.resources.get(rid)
        if res and NAMED.get(res["Type"], (None,))[0] == attr:
            return self.name_of(rid, depth + 1) or self.unknown()
        return self.unknown()

    def eval(self, v, depth):
        if depth > EVAL_DEPTH:
            return self.unknown()
        if isinstance(v, bool):
            return str(v).lower(), False, set()
        if isinstance(v, (str, int, float)):
            return str(v), False, set()
        if not isinstance(v, dict) or len(v) != 1:
            return self.unknown()
        (fn, arg), = v.items()
        if fn == "Ref" and isinstance(arg, str):
            return self.ref(arg, depth)
        if fn == "Fn::GetAtt":
            got = getatt(arg)
            return self.attribute(got[0], got[1], depth) if got and got[1] is not None else self.unknown()
        if fn == "Fn::Sub":
            if isinstance(arg, str):
                return self.sub(arg, {}, depth)
            if isinstance(arg, list) and len(arg) == 2 and isinstance(arg[0], str) and isinstance(arg[1], dict):
                return self.sub(arg[0], arg[1], depth)
            return self.unknown()
        if fn == "Fn::Join" and isinstance(arg, list) and len(arg) == 2 and isinstance(arg[1], list):
            delimiter = self.eval(arg[0], depth + 1)
            if delimiter[1]:
                return self.unknown()
            parts = [self.eval(i, depth + 1) for i in arg[1]]
            text, pattern, through = self.join(parts, delimiter[0])
            return text, pattern, through | delimiter[2]
        return self.unknown()

    def sub(self, text, variables, depth):
        parts = []
        for piece in re.split(r"(\$\{[^}]*\})", text):
            m = re.fullmatch(r"\$\{([^}]*)\}", piece)
            if not m:
                parts.append((piece, False, set()))
            elif m.group(1).startswith("!"):
                parts.append(("${" + m.group(1)[1:] + "}", False, set()))
            else:
                var = m.group(1).strip()
                if var in variables:
                    parts.append(self.eval(variables[var], depth + 1))
                elif "." in var:
                    rid, attr = var.split(".", 1)
                    parts.append(self.attribute(rid, attr, depth))
                else:
                    parts.append(self.ref(var, depth))
        return self.join(parts)


def getatt(arg):
    """(resource, attribute); the attribute is None when computed (`!Ref Param`, the one function
    CloudFormation allows there)."""
    if isinstance(arg, str) and "." in arg:
        return tuple(arg.split(".", 1))
    if isinstance(arg, list) and len(arg) == 2 and isinstance(arg[0], str):
        if isinstance(arg[1], str):
            return arg[0], arg[1]
        if isinstance(arg[1], dict):
            return arg[0], None
    return None


def sub_variables(text, variables):
    """The `Resource` of each `${Resource}` and `${Resource.Attribute}` the map does not define."""
    out = []
    for m in re.finditer(r"\$\{([^}]*)\}", text):
        var = m.group(1).strip()
        if var.startswith("!") or var in variables:
            continue
        out.append(var.split(".", 1)[0])
    return out


def references_of(rid, body, resources):
    found = set()

    def add(target, via):
        if target != rid and target in resources:
            found.add((target, via))

    deps = body.get("DependsOn")
    for t in [deps] if isinstance(deps, str) else deps if isinstance(deps, list) else []:
        if isinstance(t, str):
            add(t, "DependsOn")
    stack = [v for k, v in body.items() if k != "DependsOn"]
    while stack:
        v = stack.pop()
        if isinstance(v, list):
            stack.extend(v)
        elif isinstance(v, dict):
            if len(v) == 1:
                (fn, arg), = v.items()
                if fn == "Ref" and isinstance(arg, str):
                    add(arg, "Ref")
                elif fn == "Fn::GetAtt":
                    got = getatt(arg)
                    if got:
                        add(got[0], "Fn::GetAtt" if got[1] is None else f"Fn::GetAtt.{got[1]}")
                elif fn == "Fn::Sub":
                    if isinstance(arg, str):
                        names = sub_variables(arg, {})
                    elif isinstance(arg, list) and len(arg) == 2 and isinstance(arg[0], str) and isinstance(arg[1], dict):
                        names = sub_variables(arg[0], arg[1])
                    else:
                        names = []
                    for n in names:
                        add(n, "Fn::Sub")
            stack.extend(v.values())
    return found


def templates(repo, unreadable):
    """(path, parsed document, composed root node) of each template under `repo`; the files it
    could not read go to `unreadable`."""
    repo = Path(repo)
    for rel in files(repo):
        raw = (repo / rel).read_bytes()
        if len(raw) > MAX_BYTES or not any(m in raw for m in (b"AWSTemplateFormatVersion", SAM.encode())):
            continue
        try:
            text = raw.decode()
            root_node = yaml.compose(text, Loader=Loader)
            doc = yaml.load(text, Loader=Loader)
        except Exception as e:  # not UTF-8, not YAML, not JSON
            unreadable.append((rel, str(e)[:80]))
            continue
        if root_node is not None and is_template(doc):
            yield rel, doc, root_node


def redacted_defaults(repo):
    """How many parameters have `NoEcho` and a scalar `Default`: the config values `meada status`
    counts as redacted that the base counted as plain."""
    return sum(1 for _, doc, _ in templates(repo, [])
               for p in (doc.get("Parameters") or {}).values()
               if isinstance(p, dict) and no_echo(p) and "Default" in p)


def evaluate(repo):
    """(nodes, references, unreadable files), as `tf_oracle.evaluate` gives them."""
    nodes, references, unreadable = [], set(), []
    for rel, doc, root_node in templates(repo, unreadable):
        lines, spans = {}, {}
        for key, value in root_node.value:
            top = key.value
            if top == "Parameters":
                for pk, pv in value.value:
                    for k, v in pv.value:
                        if k.value == "Default":
                            lines[("param", pk.value)] = v.start_mark.line + 1
            if top == "Resources":
                for rk, rv in value.value:
                    found = []
                    scalar_lines(rv, found)
                    spans[rk.value] = (rk.start_mark.line + 1, max(found + [rk.start_mark.line + 1]))
        resources = {rid: body for rid, body in doc["Resources"].items()
                     if isinstance(body, dict) and isinstance(body.get("Type"), str) and "::" in body["Type"]}
        params = {k: v for k, v in (doc.get("Parameters") or {}).items() if isinstance(v, dict)}
        names = Names(rel, params, resources, lines)
        for rid in sorted(resources):
            body = resources[rid]
            if rid not in spans:  # a resource that came by a merge key or an alias has no lines
                continue
            found = names.name_of(rid, 0)
            name, pattern, through = (None, False, set()) if found is None else found
            first, last = spans[rid]
            nodes.append({"address": rid, "root": rel, "type": body["Type"], "name": name, "pattern": pattern,
                          "through": tuple(sorted(through)), "unchecked": False, "path": rel,
                          "line": first, "end": last,
                          **sam_code_fields(rel, doc, body)})
            for target, via in sorted(references_of(rid, body, resources))[:64]:
                references.add((rel, rid, target, via))
    return nodes, sorted(references), unreadable


def sam_code_fields(rel, doc, resource):
    """Literal SAM CodeUri and Handler, with function properties overriding Globals (#664).

    These are source metadata, not a claim that the code exists or executes. A computed
    function property masks its global default rather than inheriting a guessed value.
    """
    if resource.get("Type") != "AWS::Serverless::Function":
        return {}
    globals_ = doc.get("Globals") or {}
    defaults = (globals_.get("Function") or {}) if isinstance(globals_, dict) else {}
    properties = resource.get("Properties") or {}
    fields = {**(defaults if isinstance(defaults, dict) else {}),
              **(properties if isinstance(properties, dict) else {})}
    result = {}
    code = fields.get("CodeUri")
    if isinstance(code, str) and code and len(code.encode()) <= NAME_MAX \
            and not code.startswith("/") and "\\" not in code and ":" not in code \
            and ".." not in code.split("/") and "${" not in code:
        result["build"] = str(PurePosixPath(rel).parent / code)
    handler = fields.get("Handler")
    if isinstance(handler, str) and handler and len(handler.encode()) <= NAME_MAX and "${" not in handler:
        result["handler"] = handler
    return result
