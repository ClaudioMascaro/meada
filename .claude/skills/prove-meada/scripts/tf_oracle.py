"""The A7 IaC nodes oracle of prove-meada (#649): reads a repository's Terraform files with
python-hcl2 (a Lark grammar of HCL, never Meada's code) and derives, by Terraform's own address
rules and ticket #649's criteria, the nodes (address, type, physical name, file, lines, root
module) and the references between them, then the answers `search` kind `iac`, `context` and
`trace` must give. Conventions where the spec names no rule are listed in
references/config-iac.md ("IaC oracle conventions").
"""

import json
import re
import subprocess

import hcl2  # python-hcl2: see SKILL.md's Gotchas for the venv that holds it

from tf_names import Names, Unchecked

IDENT = r"[A-Za-z_][A-Za-z0-9_-]*"
# A traversal: a root name, then attributes, indexes and splats; not after a `.`, a word
# character or a quote, so `a.b` inside `x.a.b` or `"a.b"` is not a second one.
TRAVERSAL = re.compile(rf"(?<![\w.\"-])({IDENT})((?:\.{IDENT}|\.\*|\[[^\[\]]*\])+)")
STEP = re.compile(rf"\.({IDENT})|\.\*|\[([^\[\]]*)\]")
SKIPPED_ROOTS = {"each", "count", "path", "terraform", "self"}
EVAL_DEPTH = 32  # the ceiling ADR 0025 and docs/agents/ceilings name for names gone through
MODULE_DEPTH = 16
NESTING = 64  # #640: a file nesting brackets deeper is unreadable


def unquote(s):
    """A literal string hcl2 keeps quoted (`"\\"orders\\""`) as its text; anything else None."""
    if isinstance(s, str) and len(s) >= 2 and s[0] == s[-1] == '"' and "${" not in s and "%{" not in s:
        try:
            return json.loads(s)
        except ValueError:
            return s[1:-1]
    return None


def key_of(text):
    """An index's literal key: an int, a string, or None for anything else."""
    text = text.strip()
    if re.fullmatch(r"\d+", text):
        return int(text)
    if re.fullmatch(r'"[^"\\]*"', text):
        return text[1:-1]
    return None


def shown_key(key):
    return f"[{key}]" if isinstance(key, int) else "[" + json.dumps(key, ensure_ascii=False) + "]"


def refs_in(value):
    """Every reference a value's expressions make: (kind, type, name, key, output) for a block
    (kind `resource`, `data` or `module`), ('var', name) or ('local', name)."""
    out = []
    if isinstance(value, dict):
        for k, v in value.items():
            if not k.startswith("__"):
                out += refs_in(v)
    elif isinstance(value, list):
        for v in value:
            out += refs_in(v)
    elif isinstance(value, str) and "${" in value:
        for m in TRAVERSAL.finditer(value):
            root, rest = m.group(1), m.group(2)
            steps = [(s.group(1), s.group(2)) for s in STEP.finditer(rest)]
            attrs = lambda at: steps[at][0] if at < len(steps) and steps[at][0] else None
            def keyed(at):
                if at < len(steps) and steps[at][0] is None:  # an index or a splat
                    k = steps[at][1]
                    return (key_of(k) if k is not None else None), at + 1
                return None, at
            if root in SKIPPED_ROOTS:
                continue
            if root in ("var", "local"):
                if attrs(0):
                    out.append((root, attrs(0)))
            elif root == "module":
                if attrs(0):
                    key, at = keyed(1)
                    out.append(("module", "", attrs(0), key, attrs(at)))
            elif root == "data":
                if attrs(0) and attrs(1):
                    out.append(("data", attrs(0), attrs(1), keyed(2)[0], None))
            elif attrs(0):
                out.append(("resource", root, attrs(0), keyed(1)[0], None))
    return out


def lines_of(text, kind, labels):
    """The first and last line of the block `kind "label"...` in `text`: its header found by
    pattern, its end by matching braces outside strings and heredocs."""
    head = r"\s+".join([re.escape(kind)] + [rf'"{re.escape(l)}"' for l in labels])
    m = re.search(rf"(?m)^[ \t]*{head}\s*\{{", text)
    if not m:
        return None, None
    start = text.count("\n", 0, m.start()) + 1
    depth, i, n = 0, m.end() - 1, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif text.startswith("<<", i):
            h = re.match(r"<<-?(\w+)\n", text[i:])
            if h:
                end = re.search(rf"(?m)^[ \t]*{h.group(1)}[ \t]*$", text[i + h.end():])
                i += h.end() + (end.end() if end else 0)
                continue
        elif c in "#" or text.startswith("//", i):
            while i < n and text[i] != "\n":
                i += 1
            continue
        elif text.startswith("/*", i):
            j = text.find("*/", i)
            i = (j + 2) if j >= 0 else n
            continue
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return start, text.count("\n", 0, i) + 1
        i += 1
    return start, None


def local_source(directory, source):
    if not isinstance(source, str) or not source.startswith(("./", "../")):
        return None
    parts = [p for p in directory.split("/") if p]
    for p in source.split("/"):
        if p in ("", "."):
            continue
        if p == "..":
            if not parts:
                return None
            parts.pop()
        else:
            parts.append(p)
    return "/".join(parts)


def tf_files(repo):
    """The `.tf` and `.tofu` files the walk visits: tracked or untracked, not ignored, no hidden
    part, no symlink on the way, and no `.tf` file a `.tofu` one of the same name shadows."""
    listed = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=repo,
                            check=True, capture_output=True, text=True).stdout.split("\0")
    out = []
    for rel in sorted(p for p in listed if p.endswith((".tf", ".tofu"))):
        parts = rel.split("/")
        if any(p.startswith(".") for p in parts):
            continue
        if any((repo.joinpath(*parts[:i + 1])).is_symlink() for i in range(len(parts))):
            continue
        out.append(rel)
    kept = set(out)
    return [p for p in out if not (p.endswith(".tf") and p[:-3] + ".tofu" in kept)]


def depth_past(text):
    depth = 0
    for c in text:
        if c in "{[(":
            depth += 1
            if depth > NESTING:
                return True
        elif c in "}])":
            depth = max(0, depth - 1)
    return False


def read_repo(repo):
    """Per directory: its blocks, outputs and locals; and the files it could not read."""
    modules, unreadable = {}, []
    for rel in tf_files(repo):
        text = (repo / rel).read_text(errors="replace")
        try:
            if depth_past(text):
                raise ValueError("nested past 64 brackets")
            doc = hcl2.loads(text)
        except Exception as e:
            unreadable.append((rel, str(e).splitlines()[0][:120] if str(e) else type(e).__name__))
            continue
        d = rel.rsplit("/", 1)[0] if "/" in rel else ""
        m = module_of(modules, d)
        for entry in doc.get("variable", []):
            for name, body in entry.items():
                name_ = unquote(name) or name
                m["variables"][name_] = {"default": body.get("default"), "has_default": "default" in body,
                                         "sensitive": body.get("sensitive") is True, "path": rel,
                                         "line": lines_of(text, "variable", [name_])[0]}
        for kind in ("resource", "data"):
            for entry in doc.get(kind, []):
                for ty, named in entry.items():
                    for name, body in named.items():
                        ty_, name_ = unquote(ty) or ty, unquote(name) or name
                        first, last = lines_of(text, kind, [ty_, name_])
                        m["blocks"].append({"kind": kind, "type": ty_, "name": name_, "path": rel, "line": first, "end": last,
                                            "name_raw": body.get("name"), "count_raw": body.get("count"),
                                            "for_each_raw": body.get("for_each"),
                                            "refs": refs_in(body), "source": None, "args": {}})
        for entry in doc.get("module", []):
            for name, body in entry.items():
                name_ = unquote(name) or name
                first, last = lines_of(text, "module", [name_])
                m["blocks"].append({"kind": "module", "type": "module", "name": name_, "path": rel, "line": first, "end": last,
                                    # a module's `name` is an input, not what it creates
                                    "count_raw": body.get("count"), "for_each_raw": body.get("for_each"),
                                    "args_raw": {k: v for k, v in body.items() if not k.startswith("__")},
                                    "refs": refs_in(body),
                                    "source": unquote(body.get("source")),
                                    "args": {k: refs_in(v) for k, v in body.items() if not k.startswith("__")}})
        for entry in doc.get("output", []):
            for name, body in entry.items():
                m["outputs"][unquote(name) or name] = refs_in(body.get("value"))
                if "value" in body:
                    m["outputs_raw"][unquote(name) or name] = body["value"]
        for entry in doc.get("locals", []):
            for k, v in entry.items():
                if not k.startswith("__"):
                    m["locals"][k] = refs_in(v)
                    m["locals_raw"][k] = v
    # #650: the var files Terraform loads by itself, `terraform.tfvars` first, then each
    # `*.auto.tfvars` in name order, a later value winning; a value that is no constant is left out.
    def rank(r):
        f = r.rsplit("/", 1)[-1]
        return (r.rsplit("/", 1)[0] if "/" in r else "", 0 if f == "terraform.tfvars" else 1 if f == "terraform.tfvars.json" else 2, f)
    as_hcl2 = lambda v: (json.dumps(v) if isinstance(v, str) else [as_hcl2(x) for x in v] if isinstance(v, list)
                         else {k: as_hcl2(x) for k, x in v.items()} if isinstance(v, dict) else v)
    for rel in sorted(var_files(repo), key=rank):
        text = (repo / rel).read_text(errors="replace")
        try:
            doc = hcl2.loads(text) if not rel.endswith(".json") else as_hcl2(json.loads(text))
        except Exception:
            continue
        m = module_of(modules, rel.rsplit("/", 1)[0] if "/" in rel else "")
        for k, v in doc.items():
            if k.startswith("__") or "${" in json.dumps(v):
                continue
            line = re.search(rf"^\s*{re.escape(k)}\s*=", text, re.M) if not rel.endswith(".json") else re.search(re.escape(json.dumps(k)), text)
            m["tfvars"][k] = (v, rel, text.count("\n", 0, line.start()) + 1 if line else 1)
    for m in modules.values():
        m["blocks"].sort(key=lambda b: (b["path"], b["line"] or 0))
    return modules, unreadable


def module_of(modules, d):
    return modules.setdefault(d, {"blocks": [], "outputs": {}, "locals": {}, "outputs_raw": {},
                                  "locals_raw": {}, "variables": {}, "tfvars": {}})


def var_files(repo):
    """The var files the walk visits: `terraform.tfvars` and `*.auto.tfvars`, as `tf_files`."""
    listed = subprocess.run(["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=repo,
                            check=True, capture_output=True, text=True).stdout.split("\0")
    out = []
    names = ("terraform.tfvars", "terraform.tfvars.json")
    for rel in sorted(p for p in listed if p.rsplit("/", 1)[-1] in names or p.endswith((".auto.tfvars", ".auto.tfvars.json"))):
        parts = rel.split("/")
        if any(p.startswith(".") for p in parts) or any(repo.joinpath(*parts[:i + 1]).is_symlink() for i in range(len(parts))):
            continue
        out.append(rel)
    return out


def local_address(b):
    return {"resource": f"{b['type']}.{b['name']}", "data": f"data.{b['type']}.{b['name']}",
            "module": f"module.{b['name']}"}[b["kind"]]


def evaluate(repo):
    """(nodes, references, unreadable files): nodes as dicts with address, root, type, name,
    path, line, end; references as (root, source, target, via)."""
    modules, unreadable = read_repo(repo)
    called = {t for d, m in modules.items() for b in m["blocks"]
              if b["kind"] == "module" and (t := local_source(d, b["source"])) not in (None, d) and t in modules}
    nodes, references = [], set()
    for root_dir in sorted(d for d in modules if d not in called):
        root = root_dir or "."
        instances = []  # {dir, parent: (instance, block), nodes: {(kind,type,name): [(key, address)]}, children: {name: [(key, instance)]}}

        names, pending, unchecked = Names(modules, instances, root_dir), [], []

        def instantiate(directory, prefix, parent, call, stack):
            me = len(instances)
            instances.append({"dir": directory, "parent": parent, "call": call, "nodes": {}, "children": {}})
            for b in modules[directory]["blocks"]:
                keys = names.keys(me, b)
                if keys == "unchecked":
                    unchecked.append(f"{b['path']}:{b['line']}")
                for key, each in (keys if isinstance(keys, list) else [(None, None)]):
                    address = prefix + local_address(b) + ("" if key is None else shown_key(key))
                    node = {"address": address, "root": root, "type": b["type"], "name": None, "pattern": False,
                            "through": (), "unchecked": False, "path": b["path"], "line": b["line"], "end": b["end"]}
                    nodes.append(node)
                    pending.append((node, me, b, each))
                    instances[me]["nodes"].setdefault((b["kind"], b["type"], b["name"]), []).append((key, address))
                    target = local_source(directory, b["source"]) if b["kind"] == "module" else None
                    if target in modules and target not in stack and len(stack) <= MODULE_DEPTH:
                        child = instantiate(target, address + ".", (me, b), each, stack + [target])
                        instances[me]["children"].setdefault(b["name"], []).append((key, child))
            return me

        instantiate(root_dir, "", None, None, [root_dir])
        # Every instance made, module outputs are read: the names are evaluated again with them.
        names.memo, names.outputs = {}, True
        for node, me, b, each in pending:
            found = names.physical(me, each, b)
            if found == "unchecked":
                node["unchecked"] = True
            else:
                node["name"], node["pattern"], node["through"] = found
            node["root_unchecked"] = bool(unchecked)

        def pick(items, key):
            named = [t for k, t in items if key is not None and k == key]
            return named or [t for _, t in items]

        def resolve(at, ref, depth, open_):
            if depth > EVAL_DEPTH or (at, ref) in open_:
                return set()
            open_ = open_ | {(at, ref)}
            inst, found = instances[at], set()
            if ref[0] in ("resource", "data", "module"):
                kind, ty, name, key, output = ref
                if kind == "module" and output:
                    for child in pick(inst["children"].get(name, []), key):
                        for inner in modules[instances[child]["dir"]]["outputs"].get(output, []):
                            found |= resolve(child, inner, depth + 1, open_)
                if not found:
                    found = set(pick(inst["nodes"].get((kind, "module" if kind == "module" else ty, name), []), key))
            elif ref[0] == "var" and inst["parent"]:
                caller, block = inst["parent"]
                for inner in block["args"].get(ref[1], []):
                    found |= resolve(caller, inner, depth + 1, open_)
            elif ref[0] == "local":
                for inner in modules[inst["dir"]]["locals"].get(ref[1], []):
                    found |= resolve(at, inner, depth + 1, open_)
            return found

        def via(ref):
            if ref[0] == "module" and ref[4]:
                return f"module.{ref[2]}{'' if ref[3] is None else shown_key(ref[3])}.{ref[4]}"
            return f"{ref[0]}.{ref[1]}" if ref[0] in ("var", "local") else None

        for i, inst in enumerate(instances):
            for b in modules[inst["dir"]]["blocks"]:
                for _, source in inst["nodes"].get((b["kind"], b["type"], b["name"]), []):
                    for ref in b["refs"]:
                        for target in resolve(i, ref, 0, frozenset()):
                            if target != source:
                                references.add((root, source, target, via(ref)))
    return nodes, sorted(references), unreadable


# Where a hit line's name goes when the oracle could not evaluate it (`prove.py` matches any).
UNCHECKED_NAME = "\x00"


def node_line(identity, n):
    return f"{identity} {n['path']}:{n['line']} {n['address']}"


def search_hits(identity, nodes, query):
    """The lines `search` kind `iac` gives for `query`, in order (ADR 0025's rank, then the
    documented tie-break: repository, path, line, address, root)."""
    q = query.lower()
    hits = []
    for n in nodes:
        ranks = [r for f in (n["address"], n["type"], n["name"]) if f is not None
                 for r in [0 if f.lower() == q else 1 if f.lower().startswith(q) else 2 if q in f.lower() else None]
                 if r is not None]
        if not ranks:
            continue
        word = "pattern" if n["pattern"] else "name"
        name = "" if n["name"] is None else f", {word} " + json.dumps(n["name"], ensure_ascii=False)
        if n["unchecked"]:
            name = UNCHECKED_NAME
        key = f"{identity} {n['path']} {n['line']:010} {n['address']} {n['root']}"
        hits.append((min(ranks), key, f"{node_line(identity, n)} ({n['type']}{name}, root {n['root']})"))
    return [h[2] for h in sorted(hits)]


def neighbours(nodes, references, node, down):
    by = {(n["root"], n["address"]): n for n in nodes}
    found = []
    for root, source, target, via in references:
        if root != node["root"]:
            continue
        if (source if down else target) == node["address"]:
            other = by[(root, target if down else source)]
            found.append((other, via))
    found.sort(key=lambda p: (p[0]["path"], p[0]["line"], p[0]["address"], p[1] or ""))
    return found


def context_lines(identity, nodes, references, node):
    """The lines `context` on `node` gives before its `values:` section."""
    name = "none" if node["name"] is None else json.dumps(node["name"], ensure_ascii=False)
    name += " (pattern)" if node["pattern"] else ""
    lines = [f"{identity} {node['path']}:{node['line']}-{node['end']} {node['address']}",
             f"type: {node['type']}", f"name: {name}", f"root: {node['root']}"]
    # #650: the root variables the name went through, in path and line order.
    lines.append("overridable:" if node["through"] else "overridable: none")
    lines += [f"{identity} {path}:{line} {var} ({'tfvars' if tfvars else 'default'})"
              for path, line, var, tfvars in node["through"]]
    for title, down in (("references", True), ("referenced by", False)):
        got = neighbours(nodes, references, node, down)
        lines.append(f"{title}:" if got else f"{title}: none")
        lines += [node_line(identity, m) + (f" via {v}" if v else "") for m, v in got]
    return lines


def trace_lines(identity, nodes, references, node, down):
    """The lines of `trace` from `node` before its footer: depth first, children in path, line,
    address and via order, `(cycle)` for an ancestor and `(see above)` for one already followed."""
    lines, expanded = [], set()

    def walk(n, via, depth, path):
        text = "  " * depth + node_line(identity, n) + (f" via {via}" if via else "")
        if n["address"] in path:
            lines.append(text + " (cycle)")
            return
        if n["address"] in expanded:
            lines.append(text + " (see above)")
            return
        lines.append(text)
        expanded.add(n["address"])
        for m, v in neighbours(nodes, references, n, down):
            walk(m, v, depth + 1, path + [n["address"]])

    walk(node, None, 0, [])
    return lines
