# Decisions

Detail behind the [architecture map](../architecture.md), which keeps the flow, identities,
seams, invariants and Known ceilings. The code wins over this file, and a change to these
modules updates it.

- `decide.rs`: `meada decide`. `items(engine, only)` (`Engine::local_items`: the open items
  whose call lives in the repository where Meada runs, id order; with `--item`,
  `Engine::local_item`, which also takes a decided call, offered with every candidate so the
  new pick replaces its entry; an id that is neither is an error),
  `ask(items, input, out) -> Vec<Decision>` (human mode: prints each item and its numbered
  candidates, reads plain lines, `s` or empty skips, end of input ends), `finish(engine, picks,
  dry_run, out)` (one write at the end through `decisions::canonical` + `save`; nothing picked
  writes nothing), `list` (`--list`: open items, then the repository's stale and waiting
  decisions from `Engine::local_decisions`) and `prune` (`--prune`: `decisions::pruned` drops
  exactly the stale entries and `save` rewrites the file; waiting ones stay; no write when none
  is stale) and `run`, the public entry. Human mode reads no environment variable and
  opens no connection. With `--decider` (and neither `--list` nor `--prune`), `run` reads
  `decider::Config` first (and validates `--min-p` with `decider::min_p`, before the
  variables), then hands the items to `decider::run`, which returns an `Outcome {decisions,
  failed}`; the decisions go to `finish` and, when `failed > 0`, `run` then returns an error
  (`meada: N item(s) failed`, exit 1) so a script sees the failure while the other items are
  already written. `--show-request` stops after printing.
- `decisions.rs`: `meada.decisions.toml` (ADR 0006). `Decisions { items: Vec<Decision> }`,
  `Decision { kind (ambiguous-link | dynamic-url), call, target, by, p }`, `load(root)` and
  `replay(links, workspace, exports) -> (links, Vec<Evaluation>)`, called by
  `Engine::links_and_decisions` at the end. Replay is a pure function of links, exports and the
  files, so nothing is stored and a changed file needs no reindex. `evaluate` is the one place
  that decides a decision's `Fate`, in this order: call gone (`Cause::CallGone`), target
  repository a missing peer (`Waiting`, neither applied nor stale, whatever else holds), item no
  longer open (the core resolves the call on its own, or the item is not of the decision's kind:
  `ItemNotOpen`), target gone (`TargetGone`), target not among the ambiguous link's candidates
  (`NotCandidate`); else it applies. A stale or waiting decision changes no link. `Evaluation {
  repository, stale: Vec<Stale {call, target, cause}>, waiting: Vec<Waiting {call, target, peer}>
  }` is per repository, in file order; `Engine::decision_report()` returns them for the workspace
  (`Engine::local_decisions()` the one of the repository where Meada runs,
  `Engine::status_report()` them and the resource lines in one pass), `export` and `status` render
  them, and `Evaluation::lines` is the one rendering of the stale and waiting lines (`status`,
  `meada decide --list`); `--prune` reuses the same call. `probability(p)` is the one [0,1] check,
  for the file's `p`, a decider's answer and `--min-p`. `load` rejects a file that repeats a
  `call` (parse error `<file>:<line>: `call`: ...` at the repeat): one entry per call is a file
  invariant, so replay and the writers never see duplicates. The writer is `canonical(root, new)`
  (existing entries minus those of the same `call`, plus `new`, sorted by `call`, no comments or
  timestamps; `Decision::human(item, candidate)` builds a human pick) and `save(root, text)`.
- `decider/`: decider mode, the **only network path of Meada**. `mod.rs` owns
  `Config::from_env` (the four `MEADA_DECIDER_*` variables, read nowhere else: errors name the
  variable, `decisions` defaults to OpenRouter and `typesafe/jev-1.13` and needs the key, `chat`
  needs URL and model; HTTPS except to a loopback host), `Question` (the excerpt: call
  expression, the enclosing span read from disk only when the pack's parse of the file says
  that symbol is a function, method or inline function (`function_like`), so a call in a class
  field initializer goes alone and the top level says so; `offers: Vec<Offer {key, description,
  target}>`, keyed `"1".."n"`, described by `Candidate::describe`), `MAX_CANDIDATES` (50: more
  is skipped in decider mode only), `Config::requests` (the protocol dispatch), `show`
  (`--show-request`, the
  `Authorization` value printed as `<hidden>`) and `send` (`ureq`, rustls, fixed `TIMEOUT`, no
  proxy to a loopback host, no redirects, no retries; a non-success status is the error
  `Msg::HttpStatus` (`http status: <code>`, plus `: <message>` when the redacted body has
  `{"error": {"message"}}`)). `Config::shown(text)` is the one gate for every decoded upstream
  string printed or recorded (error message, model id, serde error text, type, keys, choice):
  control characters dropped, then the key replaced by `<hidden>`, since a JSON escape hides
  it from the raw-body redaction in `send`. `Config` has no `Debug`; the key
  leaves it only as the bearer header.
  Protocol adapters are files beside it that build requests and read answers: `chat.rs`
  (`request(config, question)`, one `POST <url>/chat/completions` per item; `answer(config,
  question, body) -> Result<Proposal>` reads `choices[0].message.content` as `{choice,
  probability}`, `by` = the response `model` or the configured one). `decisions.rs`
  (OpenRouter's alpha API: `request(config, questions) -> Option<Request>`, one `POST
  <url>/api/alpha/decisions` carrying every eligible item, `state {task, repository, items:
  name -> excerpt}` (upstream: `state` is the content to evaluate), `questions` named
  `item1..` zero-padded so the sorted maps keep item order, each `{type: choice,
  instructions: "... the call in items.<name> ...", criteria: key -> describe}`, `provider
  {zdr: true, data_collection: deny}`; `answers(config, asked, body)` parses `{model, answers, id?, provider?,
  usage?}` and each `{type, choice, confidence, probabilities}` with `deny_unknown_fields`: a
  shape error or an answer to an unknown question fails every item, an answer's values (type
  `choice`, choice and probability keys offered, each in [0,1], sum 1 within `SUM_TOLERANCE` =
  0.01, `confidence` in [0,1]) fail its item; `p` = probability of the choice, `by` = response
  `model`, `confidence` printed, not recorded). A refusal of the data-retention preferences
  arrives as a non-success status: every item of the request fails, nothing is written,
  nothing retries. A new protocol adds a file and one arm each in `Config::requests` and
  `Config::answers`.
  `validate.rs` is shared by every protocol: `proposal(config, question, choice, p, by) ->
  Result<Proposal>` (choice among the offered keys through `offered(config, question, choice)
  -> Result<&Offer>`, `p` in [0,1] through `probability(p)`, `by` not empty and not `human`; a
  `Proposal {decision, p, candidate, confidence}`) and `invalid(reason)` (the `invalid answer:
  <reason>` error text). `run`
  prints per item: `<id>` then `proposed: ...` (`p >= min_p`, returned to be written),
  `suggestion, not written: ...` (below it, `MIN_P` = 0.5 unless `--min-p`), both with
  `confidence` when the protocol gives one, or the error (`request failed: ...` /
  `invalid answer: ...`, counted in `Outcome::failed`).
