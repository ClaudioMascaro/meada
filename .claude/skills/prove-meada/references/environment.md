# Workload environment resolution (#663)

Answer key: #663 and #642 stories 47 to 49, ADR 0005's scoped-environment amendment.
Drive `python3 $S/environment.py --head-bin "$RUN/bin/meada-head" --run "$RUN/environment"`.
The script generates sibling repositories independently of the integration fixture files and
checks CLI export, workload context and queue-to-code trace over MCP stdio and HTTP. Its saved
source inventory and raw answers accompany the receipt. Inspect the whole resolved producer
and provision sets, their inferred levels and workload/variable reasons. The same variable in
an unrelated workload and a Secret reference must remain unresolved; changing only the
ConfigMap must refresh the answer. No value may come from the verifier's process environment.

Shared index/contract changes also require the Scope table's all-corpus base parity and console
query checks. A script pass is evidence, never a replacement for the independent verdict.

Controls to try independently: a second container with a different image, namespace mismatch,
missing ConfigMap/key, Secret envFrom, duplicate entries, direct Compose path outside the code
subdirectory, Compose defaults and override removal, Kustomize roots and nested same-namespace child prefixes, unknown later `envFrom` with a
prefix, unresolved Compose URL interpolation, truncation and hostile fan-out. Mutation audit: remove the environment workspace step, allow every workload to match,
or let a redacted target resolve; each relevant check must turn red.
