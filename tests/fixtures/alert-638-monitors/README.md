# Public issue 638

The original `monitors/app.yaml`, routing HCL and sibling app Helm values are copied
from https://github.com/ClaudioMascaro/meada/issues/638. The unsupported monitor YAML
and unrendered Helm values remain configuration, as spec 642 requires.

`main.tf` is the standard Datadog resource adaptation of the same monitor and tags.
The original routing rule tests `service:app`, not `support:team-b`. Trace from this
monitor's support side reaches that rule because both tags belong to the same monitor.
It does not prove a support matcher exists or predict final team delivery.
