# Stability

This document says what you can build on in the 1.x series and when a change to Meada needs a major version. It applies from 1.0: the 0.x series carries no stability promise, and any item here may change between 0.x releases.

Meada follows [semantic versioning](https://semver.org/). In 1.x, a change that breaks a frozen item ships only in a major version (2.0). An additive change ships in a minor version (1.x).

This document describes the default build (`cargo install meada`). The [cloud modules](cloud.md) exist only in builds with their feature and only add to the surface; none of them changes what is listed here.

## What is frozen

### MCP tools

Three tools, always with these names. None of them writes.

| Tool | Parameter | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `search` | `query` | string, required | | Name to look for, case-insensitive. |
| `search` | `kind` | string, optional | all | Restricts to one kind (list below). |
| `search` | `budget` | unsigned 32-bit integer, optional | 2,000 | Token budget of the answer. |
| `context` | `symbol` | string, required | | Symbol name, `<path>::<name>` or full identity; a resource (`resource::<kind>::<id>`) or a channel (`channel::<broker>::<name>`). |
| `context` | `budget` | unsigned 32-bit integer, optional | 2,000 | Token budget of the answer. |
| `trace` | `from` | string, optional | | Start: a symbol, a route (`GET /users/{}` or its contract), a call site or a process (`process::<entry point identity>`, `down` only). |
| `trace` | `changes` | string, optional | | `uncommitted`: start from the symbols the working-tree diff touches. It is the only accepted value. |
| `trace` | `direction` | string, optional | `down` | `down` (what it reaches) or `up` (what reaches it). |
| `trace` | `budget` | unsigned 32-bit integer, optional | 2,000 | Token budget of the answer. |

- `trace` takes exactly one of `from` and `changes`.
- The default budget of 2,000 tokens is part of the surface. A token is an estimate: one token per four bytes of UTF-8 output.
- The `search` kinds: `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`, `drift`, `community`, `process`, `duplicate`, `ambiguous` and `unresolved`.

### Output grammar

The output of the three tools is line-oriented text, and its grammar is specified in the [output grammar reference document](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md) (in English, like the MCP surface, [ADR 0008](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0008-mcp-surface-and-output-languages.md)). Frozen are:

- every line kind of each tool and the meaning of each of its fields;
- the link notation `→[level: reason]`, with the levels `exact`, `declared`, `inferred`, `decided` and `ambiguous`;
- contract identifiers, each starting with its contract kind: `http::`, `channel::`, `package::`, `resource::`;
- the footer of `trace` (`chain: <weakest level> · <n> hops · <n> repos · <n> tokens`);
- the note that ends an answer cut by the budget;
- the token estimate.

A reader of the output skips the line kinds and fields it does not know: that is how a minor version adds something without breaking anyone.

### Command line

Commands and flags of the default build:

| Command | Flags |
| --- | --- |
| `meada index` | `--workspace <manifest>` |
| `meada status` | `--workspace <manifest>` |
| `meada export` | `--workspace <manifest>`, `--repo <identity>` |
| `meada mcp` | `--listen <address>`, `--workspace <manifest>` |
| `meada decide` | `--item <id>`, `--dry-run`, `--list`, `--prune`, `--decider`, `--show-request`, `--min-p <p>` |

`meada help`, `--help` and `--version` (`-h`, `-V`) are also covered.

- `--workspace`, `--repo` and `--listen` belong to [server mode](server.md). On `meada export`, `--workspace` and `--repo` go together; on `meada mcp`, `--listen` and `--workspace` go together, and `--listen` serves MCP over streamable HTTP, one endpoint per repository identity.
- `meada decide`: `--prune` does not combine with `--item`, `--dry-run` or `--list`; `--show-request` and `--min-p` apply only with `--decider`; `--min-p` ranges from 0 to 1 and defaults to 0.5.
- The decider's environment variables: `MEADA_DECIDER_PROTOCOL` (`decisions` or `chat`, default `decisions`); `MEADA_DECIDER_URL` (default `https://openrouter.ai` with `decisions`, required with `chat`); `MEADA_DECIDER_KEY` (required with `decisions`, optional with `chat`); `MEADA_DECIDER_MODEL` (default `typesafe/jev-1.14` with `decisions`, required with `chat`). See [Decisions](decisions.md).

### `meada.toml`

Every table and key. An unknown key is an error naming the file, the line and the key.

| Table | Key | Type | Note |
| --- | --- | --- | --- |
| `[[peer]]` | `repo` | string, required | The peer repository's identity. |
| `[[peer]]` | `path` | string, optional | A hint of where the clone is, relative to the root. |
| `[[peer]]` | `base_url_env` | list of strings, optional | Variables this repository reads the peer's base URL from. |
| `[serve]` | `hosts` | list of strings, optional | Hosts the repository answers on. |
| `[serve]` | `prefix` | string, optional | What the gateway adds in front of paths. |
| `[serve]` | `strip` | string, optional | What the gateway removes from the start of paths. |
| `[serve]` | `documents` | list of strings, optional | OpenAPI or AsyncAPI documents besides those found by file name, relative to the root. |
| `[[resource]]` | `kind` | string, required | Lowercase identifier (letters, digits and hyphens, starting with a letter). |
| `[[resource]]` | `id` | string, required | Not empty. |
| `[[resource]]` | `role` | string, required | `read`, `write` or `read-write`. |
| `[[resource]]` | `via` | list of strings, optional | The symbols that do the access, as `<path>::<qualified name>`. |

See the [configuration](configuration.md).

### `meada.decisions.toml`

One table, `[[decision]]`, with these keys. An unknown key is an error.

| Key | Type | Note |
| --- | --- | --- |
| `kind` | string, required | `ambiguous-link` or `dynamic-url`. |
| `call` | string, required | The item's call site, as `meada export` names it. |
| `target` | string, required | `<provider repository identity>::<contract identifier>`. |
| `by` | string, required | `human` or the model id. |
| `p` | number, from 0 to 1 | Required when `by` is a model; forbidden when it is `human`. |

Each `call` appears in at most one decision; repeating it is an error.

### `meada.workspace.toml`

The workspace manifest of [server mode](server.md), read by the default build:

| Table | Key | Type | Note |
| --- | --- | --- | --- |
| (root) | `index_dir` | string, required | Where the indexes live, `<index_dir>/<identity>/index.db`. |
| `[workspace]` | `source` | string, optional | `file` (default) or `store`; `store` needs a build with the `generation-pointer` feature, and the default build refuses it with an error. |
| `[[repository]]` | `repo` | string, required | The repository's identity. |
| `[[repository]]` | `path` | string, required | Where the mirror is. |
| `[[repository]]` | `owner` | string, optional | The OIDC group that answers for the repository; entry metadata. |
| `[[repository]]` | `added_by` | string, optional | The GitHub login that added the entry; metadata. |
| `[repository.serve]` | `hosts`, `prefix`, `strip`, `documents` | | As `[serve]` in `meada.toml`. |
| `[[repository.resource]]` | `kind`, `id`, `role`, `via` | | As `[[resource]]` in `meada.toml`. |
| `[[repository.peer]]` | `repo`, `base_url_env` | | As `[[peer]]`, without `path`. |

The sections of the cloud modules (`[valkey]`, `[snapshot]`, `[oidc]` and the rest) exist only in builds with their feature.

### The `meada export` format

A repository's export is indented JSON ending in a newline, with every list sorted: the same files give identical bytes. An empty top-level list is left out of the document, except `files`, `symbols`, `calls`, `providers`, `consumers` and `links`, which always appear; nested lists (`members`, `steps`, `contracts`, `fields`) are always written. The document carries no format version field: the format grows only by additions, and a reader of the export ignores the fields it does not know.

| Field | Content |
| --- | --- |
| `repository` | The repository's identity. |
| `files` | `path`, `hash`. |
| `symbols` | `id`, `span` (`path`, `start`, `end`), `hash`. |
| `calls` | `id`, `caller`, `target`, `level`, `candidates`, `span`, `narrowed`, `decorates`. |
| `providers` | `contract`, `site`, `gateway`, `shape`. |
| `consumers` | `contract`, `site`, `via`, `shape`. |
| `bindings` | `contract`, `exchange`, `pattern`, `site`. |
| `documents` | `path`, `format` (`openapi` or `asyncapi`), `version`, `contracts`. |
| `publishes` | `exchange`, `key`, `site`. |
| `packages` | `name`, `version`, `repository`, `directory`, `ecosystem`. |
| `dependencies` | `manifest`, `field`, `name`, `alias`, `range`. |
| `imports` | `site`, `specifier`, `name`, `level`, `how`, `target`. |
| `communities` | `name`, `members`. |
| `links` | `consumer`, `provider`, `contract`, `level`, `reason`, `by`, `p`, `drift` (`missing`, `unchecked`). |
| `processes` | `entry_point`, `kind` (`route`, `export`, `channel` or `program`), `label`, `steps`. |
| `stale_decisions` | `call`, `target`, `cause`. |

The fields of `shape` (`source`, `fields`, `open`, `scalars`, `declared`, `optional`, `guessed`) and of each step in `steps` (`depth`, `repository`, `level`, `via`, `node`, `reason`, `mark`, `candidates`, `contract`) are part of the format, and so are these values:

- `level`: `exact`, `declared`, `inferred`, `decided` and `ambiguous`;
- `shape.source`: an object with one key, `document`, `zod`, `literals` or `reads`, whose value is the file path;
- `drift.missing`: each path with its level; `drift.unchecked`: each path with the open path that covers it;
- `imports[].how`: `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath`, `package` and `namespace`;
- `steps[].via`: `entry`, `call` and `link`;
- `steps[].mark`: `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes` and `depth-limit`;
- `stale_decisions[].cause`: `call-gone`, `item-not-open`, `target-gone` and `not-candidate`.

## What counts as breaking

After 1.0, a break in any frozen item needs a major version. Breaking means:

- removing or renaming an item (tool, parameter, command, flag, table, key, export field);
- changing a parameter's type, default or meaning, including the default budget of 2,000 tokens;
- changing the grammar or meaning of an existing line kind, field or key;
- turning valid configuration invalid.

## What counts as a minor change

These changes ship in a minor version:

- adding an optional parameter;
- adding a `search` kind;
- adding a line kind to the output;
- adding an export field;
- adding a configuration key;
- adding a contract kind;
- adding a language pack.

[Server mode](server.md) is an example: `--workspace`, `--repo` and `--listen`, `meada.workspace.toml` and the `index behind the mirror: run meada index --workspace` line of `context` only add to the surface, and nothing that existed changes because of them.

## What is not frozen

- The wording of command-line messages, in any language.
- The text of the MCP tool descriptions, which stays within the 1,000-token budget of the tool list.
- The index file and the layout of `.meada/`, rebuilt when the schema changes ([ADR 0004](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0004-index-storage-and-freshness.md)).
- Which links Meada finds for a repository. Improving resolution is a fix, not a break: a new link, a stronger level or one false positive fewer may arrive in any release.

## The review gate

Every change to the frozen surface shows up as a diff in review, through two mechanisms:

- the **surface goldens**: the MCP tool list (names, parameter schemas and descriptions, with the check of the 1,000-token limit) and the English help of every CLI command;
- the **grammar check**: the goldens of rendered answers are checked against the reference grammar, so every line kind they contain is documented.

Whoever opens a PR that changes one of those goldens, a configuration schema or the export format says in the PR whether the change is breaking or minor, by the lists above. A breaking change lands only in the next major release.
