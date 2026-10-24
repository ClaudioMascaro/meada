# Stability in 1.x

This tree declares **0.1.0** and prepares 1.0. The 0.x series carries no stability promise.
The policy below takes effect **when 1.0 is published** and covers 1.x releases;
this document does not announce a release or claim that the acceptance gates have passed.

## Frozen surface

The inventories linked below are part of this policy. They record the existing surface,
with its types, defaults, validation and meanings.

| Surface | What is frozen in 1.x | Complete inventory |
| --- | --- | --- |
| MCP | The names `search`, `context` and `trace`, the parameter names/types/defaults below and the output grammar. | [Agents and MCP](mcp.md) and the [normative output grammar](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md). |
| CLI | Commands, subcommands and flags, including those available only when the corresponding feature is compiled. Includes local commands, `snapshot`, `github-app`, `ui`, `workspace`, help and version. | [Commands](cli.md), with arguments, defaults and restrictions. |
| Local `meada.toml` | `[[peer]]`: `repo`, `path`, `base_url_env`; `[serve]`: `hosts`, `prefix`, `strip`, `documents`; `[[resource]]`: `kind`, `id`, `role`, `via`. | [Configuration](configuration.md), every table and key with its type, default and validation. |
| `meada.decisions.toml` | `[[decision]]`: `kind`, `call`, `target`, `by`, `p`, including unique `call` values and the conditional rules for `p`. | [Decisions](decisions.md), with accepted types and values. |
| `meada export` | Fields, types and meanings of the canonical JSON, including nested records, enums, `null`, omissions and ordering. | [JSON export](export.md). |

### MCP parameters

| Tool | Parameter | JSON type | Default and meaning |
| --- | --- | --- | --- |
| `search` | `query` | required string | No default; name to look for, case-insensitive. |
| `search` | `kind` | optional string or `null` | Absent or `null`: symbols and provider/consumer contracts. A string selects one of the search kinds in the [MCP inventory](mcp.md). |
| `context` | `symbol` | required string | No default; symbol or resource/channel identity to describe. |
| `trace` | `from` | optional string or `null` | Absent or `null`: does not select an identity as the start. Accepts a symbol, route, call site or `process::<entry point identity>`. |
| `trace` | `changes` | optional string or `null` | Absent or `null`: does not select the diff. Accepts only `uncommitted`. |
| `trace` | `direction` | optional string or `null` | Absent or `null`: `down`; also accepts `up`. Processes accept only `down`. |
| All | `budget` | optional integer from 0 to 4,294,967,295 or `null` | Absent or `null`: **2,000 tokens** per answer. |

`trace` requires exactly one of `from` and `changes` to have a non-null value. Server mode
refuses `changes: uncommitted`. The defaults above are effective behavior, including when
the client omits the property or sends `null`.

The normative grammar is written in English, like the MCP surface. It specifies line kinds,
contract identifiers, levels/reasons, the footer, cut marker and token estimate: one token
per four UTF-8 bytes, rounded up. Readers of 1.x must **skip line kinds and fields they do
not know**, including new fields on known line kinds. Export readers must also skip unknown
fields.

## When to change the version

After 1.0 is published, a breaking change to any frozen item requires a **major** version,
such as 2.0. A breaking change means:

- removing or renaming an item;
- changing a parameter's type, default or meaning;
- changing the grammar or meaning of an existing line kind, field or key;
- turning valid configuration invalid.

The following are **minor** changes, such as 1.1: adding an optional parameter, a `search`
kind, a line kind, an export field, a configuration key, a contract kind or a language pack.
Additions preserve existing items according to the rules above.

## What is not frozen

- CLI message wording in any language.
- MCP tool description text, still within the **1,000-token** tool-list budget (descriptions
  and parameter schemas).
- The index file and the layout of `.meada/`; a schema change may rebuild the index.
- Which links Meada finds for a given code base. Improving resolution is a fix, not a break
  in the stability promise.

## Surface review gate

Before approving a change, compare it with the inventories and review the diffs of these
[golden files](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden):

- [`surface.mcp-tools.json`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.mcp-tools.json): names, schemas and descriptions of the three tools. [`tests/it/mcp.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/mcp.rs) also checks the 1,000-token ceiling and the effective default of 2,000 per tool.
- [`surface.cli.default.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.default.en.txt) and [`surface.cli.cloud.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.cloud.en.txt): English help for every command in the default and complete builds. [`tests/it/cli.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/cli.rs) walks the subcommands.
- [`tests/it/grammar.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/grammar.rs): checks the normative reference's inventories and validates every rendered-answer golden against the grammar.

These checks make a change visible for review; updating a golden does not make a breaking
change compatible with 1.x. Wording-only changes remain outside the freeze described above,
even when they appear in a golden diff.
