# A7 declared resource locations (#641)

Use the existing CLI and MCP surfaces, with #641 and #642 stories 61/62 as the new-flow
answer key. Existing symbol declarations use the base binary and the chain fixture goldens.
The approved methods are full CLI export/status checks, MCP stdio context and both trace
directions, HTTP parity on the same repository scope, and removal/restoration on a persistent
stdio server. They extend the shared-code corpus and console parity selected by Scope.

```bash
python3 $S/resource-via.py --head-bin "$RUN/bin/meada-head" \
  --run "$RUN/resource-via" --http
```

The binary must be an immutable copy from the build identified in the receipt. The runner
creates sibling repositories inside its new run directory and saves every raw CLI output,
MCP wire reply, expected site and complete link set. It imports only the transport mechanics
from `prove.py`; its neutral source declarations are independent of feature tests and goldens.
No service credentials, cloud calls, package installation or source checkout mutation is used.
Loopback sockets are required for HTTP. Its manifest has precisely the two source repositories.

Done looks like: one writer at the selector's actual config key, every Terraform, Kubernetes,
Crossplane, Compose, template-qualified CloudFormation/SAM and Serverless reader at its own
node, every link `declared`, and only the deliberately deleted and ambiguous locations stale. Context reaches
both ends; traces cross both directions. All responses copy over HTTP byte for byte. Changing
the list selector makes its declaration stale; restoring it restores the entire trace.
A script pass says checks passed; a fresh checker inspects the source, oracle and replies and
writes the verdict. The parent implementation workflow arranges this independent run.

To try to fool the verifier, drop a reader, replace a key writer with the repository identity,
change a link to `exact`, or mark a valid key stale in saved output. The complete-set assertions
must reject each. Ordinary symbol declarations and no-config repositories must still match
the base. Tests cover quoted keys, selectors in nested lists, multi-document paths, ignored
files and secret/cut selector values; a checker additionally follows those cases through the
running binary when the diff changes their resolver.

This flow does not by itself prove console rendering or every code-language regression.
Run the existing console API/export parity and the all-corpus flows whenever Scope requires
them. Keep their receipts and any failures alongside this one; do not relabel a partial run
PROVED. No protocol or export-format integer change is introduced by #641.
