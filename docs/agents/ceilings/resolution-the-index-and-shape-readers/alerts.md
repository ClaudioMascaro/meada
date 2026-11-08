# Alert monitoring readers and routing

- `ALERT_STORED_MAX` (`alert/config.rs`): 4 MiB per monitoring file, charging stored
  facts, every projected rule/route IaC node field and the provider/consumer fields they
  materialize, including inherited conditions, repeated labels and long route paths; reaching
  it discards that file's alert facts. Pending routes retain parent indexes rather than copying
  inherited receivers or paths across the whole child list before visiting a child.
  The shared event reader bounds YAML/JSON nesting and alias expansion; the HCL `NESTING_MAX`
  and `OBJECT_WORK_MAX` guards bound Terraform alert and incident parsing.
- `ALERT_REGEX_MAX` (`alert.rs`): 4096 bytes per alert or incident regex. The budgets below
  come in equal parts (`regexp::Pass`): a linking pass gives one to each routing scope that
  compiles an expression (a route configuration with a label route, and one repository's incident
  routes together), and a scope shares its part equally among its distinct expressions, so what
  an expression may spend depends only on how many scopes and expressions the workspace holds,
  never on the repository asking or the scopes compiled before it. A pass compiles an
  expression once for each share it gets, so the same expression in scopes of as many
  expressions is compiled once. Larger expressions or automata stay unmatched, as do invalid or
  unsupported RE2 expressions; `status` shows such a matcher as an orphan with no emitter, not
  as a limit. Regex matching is anchored to the entire label, tag or event-field value.
- `ALERT_REGEX_SIZE_MAX` (`alert.rs`): 1 MiB per scope for compiled automata and their search
  caches; `ALERT_REGEX_POOL_MAX`: 16 MiB per linking pass, a scope's part being the smaller of
  the two. An automaton of 1 MiB compiles in under 10 ms (release), where an ordinary expression
  needs about 1.5 KiB; 400 route configurations compiling a megabyte each took 14.6 s (debug),
  while from 389 scopes on no part holds even `\pL` (43 KB).
- `ALERT_REGEX_FOLD_MAX` (`alert.rs`): 2^22 code points that case folding may visit in one
  linking pass. `regex-syntax` folds each Unicode or ASCII class, and each bracket holding a
  character or a range, of a case-insensitive expression by visiting every code point of each of
  its ranges, before negation, that holds a cased one (1.1 million for `(?i)\p{Any}`, none for
  `\p{Han}`), before any automaton is built; a bracket of classes alone, such as the `[\S]` the
  ASCII Perl classes become, is folded already. `alert/regexp.rs` counts them first, an upper
  bound: an `i` flag anywhere counts every class, and a range holding a code point of
  `\p{CWCM}` (a superset of what folding maps) counts whole. A bracket of characters and classes
  visits its classes as folding left them, which the count folds too once it fits (106 for
  `(?i)[\w-]`, where counting every cased code point gave 3,033).
- `ALERT_REGEX_CLASSES_MAX` (`alert.rs`): 4096 Unicode classes in the expressions one linking
  pass reads, case-insensitive or not: `regex-syntax` looks each up in its tables and joins it to
  the bracket around it before any automaton is built. The empty-result diagnostic pass of
  `status` compiles with the same parts.

- `ALERT_LINK_BYTES_MAX` (`alert.rs`): 4 MiB of retained alert link fields and String
  contents per workspace pass. Every field is charged with checked arithmetic before
  cloning it or formatting the repeated conjunction reason. Overflow discards all alert
  links; status reports the output limit and suppresses misleading orphan reports.
- `RE2_REPEAT_MAX` (`alert/regexp.rs`): Go/RE2's counted-repeat limit of 1000, including
  nested counted-repeat products. Rust-only flags, class operators and Unicode property
  aliases are refused; canonical Go categories/scripts and ASCII Perl classes are read.

- `ALERT_MATCH_WORK_MAX` (`alert.rs`): 1,000,000 route visits and matcher evaluations per
  workspace pass, including unmatched pairs, receiver inspections, scanned template bytes and the
  steps matching may take (`ALERT_SCAN_UNIT`). Exhaustion discards all alert links and status
  reports the evaluation limit instead of ordinary orphan results. Context reuses one prepared
  link pass; status reuses existing links with at most one extra empty-result diagnostic pass
  per query, independent of unrelated repository count. Roots are indexed once, so a false
  parent condition does not scan its descendants for every rule.
- `ALERT_SCAN_UNIT` (`alert.rs`): 64 steps per unit of `ALERT_MATCH_WORK_MAX`. Before reading a
  value, each condition is charged, past its own unit, what matching it may take: for an
  expression (`regexp::Compiled::cost`), a step per automaton position (a character, dot or
  class, once per copy its counted repetitions make) and byte offset it may start at, every
  offset of the value and one more inside or after an unbounded repetition, else one more than
  the characters what precedes it may vary by; for a comparison or a substring search, the bytes
  it reads. A condition on tags is charged for all of them at once. Matching `.*a.{1000}a.*`
  against a label of 300,000 bytes mixing `a` and `b` (the lazy DFA cannot hold its states and
  gives way to the PikeVM, 84 ns a step in a debug build) took 43 s for each route; it now stops
  at the evaluation limit before matching, and a pass runs at most 2^26 steps (5.4 s, debug;
  `status` may run two passes). A literal alternation costs its characters whatever the value, so
  up to 63 of them cost no unit, and an ordinary expression on a short value a few units at most.

- `ALERT_PCL_DEPTH_MAX` (`alert/pcl.rs`): 64 nested PCL groups or unary conditions.
  Deeper conditions remain unread as a whole. The literal reader recognizes `matches`,
  `matches regex` and `matches part`, conjunction, disjunction, negation and grouping; it does not
  execute PCL, provider expressions or templates. Missing native event fields are unknown.

The incident readers share `ALERT_STORED_MAX`, the YAML/JSON event reader limits and the
existing HCL nesting guard. `ALERT_LINK_BYTES_MAX` also charges retained receiver expansions,
scopes and explanations before allocating them. Label substitution visits the original
receiver template once; substituted text is never reparsed. Only explicitly grouped label
keys (or all labels with `...`) can establish CommonLabels/GroupLabels substitutions.
OpsGenie conditions test complete tag strings. Tag and event-field streams cannot join
without a known forwarding. Regex compilation uses the same validated Go/RE2 helper and
shared automaton and case-folding budgets as Alertmanager.
