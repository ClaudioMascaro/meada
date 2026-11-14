# Alert labels and routes (#672)

Run `python3 $S/alerts.py --head-bin "$RUN/bin/meada-head" --run "$RUN/alerts" --http`.
The answer keys are #672, parent #642 and ADR 0026, obtained at run time. The script's
neutral repositories are separate from feature fixtures and preserve every source file, hash,
raw CLI response and MCP answer under the run directory. It drives full emitter/matcher/link
sets, equality versus regex levels, conjunctive routing, receiver context, both trace
directions, Helm exclusions, refresh, restoration and scope-aware orphan reporting.

The fresh checker additionally reads the pinned `kube-prometheus` rule labels and compares
them with its alert export, probes PrometheusRule, Loki, Grafana and AlertmanagerConfig, and
loads the neutral exports through the console graph, checking alert contracts and links.
Shared trace/schema changes require existing all-corpus parity and the console API checks.
A driver pass alone leaves those independent public-source and graph checks unproved.
