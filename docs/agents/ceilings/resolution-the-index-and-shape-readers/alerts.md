# Alert monitoring readers and routing

- `ALERT_STORED_MAX` (`alert/config.rs`): 4 MiB per monitoring file, charging stored
  facts, every projected rule/route IaC node field and the provider/consumer fields they
  materialize, including inherited conditions, repeated labels and long route paths; reaching
  it discards that file's alert facts. Pending routes retain parent indexes rather than copying
  inherited receivers or paths across the whole child list before visiting a child.
  The shared event reader bounds YAML/JSON nesting and alias expansion; the existing HCL
  `NESTING_MAX` guard bounds Terraform Grafana rule parsing.
- `ALERT_REGEX_MAX` (`alert.rs`): 4096 bytes per Alertmanager regex;
  `ALERT_REGEX_SIZE_MAX`: 1 MiB each for compiled automata and their search caches, shared
  equally by distinct expressions in one route configuration; identical patterns share both. Larger expressions or automata
  stay unmatched, as do invalid or unsupported RE2 expressions. Regex matching is anchored
  to the entire label value.

- `ALERT_LINK_BYTES_MAX` (`alert.rs`): 4 MiB of retained alert link fields and String
  contents per workspace pass. Every field is charged with checked arithmetic before
  cloning it or formatting the repeated conjunction reason. Overflow discards all alert
  links; status reports the output limit and suppresses misleading orphan reports.
- `RE2_REPEAT_MAX` (`alert/regexp.rs`): Go/RE2's counted-repeat limit of 1000, including
  nested counted-repeat products. Rust-only flags, class operators and Unicode property
  aliases are refused; canonical Go categories/scripts and ASCII Perl classes are read.

- `ALERT_MATCH_WORK_MAX` (`alert.rs`): 1,000,000 route visits and matcher evaluations per
  workspace pass, including unmatched pairs. Exhaustion discards all alert links and status
  reports the evaluation limit instead of ordinary orphan results. Context reuses one prepared
  link pass; status reuses existing links with at most one extra empty-result diagnostic pass
  per query, independent of unrelated repository count. Roots are indexed once, so a false
  parent condition does not scan its descendants for every rule.
