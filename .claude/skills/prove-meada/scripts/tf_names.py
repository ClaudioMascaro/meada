"""The static evaluation oracle of prove-meada (#650): evaluates a Terraform node's `name`, `count`
and `for_each` from python-hcl2's reading of the files (expressions come as `${...}` text), by
ticket #650's criteria and ADR 0025's rules, never Meada's code. It knows a subset of the forms:
literals, templates, `var.<x>`, `local.<x>`, `each.key`, `each.value`, `count.index`,
`module.<call>[key].<output>`, lists and the calls `upper`, `lower`, `format` (`%s`, `%d`),
`join`, `toset` and `length`. A form outside the subset is UNCHECKED: the node's name is not
compared. A part the ticket says is never evaluated (a data source, a resource attribute, a
registry module, `terraform.workspace`, a sensitive or undeclared variable, a call outside ADR
0025's list) is a `*`, and the name a pattern.
"""

import json
import re

EVAL_DEPTH = 32
# ADR 0025's pure functions (#650 adds `jsonencode`): a call to any other is unresolved.
PURE = {"format", "formatlist", "join", "split", "lower", "upper", "title", "trim", "trimprefix",
        "trimsuffix", "trimspace", "chomp", "replace", "substr", "concat", "coalesce", "element",
        "keys", "values", "lookup", "merge", "length", "toset", "tolist", "tomap", "tostring",
        "tonumber", "jsonencode"}
IDENT = r"[A-Za-z_][A-Za-z0-9_-]*"


class Unchecked(Exception):
    """A form this oracle does not evaluate."""


# A value: ("known", python value), ("pattern", text) or ("unknown", None), with `through`, a
# frozenset of (path, line, "var.<x>", tfvars).
UNKNOWN = ("unknown", None, frozenset())


def known(v, through=frozenset()):
    return ("known", v, through)


def split_top(text, sep=","):
    """`text` split at `sep` outside brackets and strings."""
    parts, depth, cur, i, quote = [], 0, "", 0, False
    while i < len(text):
        c = text[i]
        if quote:
            cur += c
            if c == "\\" and i + 1 < len(text):
                cur += text[i + 1]
                i += 1
            elif c == '"':
                quote = False
        elif c == '"':
            quote, cur = True, cur + c
        elif c in "([{":
            depth, cur = depth + 1, cur + c
        elif c in ")]}":
            depth, cur = depth - 1, cur + c
        elif c == sep and depth == 0:
            parts.append(cur.strip())
            cur = ""
        else:
            cur += c
        i += 1
    if cur.strip():
        parts.append(cur.strip())
    return parts


def split_op(text, op):
    """`text` split at the last `op` outside brackets and strings, when there is one."""
    depth, quote, i, at = 0, False, 0, None
    while i < len(text):
        c = text[i]
        if quote:
            if c == "\\":
                i += 1
            elif c == '"':
                quote = False
        elif c == '"':
            quote = True
        elif c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif depth == 0 and text.startswith(op, i):
            at = i
        i += 1
    return [text] if at is None else [text[:at].strip(), text[at + len(op):].strip()]


def template_parts(body):
    """A quoted template's body as ("lit", text) and ("expr", text) parts; a directive raises."""
    parts, i = [], 0
    while i < len(body):
        if body.startswith("%{", i):
            raise Unchecked("a directive")
        if body.startswith("${", i):
            depth, j, quote = 1, i + 2, False
            while j < len(body) and depth:
                c = body[j]
                if quote:
                    if c == "\\":
                        j += 1
                    elif c == '"':
                        quote = False
                elif c == '"':
                    quote = True
                elif c == "{":
                    depth += 1
                elif c == "}":
                    depth -= 1
                j += 1
            parts.append(("expr", body[i + 2:j - 1].strip()))
            i = j
            continue
        j = i
        while j < len(body) and not body.startswith("${", j) and not body.startswith("%{", j):
            j += 1
        text = body[i:j]
        try:
            text = json.loads('"' + text + '"')
        except ValueError:
            raise Unchecked("an escape")
        parts.append(("lit", text))
        i = j
    return parts


def text_of(v):
    """A value as a template shows it, and whether it is unresolved."""
    kind, value, _ = v
    if kind == "known" and isinstance(value, bool):
        return ("true" if value else "false"), False
    if kind == "known" and isinstance(value, (str, int, float)):
        return str(value), False
    if kind == "pattern":
        return value, True
    return "*", True


class Names:
    """The evaluation of one root: `instances` is the oracle's list of instances (dir, parent,
    children, call), `modules` its modules."""

    def __init__(self, modules, instances, root_dir):
        self.modules, self.instances, self.root_dir = modules, instances, root_dir
        self.memo, self.open, self.outputs = {}, set(), False

    def name(self, at, kind, n, depth):
        key = (at, kind, n)
        if key in self.memo:
            return self.memo[key]
        if depth > EVAL_DEPTH or key in self.open:
            return UNKNOWN
        self.open.add(key)
        try:
            m = self.modules[self.instances[at]["dir"]]
            if kind == "local":
                found = self.raw(at, None, m["locals_raw"][n], depth + 1) if n in m["locals_raw"] else UNKNOWN
            elif kind == "output":
                found = self.raw(at, None, m["outputs_raw"][n], depth + 1) if n in m["outputs_raw"] else UNKNOWN
            else:
                found = self.var(at, n, depth)
        finally:
            self.open.discard(key)
        self.memo[key] = found
        return found

    def var(self, at, n, depth):
        inst = self.instances[at]
        m = self.modules[inst["dir"]]
        v = m["variables"].get(n)
        if v is None or v["sensitive"]:
            return UNKNOWN
        if inst["parent"] is not None:
            caller, block = inst["parent"]
            if n in block["args_raw"]:
                return self.raw(caller, inst["call"], block["args_raw"][n], depth + 1)
            return self.raw(at, None, v["default"], depth + 1) if v["has_default"] else UNKNOWN
        if n in m["tfvars"]:
            raw, path, line = m["tfvars"][n]
            found, mark = self.raw(at, None, raw, depth + 1), (path, line, f"var.{n}", True)
        elif v["has_default"]:
            found, mark = self.raw(at, None, v["default"], depth + 1), (v["path"], v["line"], f"var.{n}", False)
        else:
            return UNKNOWN
        return (found[0], found[1], found[2] | {mark})

    def raw(self, at, each, value, depth):
        """A value as python-hcl2 gives it."""
        if depth > EVAL_DEPTH:
            return UNKNOWN
        if isinstance(value, bool) or isinstance(value, (int, float)) or value is None:
            return known(value)
        if isinstance(value, list):
            items = [self.raw(at, each, x, depth) for x in value]
            return self.collection(items, lambda vs: vs)
        if isinstance(value, dict):
            keys = [k[1:-1] if k.startswith('"') else k for k in value if not k.startswith("__")]
            items = [self.raw(at, each, v, depth) for k, v in value.items() if not k.startswith("__")]
            return self.collection(items, lambda vs: dict(zip(keys, vs)))
        s = value
        if s.startswith("${") and s.endswith("}") and s.count("${") == 1:
            return self.expr(at, each, s[2:-1].strip(), depth)
        if len(s) >= 2 and s[0] == s[-1] == '"':
            return self.template(at, each, s[1:-1], depth)
        raise Unchecked(f"a value {s[:40]!r}")

    @staticmethod
    def collection(items, build):
        through = frozenset().union(*[t for _, _, t in items]) if items else frozenset()
        if all(k == "known" for k, _, _ in items):
            return known(build([v for _, v, _ in items]), through)
        return UNKNOWN

    def template(self, at, each, body, depth):
        text, open_, through = "", False, frozenset()
        for kind, part in template_parts(body):
            if kind == "lit":
                text += part
                continue
            v = self.expr(at, each, part, depth)
            shown, unresolved = text_of(v)
            text, open_, through = text + shown, open_ or unresolved, through | v[2]
        if not open_:
            return known(text, through)
        while "**" in text:
            text = text.replace("**", "*")
        return ("pattern", text, through)

    def expr(self, at, each, e, depth):
        e = e.strip()
        if re.fullmatch(r'"(?:[^"\\]|\\.)*"', e):
            return self.template(at, each, e[1:-1], depth)
        if re.fullmatch(r"-?\d+", e):
            return known(int(e))
        if e in ("true", "false"):
            return known(e == "true")
        if e.startswith("[") and e.endswith("]"):
            items = [self.expr(at, each, x, depth) for x in split_top(e[1:-1])]
            return self.collection(items, lambda vs: vs)
        ternary = split_top(e, "?")
        if len(ternary) == 2 and len(split_top(ternary[1], ":")) == 2:
            cond = self.expr(at, each, ternary[0], depth)
            if cond[0] != "known" or not isinstance(cond[1], bool):
                return UNKNOWN
            chosen = self.expr(at, each, split_top(ternary[1], ":")[cond[1] is False], depth)
            return (chosen[0], chosen[1], chosen[2] | cond[2])
        for op in ("||", "&&", "==", "!=", ">=", "<=", ">", "<"):
            sides = split_op(e, op)
            if len(sides) == 2:
                a, b = (self.expr(at, each, x, depth) for x in sides)
                if a[0] != "known" or b[0] != "known":
                    return UNKNOWN
                if op in (">=", "<=", ">", "<") and not all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in (a[1], b[1])):
                    return UNKNOWN
                value = {"||": lambda: a[1] or b[1], "&&": lambda: a[1] and b[1],
                         "==": lambda: a[1] == b[1], "!=": lambda: a[1] != b[1], ">=": lambda: a[1] >= b[1],
                         "<=": lambda: a[1] <= b[1], ">": lambda: a[1] > b[1], "<": lambda: a[1] < b[1]}[op]()
                return known(value, a[2] | b[2])
        if e.startswith("!"):
            a = self.expr(at, each, e[1:], depth)
            return known(not a[1], a[2]) if a[0] == "known" and isinstance(a[1], bool) else UNKNOWN
        if e.startswith("(") and e.endswith(")") and len(split_top(e[1:-1], "\0")) == 1:
            return self.expr(at, each, e[1:-1], depth)
        call = re.fullmatch(rf"({IDENT})\((.*)\)", e, re.S)
        if call:
            return self.call(at, each, call.group(1), split_top(call.group(2)), depth)
        m = re.fullmatch(rf"(var|local)\.({IDENT})", e)
        if m:
            return self.name(at, m.group(1), m.group(2), depth)
        if e in ("each.key", "each.value", "count.index"):
            if each is None:
                return UNKNOWN
            key, value, through = each
            return known({"each.key": key, "each.value": value, "count.index": key}[e], through)
        m = re.fullmatch(rf"module\.({IDENT})(?:\[(\d+|\"[^\"]*\")\])?\.({IDENT})", e)
        if m:
            if not self.outputs:
                return UNKNOWN
            key = None if m.group(2) is None else (int(m.group(2)) if m.group(2).isdigit() else m.group(2)[1:-1])
            children = [c for k, c in self.instances[at]["children"].get(m.group(1), []) if k == key]
            return self.name(children[0], "output", m.group(3), depth + 1) if len(children) == 1 else UNKNOWN
        if re.fullmatch(rf"{IDENT}(\.{IDENT}|\[[^\[\]]*\])+", e):
            if e.split(".")[0] in ("var", "local", "each", "count", "module"):
                raise Unchecked(f"a traversal {e[:40]!r}")
            return UNKNOWN  # a data source, a resource attribute, terraform.workspace, path.*
        raise Unchecked(f"an expression {e[:40]!r}")

    def call(self, at, each, f, args, depth):
        if f not in PURE:
            return UNKNOWN
        if f not in ("upper", "lower", "format", "join", "toset", "length", "coalesce"):
            raise Unchecked(f"the function {f}")
        vals = [self.expr(at, each, a, depth) for a in args]
        if any(k != "known" for k, _, _ in vals):
            return UNKNOWN
        through = frozenset().union(*[t for _, _, t in vals])
        xs = [v for _, v, _ in vals]
        if f in ("upper", "lower"):
            if not isinstance(xs[0], str):
                raise Unchecked("a case of a non-string")
            return known(xs[0].upper() if f == "upper" else xs[0].lower(), through)
        if f == "coalesce":
            found = [x for x in xs if x is not None and x != ""]
            return known(found[0], through) if found else UNKNOWN
        if f == "length":
            if isinstance(xs[0], str):
                return known(len(xs[0]), through)
            return known(len(xs[0]), through)
        if f == "toset":
            if not all(isinstance(x, str) for x in xs[0]):
                raise Unchecked("a set of non-strings")
            return known(sorted(set(xs[0])), through)
        if f == "join":
            if len(xs) != 2 or not all(isinstance(x, str) for x in xs[1]):
                raise Unchecked("a join of non-strings")
            return known(xs[0].join(xs[1]), through)
        spec, rest = xs[0], list(xs[1:])
        if re.search(r"%[^sd%]", spec):
            raise Unchecked("a format verb")
        out = re.sub(r"%([sd%])", lambda mm: "%" if mm.group(1) == "%" else text_of(known(rest.pop(0)))[0], spec)
        return known(out, through)

    def keys(self, at, block):
        """The instance keys and `each` of a block in the instance `at`: [(key, each)], or None."""
        try:
            if block.get("count_raw") is not None:
                kind, n, through = self.raw(at, None, block["count_raw"], 0)
                if kind == "known" and isinstance(n, int) and not isinstance(n, bool) and 0 <= n <= 1000:
                    return [(i, (i, None, through)) for i in range(n)]
                return None
            if block.get("for_each_raw") is None:
                return None
            kind, v, through = self.raw(at, None, block["for_each_raw"], 0)
        except Unchecked:
            return "unchecked"
        if kind != "known":
            return None
        if isinstance(v, dict):
            entries = sorted(v.items())
        elif isinstance(v, list) and all(isinstance(x, str) for x in v):
            entries = [(x, x) for x in sorted(set(v))]
        else:
            return None
        if len(entries) > 1000:
            return None
        return [(k, (k, val, through)) for k, val in entries]

    def physical(self, at, each, block):
        """(name, pattern, through) of a block's `name`, or "unchecked"."""
        if block["kind"] == "module" or block.get("name_raw") is None:
            return None, False, ()
        try:
            kind, v, through = self.raw(at, each, block["name_raw"], 0)
        except Unchecked:
            return "unchecked"
        if kind == "pattern":
            return v, True, tuple(sorted(through))
        if kind == "unknown":
            return "*", True, ()
        if isinstance(v, bool):
            return ("true" if v else "false"), False, tuple(sorted(through))
        if isinstance(v, (str, int, float)):
            return str(v), False, tuple(sorted(through))
        return None, False, ()
