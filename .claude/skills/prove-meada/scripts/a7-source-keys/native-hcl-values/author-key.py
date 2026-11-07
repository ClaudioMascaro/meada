#!/usr/bin/env python3
"""Author neutral native-HCL source and its independent, source-only answer key.

This is a source authoring tool, not a Meada runner or a result-derived oracle.
The row declarations below are the intended format semantics of the written source.
No Meada package, binary, output, database, or implementation table is read.
"""

import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess


BASE = Path(__file__).resolve().parent
REPO = BASE / "repo"
IDENTITY = "example.com/a7-proof/native-hcl-values"
ORIGIN = "https://example.com/a7-proof/native-hcl-values.git"
ROOT_SOURCE = "fe0faf5dfd7408571bf982bc395ef10d1e456a0b"
VALUE_BYTES = 1024
SOURCES = {}
ROWS = []
ABSENT = []


def save(path, data):
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def source(path, text):
    assert text.endswith("\n")
    assert path not in SOURCES
    SOURCES[path] = text
    dest = REPO / path
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text(text)


def line_of(path, anchor):
    lines = SOURCES[path].splitlines()
    matches = [i + 1 for i, line in enumerate(lines) if anchor in line]
    assert len(matches) == 1, (path, anchor, matches)
    return matches[0]


def row(path, key, kind, value, anchor, *, group=None, lexeme=None):
    assert kind in ("string", "number", "boolean", "expression", "template")
    assert isinstance(value, str)
    assert "proof" in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", key), key
    ROWS.append({
        "id": path + "#" + key,
        "file": path,
        "key": key,
        "line": line_of(path, anchor),
        "type": kind,
        "source_value": value,
        "source_lexeme": lexeme,
        "redaction_rule": group,
    })


def absent(path, key, anchor, reason):
    ABSENT.append({"file": path, "key": key, "line": line_of(path, anchor), "reason": reason})


def materialize(source_rows, phase):
    result = []
    for declaration in source_rows:
        entry = copy.deepcopy(declaration)
        value = entry.pop("source_value")
        rule = entry["redaction_rule"]
        redacted = rule == "unreadable_ancestor" or (rule in ("sensitive_default", "sensitive_tfvars") and phase != "sensitivity_public")
        if phase == "literal_changed" and entry["id"] == "botany.tf#locals.proof.refresh":
            value = "willow"
        if phase == "sensitivity_public" and entry["id"] == "sensory/variables.tf#variable.proof.sensitive":
            value = "false"
        if redacted:
            stored = None
            cut = False
            shown = "redacted"
        else:
            encoded = value.encode("utf-8")
            cut = len(encoded) > VALUE_BYTES
            stored = encoded[:VALUE_BYTES].decode("utf-8", errors="ignore") if cut else value
            if entry["type"] == "string":
                shown = json.dumps(stored, ensure_ascii=False)
            else:
                shown = stored.replace("\r", "\\r").replace("\n", "\\n")
            if cut:
                shown += " (cut)"
        entry.update({"value": stored, "cut": cut, "redacted": redacted, "shown_value": shown})
        entry["rendered_line"] = f"{IDENTITY} {entry['file']}:{entry['line']} {entry['key']} = {shown}"
        result.append(entry)
    return result


def rank(entry, needle):
    texts = [entry["key"].lower()]
    if entry["value"] is not None:
        texts.append(entry["value"].lower())
    needle = needle.lower()
    if needle in texts:
        return 0
    if any(text.startswith(needle) for text in texts):
        return 1
    if any(needle in text for text in texts):
        return 2
    return None


def selected(entries, needle):
    picks = [(rank(entry, needle), entry) for entry in entries]
    picks = [(r, entry) for r, entry in picks if r is not None]
    picks.sort(key=lambda pair: (pair[0], pair[1]["file"], pair[1]["line"], pair[1]["key"]))
    return [{"id": entry["id"], "rank": r, "rendered_line": entry["rendered_line"]} for r, entry in picks]


def status(entries):
    redacted = sum(entry["redacted"] for entry in entries)
    return {
        "config_files_read": 11,
        "config_files_skipped": 2,
        "config_files_lockfiles": 0,
        "config_files_unreadable": 2,
        "config_values": len(entries),
        "config_values_redacted": redacted,
        "config_line": f"config files: 11 read, 2 skipped (lockfiles: 0, unreadable: 2); values: {len(entries)}, redacted: {redacted}",
        "iac_nodes": 1,
        "shadowed_files_counted": 0,
        "declared_resource_unlinked_ids": ["resource::native_fixture::botanical", "resource::native_fixture::edge", "resource::native_fixture::masked"],
        "stale_via_count": 0,
        "iac_orphan_lines": 0,
        "unknown_or_unaccounted_config_status_rows_allowed": False,
    }


def main():
    REPO.mkdir(parents=True, exist_ok=True)
    source("botany.tf", '''# Independent native-HCL scalar source. Comments emit no values.
locals {
  proof = {
    empty = ""
    integer = 17
    decimal = 12.50
    exponent = 1.25e+03
    truth = true
    falsity = false
    nothing = null
    escaped = "line\\nnext\\t\\\"\\\\tail"
    unicode = "caf\\u00e9"
    refresh = "maples"
    ranked_exact = "MossNeedle"
    ranked_prefix = "mossneedle-top"
    ranked_substring = "rim-mossneedle-tail"
    scan_exact = "qz"
    scan_prefix = "qz-leaf"
    scan_substring = "a-qz-z"
    items = [
      "cypress",
      5,
      false,
      null,
      { "a.b" = "cedar" }
    ]
    object = {
      plain = "ash"
      "dash-key" = "elm"
      "region/east" = "fir"
      "quote\\\"back\\\\slash" = "hazel"
      "snow☃" = "holly"
    }
    empty_list = []
    empty_object = {}
    reference = var.proof
    call = upper("seed")
    arithmetic = 2 + 3
    unary = -7
    conditional = var.proof ? "yes" : "no"
    comprehension = [for item in ["x", "y"] : upper(item)]
    dynamic_object = { (var.proof) = "dynamic", literal = "kept-in-expression" }
    multiline = (
      2 +
      3
    )
  }
}
''')
    p = "botany.tf"
    for key, kind, value, anchor in [
        ("empty", "string", "", 'empty = ""'),
        ("integer", "number", "17", "integer = 17"),
        ("decimal", "number", "12.50", "decimal = 12.50"),
        ("exponent", "number", "1.25e+03", "exponent = 1.25e+03"),
        ("truth", "boolean", "true", "truth = true"),
        ("falsity", "boolean", "false", "falsity = false"),
        ("escaped", "string", 'line\nnext\t"\\tail', "escaped ="),
        ("unicode", "string", "café", "unicode ="),
        ("refresh", "string", "maples", 'refresh = "maples"'),
        ("ranked_exact", "string", "MossNeedle", "ranked_exact ="),
        ("ranked_prefix", "string", "mossneedle-top", "ranked_prefix ="),
        ("ranked_substring", "string", "rim-mossneedle-tail", "ranked_substring ="),
        ("scan_exact", "string", "qz", "scan_exact ="),
        ("scan_prefix", "string", "qz-leaf", "scan_prefix ="),
        ("scan_substring", "string", "a-qz-z", "scan_substring ="),
        ("reference", "expression", "var.proof", "reference ="),
        ("call", "expression", 'upper("seed")', "call ="),
        ("arithmetic", "expression", "2 + 3", "arithmetic ="),
        ("unary", "expression", "-7", "unary ="),
        ("conditional", "expression", 'var.proof ? "yes" : "no"', "conditional ="),
        ("comprehension", "expression", '[for item in ["x", "y"] : upper(item)]', "comprehension ="),
        ("dynamic_object", "expression", '{ (var.proof) = "dynamic", literal = "kept-in-expression" }', "dynamic_object ="),
        ("multiline", "expression", "(\n      2 +\n      3\n    )", "multiline ="),
    ]:
        row(p, "locals.proof." + key, kind, value, anchor)
    for key, kind, value, anchor in [
        ("items[0]", "string", "cypress", '"cypress",'),
        ("items[1]", "number", "5", "      5,"),
        ("items[2]", "boolean", "false", "      false,"),
        ('items[4]["a.b"]', "string", "cedar", '{ "a.b" = "cedar" }'),
        ("object.plain", "string", "ash", 'plain = "ash"'),
        ('object["dash-key"]', "string", "elm", '"dash-key" = "elm"'),
        ('object["region/east"]', "string", "fir", '"region/east" = "fir"'),
        ('object["quote\\\"back\\\\slash"]', "string", "hazel", ' = "hazel"'),
        ('object["snow☃"]', "string", "holly", '"snow☃" = "holly"'),
    ]:
        row(p, "locals.proof." + key, kind, value, anchor)
    for key, anchor, reason in [
        ("nothing", "nothing = null", "null has no config row"),
        ("items[3]", "      null,", "null list item has no config row"),
        ("empty_list", "empty_list = []", "empty collection has no scalar leaf"),
        ("empty_object", "empty_object = {}", "empty collection has no scalar leaf"),
    ]:
        absent(p, "locals.proof." + key, anchor, reason)

    source("facets.tf", '''resource "neutral_widget" "proof" {
  name = "native-neutral"
  sample {
    note = "first-panel"
    flag = true
  }
  sample {
    note = "second-panel"
    amount = 8
  }
  single {
    note = "only-panel"
  }
  panel "with.dot" {
    note = "first-labeled"
  }
  panel "with.dot" {
    note = "second-labeled"
  }
  panel "elsewhere" {
    note = "single-labeled"
  }
}
''')
    p = "facets.tf"
    for key, kind, value, anchor in [
        ("name", "string", "native-neutral", 'name = "native-neutral"'),
        ("sample[0].note", "string", "first-panel", 'note = "first-panel"'),
        ("sample[0].flag", "boolean", "true", "flag = true"),
        ("sample[1].note", "string", "second-panel", 'note = "second-panel"'),
        ("sample[1].amount", "number", "8", "amount = 8"),
        ("single.note", "string", "only-panel", 'note = "only-panel"'),
        ('panel["with.dot"][0].note', "string", "first-labeled", 'note = "first-labeled"'),
        ('panel["with.dot"][1].note', "string", "second-labeled", 'note = "second-labeled"'),
        ("panel.elsewhere.note", "string", "single-labeled", 'note = "single-labeled"'),
    ]:
        row(p, "resource.neutral_widget.proof." + key, kind, value, anchor)

    source("textiles.tf", '''locals {
  proof = {
    literal = <<PLAIN
root-line
  kept-spaces
PLAIN
    indented = <<-INDENT
      first-thread
        second-thread
      third-thread
    INDENT
    escaped_markers = "$${proof} %%{ proof }"
    interpolation = "prefix-${var.proof}-suffix"
    sole_interpolation = "${var.proof}"
    directive = "%{ if var.proof }yes%{ else }no%{ endif }"
    literal_heredoc_backslash = <<BACKSLASH
\\n is not a heredoc newline escape
BACKSLASH
  }
}
''')
    p = "textiles.tf"
    for key, kind, value, anchor in [
        ("literal", "string", "root-line\n  kept-spaces\n", "literal = <<PLAIN"),
        ("indented", "string", "first-thread\n  second-thread\nthird-thread\n", "indented = <<-INDENT"),
        ("escaped_markers", "string", "${proof} %{ proof }", "escaped_markers ="),
        ("interpolation", "template", '"prefix-${var.proof}-suffix"', "interpolation = \"prefix"),
        ("sole_interpolation", "template", '"${var.proof}"', "sole_interpolation ="),
        ("directive", "template", '"%{ if var.proof }yes%{ else }no%{ endif }"', "directive ="),
        ("literal_heredoc_backslash", "string", "\\n is not a heredoc newline escape\n", "literal_heredoc_backslash ="),
    ]:
        row(p, "locals.proof." + key, kind, value, anchor)

    exact = "A" * 1024
    before_scalar = "é" * 511 + "🙂" + "TAIL"
    at_scalar = "é" * 510 + "🙂" + "Z"
    source("boundary.tf", 'locals {\n  proof = {\n' + f'    exact_bytes = "{exact}"\n    before_scalar = "{before_scalar}"\n    at_scalar = "{at_scalar}"\n' + '  }\n}\n')
    for key, value in [("exact_bytes", exact), ("before_scalar", before_scalar), ("at_scalar", at_scalar)]:
        row("boundary.tf", "locals.proof." + key, "string", value, key + " =")

    source("sensory/variables.tf", '''variable "proof" {
  default = {
    plain = "native-secret-saffron"
    nested = ["native-secret-walnut", null]
    quantity = 7319
    enabled = true
    opaque = upper("native-secret-call")
    template = "native-secret-${var.proof}"
  }
  sensitive = true 
  description = "source-authored sensitivity control"
}
''')
    p = "sensory/variables.tf"
    for key, kind, value, anchor in [
        ("plain", "string", "native-secret-saffron", "plain ="),
        ("nested[0]", "string", "native-secret-walnut", "nested ="),
        ("quantity", "number", "7319", "quantity ="),
        ("enabled", "boolean", "true", "enabled ="),
        ("opaque", "expression", 'upper("native-secret-call")', "opaque ="),
        ("template", "template", '"native-secret-${var.proof}"', "template ="),
    ]:
        row(p, "variable.proof.default." + key, kind, value, anchor, group="sensitive_default")
    row(p, "variable.proof.sensitive", "boolean", "true", "sensitive = true")
    row(p, "variable.proof.description", "string", "source-authored sensitivity control", "description =")
    absent(p, "variable.proof.default.nested[1]", "nested =", "null has no config row, even under redaction")

    source("visible/variables.tf", '''variable "proof" {
  default = {
    plain = "public-birch"
    enabled = false
    gap = null
  }
  sensitive = false
}
''')
    p = "visible/variables.tf"
    row(p, "variable.proof.default.plain", "string", "public-birch", "plain =")
    row(p, "variable.proof.default.enabled", "boolean", "false", "enabled =")
    row(p, "variable.proof.sensitive", "boolean", "false", "sensitive =")
    absent(p, "variable.proof.default.gap", "gap = null", "null has no config row")

    source("visible/terraform.tfvars", '''proof = {
  plain = "tfvars-secret-lavender"
  list = [55, true, null]
  "with.dot" = "tfvars-secret-camellia"
}
''')
    p = "visible/terraform.tfvars"
    for key, kind, value, anchor in [
        ("proof.plain", "string", "tfvars-secret-lavender", "plain ="),
        ("proof.list[0]", "number", "55", "list ="),
        ("proof.list[1]", "boolean", "true", "list ="),
        ('proof["with.dot"]', "string", "tfvars-secret-camellia", '"with.dot" ='),
    ]:
        row(p, key, kind, value, anchor, group="sensitive_tfvars")
    absent(p, "proof.list[2]", "list =", "null has no config row, even under redaction")

    source("visible/petal.auto.tfvars", 'proof = "auto-secret-marigold"\n')
    row("visible/petal.auto.tfvars", "proof", "string", "auto-secret-marigold", "proof =", group="sensitive_tfvars")

    source("opaque/incomplete.tf", 'locals {\n  proof =\n')
    absent("opaque/incomplete.tf", "locals.proof", "proof =", "whole file is syntactically unreadable and yields no rows")
    source("opaque/child/terraform.tfvars", '''proof = {
  plain = "ancestor-secret-verbena"
  nested = [91, false]
  absent = null
}
''')
    p = "opaque/child/terraform.tfvars"
    for key, kind, value, anchor in [
        ("proof.plain", "string", "ancestor-secret-verbena", "plain ="),
        ("proof.nested[0]", "number", "91", "nested ="),
        ("proof.nested[1]", "boolean", "false", "nested ="),
    ]:
        row(p, key, kind, value, anchor, group="unreadable_ancestor")
    absent(p, "proof.absent", "absent =", "null has no config row, even under redaction")

    at_depth = "depth-accepted-" + "[" * 63
    over_depth = "depth-refused-" + "[" * 64
    source("depth/at.tf", f'locals {{\n  proof = "{at_depth}"\n}}\n')
    row("depth/at.tf", "locals.proof", "string", at_depth, "proof =")
    source("depth/over.tf", f'locals {{\n  proof = "{over_depth}"\n}}\n')
    absent("depth/over.tf", "locals.proof", "proof =", "65 simultaneously open brackets including strings is one past the 64-bracket ceiling; whole file yields no rows")

    source("shadow/fern.tf", '''variable "proof" {
  default = "shadow-secret-juniper"
  sensitive = true
}
locals {
  proof = "shadowed-teak"
}
''')
    for key, anchor in [("variable.proof.default", "default ="), ("variable.proof.sensitive", "sensitive ="), ("locals.proof", 'proof = "shadowed')]:
        absent("shadow/fern.tf", key, anchor, "same-directory fern.tofu shadows the entire fern.tf, so neither rows nor counts nor sensitivity markers come from it")
    source("shadow/fern.tofu", '''locals {
  proof = {
    winner = "active-elm"
  }
}
''')
    row("shadow/fern.tofu", "locals.proof.winner", "string", "active-elm", "winner =")

    source("meada.toml", '''# Declared targets make direct key context part of the #641 public contract.
[[resource]]
kind = "native_fixture"
id = "botanical"
role = "write"
via = ["botany.tf#locals.proof.ranked_exact"]

[[resource]]
kind = "native_fixture"
id = "masked"
role = "write"
via = ["sensory/variables.tf#variable.proof.default.plain"]

[[resource]]
kind = "native_fixture"
id = "edge"
role = "write"
via = ["boundary.tf#locals.proof.before_scalar"]
''')

    assert len([p for p in SOURCES if p != "meada.toml"]) == 14
    assert sum(len(t.encode("utf-8")) for t in SOURCES.values()) <= 32768
    assert len({r["id"] for r in ROWS}) == len(ROWS)
    baseline = materialize(ROWS, "baseline")
    changed = materialize(ROWS, "literal_changed")
    public = materialize(ROWS, "sensitivity_public")
    phases = {"baseline": baseline, "literal_changed": changed, "sensitivity_public": public, "restored": baseline}

    queries = []
    query_keys = []

    def search(needle, budget=200000, kind="value", purpose="source-selected scalar query"):
        args = {"query": needle, "budget": budget}
        if kind is not None:
            args["kind"] = kind
        index = len(queries)
        queries.append({"tool": "search", "arguments": args})
        entry = {"query_index": index, "purpose": purpose, "tool": "search", "query": needle, "budget": budget}
        if kind == "value" or kind is None:
            entry["phase_results"] = {phase: selected(rows, needle) for phase, rows in phases.items()}
            if budget < 200000:
                entry["budget_expectation"] = {
                    "must_cut": True,
                    "complete_result_count": len(selected(baseline, needle)),
                    "rows_must_be_a_strict_prefix_of_high_budget_result": True,
                    "must_show_documented_cut_footer_with_total_count": True,
                    "exact_retained_count": 0,
                    "expected_footer": "… cut after 0 of 72 lines (budget 1 tokens); raise budget for more",
                    "reason": "The first complete independently rendered row is far longer than four bytes. One token cannot retain a complete row under the documented four-byte estimate. Preserve the whole response including its footer.",
                }
        query_keys.append(entry)

    search("proof", purpose="complete independently enumerated row multiset and rank/path/line/key order")
    search("proof", budget=1, purpose="genuinely cutting exhaustive query with source-derived total and retained-prefix expectations")
    search("MossNeedle", purpose="one exact, one prefix, one substring in that order, case insensitive")
    search("MOSSNEEDLE", purpose="case-fold equivalence of exact/prefix/substring control")
    search("qz", purpose="under-three-character scan path, one exact/prefix/substring")
    search("maples", purpose="same-size literal is present at baseline and absent after edit")
    search("willow", purpose="same-size literal is absent at baseline and present after edit")
    search("proof", kind=None, purpose="default search falls back to exactly the complete value result when no symbol or contract matches proof")
    for needle in [
        "native-secret-saffron", "native-secret-walnut", "native-secret-call", "native-secret-", "7319",
        "tfvars-secret-lavender", "tfvars-secret-camellia", "auto-secret-marigold", "ancestor-secret-verbena",
        "shadow-secret-juniper", "shadowed-teak", "depth-refused-", "TAIL",
        "locals.proof.object", "resource.neutral_widget.proof.sample", 'proof["with.dot"]',
        "locals.proof.nothing", "locals.proof.empty_list", "locals.proof.empty_object",
        "active-elm", "source-authored sensitivity control", "public-birch", "locals.proof.before_scalar",
    ]:
        search(needle)

    def context(symbol, purpose, rows=None, target=None):
        index = len(queries)
        queries.append({"tool": "context", "arguments": {"symbol": symbol, "budget": 200000}})
        key = {"query_index": index, "tool": "context", "purpose": purpose, "symbol": symbol, "preserve_complete_answer": True}
        if rows:
            key["complete_block_value_rows_by_phase"] = {
                phase: [row for row in entries if row["file"] == rows[0] and row["key"].startswith(rows[1])]
                for phase, entries in phases.items()
            }
        if target:
            key["declared_target"] = target
            key["must_not_repeat_scalar_payload"] = True
            row_id = target["file"] + "#" + target["key"]
            key["source_target_metadata_by_phase"] = {
                phase: {field: entry[field] for field in ("file", "key", "line", "type", "cut", "redacted")}
                for phase, entries in phases.items()
                for entry in entries if entry["id"] == row_id
            }
        query_keys.append(key)

    context("facets.tf::neutral_widget.proof", "one unknown-type IaC block exposes all of its independently authored value rows", rows=("facets.tf", "resource.neutral_widget.proof."))
    for file, key, contract in [
        ("botany.tf", "locals.proof.ranked_exact", "resource::native_fixture::botanical"),
        ("sensory/variables.tf", "variable.proof.default.plain", "resource::native_fixture::masked"),
        ("boundary.tf", "locals.proof.before_scalar", "resource::native_fixture::edge"),
    ]:
        context(file + "#" + key, "direct declared configuration-key context, full target metadata and payload privacy", target={"file": file, "key": key, "line": next(r["line"] for r in baseline if r["id"] == file + "#" + key), "contract": contract, "role": "write", "level": "declared"})
        context(contract, "declared resource context reaches exactly its source key without repeating scalar contents", target={"file": file, "key": key, "contract": contract, "role": "write", "level": "declared"})

    assert all(set(query) == {"tool", "arguments"} for query in queries)
    (BASE / "queries.jsonl").write_text("".join(json.dumps(q, ensure_ascii=False) + "\n" for q in queries))

    same_size = []
    for phase, file, before, after in [
        ("literal_changed", "botany.tf", 'refresh = "maples"', 'refresh = "willow"'),
        ("sensitivity_public", "sensory/variables.tf", "sensitive = true ", "sensitive = false"),
    ]:
        original = SOURCES[file].encode("utf-8")
        assert len(before.encode()) == len(after.encode())
        assert original.count(before.encode()) == 1
        replacement = original.replace(before.encode(), after.encode())
        same_size.append({
            "phase": phase, "starts_from": "baseline", "file": file,
            "replace_exactly_once": {"before": before, "after": after},
            "bytes_before": len(original), "bytes_after": len(replacement),
            "sha256_before": hashlib.sha256(original).hexdigest(), "sha256_after": hashlib.sha256(replacement).hexdigest(),
            "preserve_original_mtime_ns_for_warm_refresh": True,
            "query_file": "queries.jsonl",
            "warm_and_cold_must_equal_complete_phase_expectations": True,
            "restore_exact_original_bytes_and_mtime_after_phase": True,
            "restored_full_query_status_export_must_equal_baseline": True,
        })

    active = [p for p in SOURCES if p.endswith((".tf", ".tofu", ".tfvars")) and p != "shadow/fern.tf"]
    read_files = [p for p in active if p not in ("depth/over.tf", "opaque/incomplete.tf")]
    files = []
    for path in sorted(SOURCES):
        if path == "meada.toml":
            outcome = "manifest_not_a_config_value_file"
        elif path == "shadow/fern.tf":
            outcome = "shadowed_not_read_or_counted"
        elif path in ("depth/over.tf", "opaque/incomplete.tf"):
            outcome = "unreadable"
        else:
            outcome = "read"
        files.append({
            "file": path, "bytes": len(SOURCES[path].encode()), "outcome": outcome,
            "row_ids": [r["id"] for r in baseline if r["file"] == path],
            "baseline_redacted_rows": sum(r["redacted"] for r in baseline if r["file"] == path),
        })

    secrets = [
        "native-secret-saffron", "native-secret-walnut", "native-secret-call", "native-secret-", "7319",
        "tfvars-secret-lavender", "tfvars-secret-camellia", "auto-secret-marigold",
    ]
    always_absent = ["ancestor-secret-verbena", "shadow-secret-juniper", "shadowed-teak", "depth-refused-", "TAIL"]
    key = {
        "schema": "independent-native-hcl-source-key-v1",
        "status": "SOURCE_KEY_FROZEN_NO_PRODUCT_EXECUTION",
        "root_source": ROOT_SOURCE,
        "independence": "Authored from issues 639/640/641/642, ADR 0025, documented ceilings and pinned official HCL/OpenTofu format rules before any candidate answer. No Meada runtime, database, implementation table, feature fixture, Terraform CLI or evaluator supplies these rows.",
        "origin": ORIGIN, "repository_identity": IDENTITY, "common_query_component": "proof",
        "limits": {"hcl_or_var_source_files": 14, "source_bytes_including_manifest": sum(len(t.encode()) for t in SOURCES.values()), "max_source_bytes": 32768, "value_bytes": VALUE_BYTES, "nesting_open_brackets": 64},
        "row_fields": {"value": "stored text, or null exactly when redacted", "shown_value": "complete rendered RHS including cut/redacted marker", "source_lexeme": "optional source note; null is not an expected scalar null", "line": "1-based start line of the scalar/expression, with heredoc at its introducer", "type": "format-derived native kind; compare any public type metadata without inventing a type from runtime output"},
        "files": files,
        "rows": baseline,
        "expected_absences": ABSENT,
        "complete_exhaustive_order": selected(baseline, "proof"),
        "phase_rows": {"literal_changed": changed, "sensitivity_public": public},
        "phase_status": {phase: status(entries) for phase, entries in phases.items()},
        "phase_complete_exhaustive_order": {phase: selected(entries, "proof") for phase, entries in phases.items()},
        "query_expectations": query_keys,
        "source_edits": same_size,
        "privacy": {
            "baseline_and_literal_changed_forbidden_substrings": secrets + always_absent,
            "sensitivity_public_forbidden_substrings": always_absent,
            "restored_forbidden_substrings": secrets + always_absent,
            "scope": "raw CLI stdout/stderr, tool schemas and complete response payloads within whole MCP records for stdio and HTTP, context answers, exports and diagnostic/decision payloads produced by this capture; separately recorded request arguments and input fixture/key files are not response leaks",
            "export_no_scalar_payloads_any_phase": True,
            "export_always_forbidden_substrings": secrets + always_absent + ["MossNeedle", "native-neutral", "public-birch", "source-authored sensitivity control", "active-elm", "maples", "willow", "cypress"],
            "declared_target_context_no_scalar_payloads": ["MossNeedle", "native-secret-saffron", "é" * 20],
            "do_not_search_request_arguments_as_if_they_were_secret_response_leaks": True,
            "never_read_or_decode_process_environment": True,
        },
        "export_expectations": {
            "config_value_rows": 0,
            "declared_contracts": ["resource::native_fixture::botanical", "resource::native_fixture::edge", "resource::native_fixture::masked"],
            "declared_role": "write", "links": [], "provisioners": [],
            "automatic_contracts_from_neutral_source": [],
            "preserve_complete_export_and_judge_unexpected_fields_or_rows": True,
        },
        "status_source_inventory": {
            "active_native_config_files": active, "read_native_config_files": read_files,
            "unreadable_native_files": ["depth/over.tf", "opaque/incomplete.tf"],
            "native_definition_files_excluding_var_files": [p for p in active if not p.endswith(".tfvars")],
            "var_files": [p for p in active if p.endswith(".tfvars")],
            "iac_nodes": [{"address": "neutral_widget.proof", "type": "neutral_widget", "file": "facets.tf", "root": ".", "line": 1, "end_line": 23}],
            "declared_resource_sides": 3, "declared_targets_stale": 0,
            "unsettled_iac_status_counter": "The read/skipped config counters and values are exact above. The normative material does not specify whether the separate IaC file counter counts the 3 var files as IaC inputs. The exact inventories are supplied; preserve the entire status and report this field UNPROVEN until an independent accepted counter definition is supplied. Do not bless a counter from candidate output.",
        },
        "comparison": {
            "search": "Require the entire high-budget row multiset and exact listed order, every file/key/line/value/cut/redacted field, and no unexpected row. Full result counts must equal the independent key. Do not filter surprises into success.",
            "kind_visibility": "Expected native kinds are complete. Search's public rendering lacks an explicit kind column; retain the complete direct-key metadata and record any kind that cannot be observed as UNPROVEN rather than claiming storage introspection.",
            "transports": "For each phase and each query use identical source, index, repository scope and query arguments; HTTP must copy the full stdio answer. Preserve both records, tool schemas, diagnostics and cleanup.",
            "freshness": "Each same-size edit starts from baseline. Compare warm refresh with original size and mtime to a cold index of exactly the changed bytes, then restore all source hashes and query/status/export equivalence. Never change the answer key from a runtime result.",
            "unknown_payload_fields": "Preserve complete record/status/export/context/decision payloads. A new/unexpected field is review evidence, not discarded data. Independent runtime judgment is separate and may be BROKEN, BLOCKED or UNPROVEN.",
        },
        "not_claimed": ["generic Terraform evaluator correctness", "Terraform node-name or provisioner acceptance", "SSM SecureString matrix (reuse its dedicated source receipt)", "complete generic IaC field key", "a runtime verdict"],
    }
    if (REPO / ".git").is_dir():
        def git(*args):
            return subprocess.check_output(["git", "-C", str(REPO), *args], text=True).strip()
        key["fixture_git"] = {
            "commit": git("rev-parse", "HEAD"),
            "tree": git("rev-parse", "HEAD^{tree}"),
            "branch": git("branch", "--show-current"),
            "origin": git("remote", "get-url", "origin"),
            "author_name": "Native HCL source key",
            "author_email": "proof@example.com",
            "author_and_committer_date": "2026-11-07T00:00:00+00:00",
            "commit_message": "test: add independent native HCL source fixture",
        }
    save(BASE / "expected-values.json", key)
    print(json.dumps({"files": len(SOURCES), "hcl_or_var_files": 14, "bytes": key["limits"]["source_bytes_including_manifest"], "rows": len(baseline), "redacted": sum(r["redacted"] for r in baseline), "queries": len(queries), "phase_status": key["phase_status"]}, indent=2))


if __name__ == "__main__":
    main()
