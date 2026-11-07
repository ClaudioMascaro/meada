# Scope and facts that remain unspecified

This key tests native HCL config scalar rows, not a Terraform plan or provider
validation. Source expressions in a default remain native HCL source and must be
redacted before any evaluation. No claim about deployment validity follows.
The unknown `neutral_widget.proof` resource is only a source-authorized context
container. Its one node is a control, not a generic node-name/provisioner proof.

The scalar starts at its expression's first source line, including a heredoc's
introducer. The HCL literal heredocs include their final content newline; the
indented form removes the minimum content indentation. Native sole interpolation
strings are `template`; the JSON-only single-interpolation `expression` rule does
not apply. Unary `-7` is `expression` under the pinned HCL numeric/unary grammar.
Numbers retain source spelling, including `12.50` and `1.25e+03`.

The source-derived config counters, rows and redaction counts are exact. The
normative issue/ADR/guide material does not define whether the separate IaC file
read counter includes the three var files. The inventory supplies 13 active
native inputs: 10 definition files plus 3 var files, of which 2 definition files
are unreadable. Capture the entire status and label that separate counter
unadjudicated/unspecified until an independent accepted definition is supplied.
Do not choose 8 or 11 read from a candidate answer and call it proved.

The public search rendering has no explicit type column. This key supplies every
native type independently; compare any documented public type metadata in the
complete direct-key context. If a type is not observable on the accepted public
surface, report that precise subfield UNPROVEN. Do not inspect internal tables or
infer an exposed schema from product output. Source file, key, line, shown value,
cut/redacted flags, complete count/order, status and privacy still have concrete
independent expectations.

The three `meada.toml` declarations use the existing #641 declared key-target
seam for direct contexts. Public target metadata may identify the file/key/type/
line/cut/redacted state and contract role, but must not repeat scalar payloads.
No new context API, default-secret classifier, secret decoding, environment read,
SSM acceptance claim, or runtime verdict is part of this key.
