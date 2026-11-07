# Console infrastructure graph (#675)

Run from the checkout whose frontend you built:

```sh
python3 $S/console-graph.py --head-bin "$RUN/bin/meada-head-ui" --ui-dir ui/dist --run "$RUN/console-graph"
```

Use a new run directory. The binary needs only `ui`; no cloud services or sign-in are needed.
Build WASM and the frontend from this tree using the [console recipe](console.md). Set
`PROVE_NODE` to a directory with `playwright-core@1.61.1` and `axe-core@4.12.1` installed, and
optionally `PROVE_CHROMIUM` to an existing Chromium executable. Without it, Playwright uses its
installed Chromium. These tools remain separate from the product's Node dependencies.

The existing frontend CI job installs these pinned tools, runs the built console flow and
uploads `.prove-it/runs/ci-console-graph`. Follow the root `AGENTS.md` validation policy.
A CI script pass supplies evidence to the independent checker, not a PROVED
verdict. The PR Evidence names the reviewed receipt and CI artifact.

## Answer keys and methods

New role names, directions, provisioning roots, keyboard access and legend answer to #675,
parent #642 and ADR 0026 (rung 4, maintainer). The independent checker reads those specs at run
time and inspects the neutral source files. Complete API export bytes copy the CLI on exactly
the same manifest and repository (rung 1); the playground copies the captured console query
answer unchanged (rung 1). Existing graph behavior answers to the unchanged chain goldens and
the console viewer flow. No flow uses a missing answer key.

| Method | Surface | Answer key | Catches | Misses | Setup / run | Reachable |
|---|---|---|---|---|---|---|
| Whole CLI/API byte parity | console export API | 1: same-mode CLI | Lost export fields, wrong repository scope | A frontend dropping rows | ui binary / seconds | yes |
| Browser and native keyboard | graph and playground | 4: #675/#642; 1: captured API | Missing roles, reversed links, local alert links omitted, dropped roots/reasons, unavailable search kinds | Manual assistive-device use | Chromium / seconds | yes |
| Layout shift | graph/playground | existing maintainer budget: CLS <= 0.1 | Unstable initial page layout | Visual taste | same / seconds | yes |
| Load time | graph/playground | existing maintainer budget: <= 2 s on loopback | Page startup regression | WAN performance | same / seconds | yes |
| Accessibility | every loaded graph table | existing maintainer budget: zero serious/critical axe violations | Unnamed regions/controls, structural and contrast failures | Full screen-reader experience | axe / seconds | yes |
| Console errors | graph/playground | existing maintainer budget: zero errors | WASM/parser/runtime failures | Silent data loss, checked above | same / seconds | yes |

These extend the existing console methods and maintainer budgets, as authorized by #675's
console-proof requirement. They do not introduce an approval step or change any answer key.

## Inputs and checks

`console-graph.py` generates four neutral repositories inside the run directory, commits them,
indexes a server-mode manifest and starts a detached console on a free loopback port. It reads
no feature fixtures or mapping tables. Its inputs include two roots provisioning one queue,
a directionless config use, two image builders (one without a runner), two runners (one
without a builder), two alert emitters and equality/regex matchers inside one repository.

Before opening the page it retains binary hash/version, frontend version, instance URL,
config health response, complete same-mode CLI/API exports and successful `value`, `iac` and
`alert` query responses. The known nonempty queue/image/alert rows establish usability; empty
data is not a health success. `console-graph.mjs` then checks complete source-derived role/link
sets and drives native keyboard view selection, focusable named tables, contracts including
unpaired sides, provisioning roots and evidence, image builder → runner, alert emitter →
matcher, both exact and inferred levels, and every `value`/`iac`/`alert` selector through the API.

Break-it controls: a runner without a builder creates no link; a builder without a runner
remains visible; local alert links appear in Alert links while the generic cross-repository
view remains empty; loading a repository without image/alert sides removes those views and
falls back to communities only if the old view is unavailable, retaining existing views
otherwise. View switching is also exercised back to contracts and provisions.

`exports/`, `*.rows.json`, `*.txt`, `*.axe.json`, screenshots, `queries.json`, `budgets.json`,
`browser-errors.json` and process cleanup are evidence, retained even if a browser assertion
fails. Read screenshots and row captures before citing them. `checks.md` says only "checks
passed"; a checker writes `receipt.md` using the main skill's format and marks whether it was
independent. Preserve failed attempts and name any retry correction.

## Limits and cleanup

This flow proves the four neutral repositories on the read-only viewer console. It does not
claim signed-in grants, remote generations, fleet operations, all IaC extractors or all-corpus
backend parity; the existing console and A7 sections cover those when the diff reaches them.
The API parity and browser share the head backend, so the explicit source-derived counts and
roles are required in addition. A script written by the implementer is not an independent
answer key; the fresh checker judges it against the supplied specs.

The Python driver stops its own console in `finally`, writes `cleanup.json`, removes its pid
file and preserves inputs/indexes/evidence for inspection. Check that the recorded PID and
port stopped. Delete the run's `work/` and `indexes/` only after review if space is needed;
keep screenshots, raw captures and the receipt. Never touch a user's checkout or a shared
server. An unavailable binary, frontend, tool or browser is BLOCKED, with its error retained.
