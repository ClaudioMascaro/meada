# Terraform and OpenTofu JSON acceptance

Run the app's permanent JSON flow after changing Terraform reading, evaluation, references,
provisioning or JSON marker discovery:

```sh
python3 "$S/terraform-json.py" --head-bin "$RUN/bin/meada-head" --run "$RUN/terraform-json" --http
```

Its answer keys are #642, #649, ADR 0025 and the HCL JSON syntax specification. It generates
neutral inputs under its run directory and records their fixed whole sets in `oracle.json`.
It never mutates a user's checkout, calls a provider or reads a process variable as IaC.
All canaries are invented values.

Check the whole `checks.json` and the raw CLI, stdio and HTTP answers. Expected behavior is
18 nodes, six provisioners and one SNS-to-SQS subscription. The fixture includes `.tf.json`,
`.tofu.json`, native HCL, array block bodies, repeated section properties, two calls of one
local JSON module, count expansion, static dependencies, a plain string that is no reference,
a data block and an unlisted type. An ordinary `.json` and a Terraform JSON file shadowed by
its OpenTofu equivalent create no extra nodes. Sensitive default and tfvars canaries stay out
of value search and the initial stored index.

The same-size variable-default edit must change the warm MCP name, its warm whole export must
equal a cold copy, and restoring the source must restore all export bytes. Deleting an OpenTofu
JSON source must make its declared resource `via` stale, and restoring it must restore the
whole export. Server-mode answers must equal the restored stdio answers byte for byte.

A script pass is evidence for the checker, not its independent verdict. Inspect each failing
capture before changing a driver: a broken driver is BLOCKED, and missing required behavior
is BROKEN. Do not reduce the expected sets or filter away unexpected rows. Shared-code base
parity, corpus, console and performance checks still follow the main skill's Scope table.
