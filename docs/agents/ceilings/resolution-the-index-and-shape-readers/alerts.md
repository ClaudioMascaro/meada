# Alert monitoring readers and routing

- `ALERT_STORED_MAX` (`alert/config.rs`): 4 MiB per monitoring file, charging stored
  facts, every projected rule/route IaC node field and the provider/consumer fields they
  materialize, including inherited conditions, repeated labels and long route paths; reaching
  it discards that file's alert facts. Pending routes retain parent indexes rather than copying
  inherited receivers or paths across the whole child list before visiting a child.
  The shared event reader bounds YAML/JSON nesting and alias expansion; the HCL `NESTING_MAX`
  and `OBJECT_WORK_MAX` guards bound Terraform alert and incident parsing.
- `ALERT_REGEX_MAX` (`alert.rs`): 4096 bytes per alert or incident regex;
  `ALERT_REGEX_SIZE_MAX`: 1 MiB each for compiled automata and their search caches, shared
  equally by distinct expressions in one route configuration; identical patterns share both. Larger expressions or automata
  stay unmatched, as do invalid or unsupported RE2 expressions. Regex matching is anchored
  to the entire label, tag or event-field value.
- `ALERT_REGEX_FOLD_MAX` (`alert.rs`): 2^22 code points that case folding may visit for the
  expressions of one route configuration, shared equally like `ALERT_REGEX_SIZE_MAX`.
  `regex-syntax` folds each Unicode class and bracket of a case-insensitive expression by
  visiting every code point of each of its ranges that may hold a cased letter (1.1 million for
  `(?i)\p{Any}`), before any automaton is built. `alert/regexp.rs` counts them first, an upper
  bound (an `i` flag anywhere counts every class, and a range reaching `A` counts whole); an
  expression past its share stays unmatched.

- `ALERT_LINK_BYTES_MAX` (`alert.rs`): 4 MiB of retained alert link fields and String
  contents per workspace pass. Every field is charged with checked arithmetic before
  cloning it or formatting the repeated conjunction reason. Overflow discards all alert
  links; status reports the output limit and suppresses misleading orphan reports.
- `RE2_REPEAT_MAX` (`alert/regexp.rs`): Go/RE2's counted-repeat limit of 1000, including
  nested counted-repeat products. Rust-only flags, class operators and Unicode property
  aliases are refused; canonical Go categories/scripts and ASCII Perl classes are read.

- `ALERT_MATCH_WORK_MAX` (`alert.rs`): 1,000,000 route visits and matcher evaluations per
  workspace pass, including unmatched pairs, receiver inspections and scanned template bytes. Exhaustion discards all alert links and status
  reports the evaluation limit instead of ordinary orphan results. Context reuses one prepared
  link pass; status reuses existing links with at most one extra empty-result diagnostic pass
  per query, independent of unrelated repository count. Roots are indexed once, so a false
  parent condition does not scan its descendants for every rule.

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
