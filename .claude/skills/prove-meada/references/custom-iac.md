# Curated and private IaC (#671)

This extends the existing app proof through `/create-prove-it`. New mappings answer to #671,
#642 and ADR0025/0026, together with official schemas linked below; existing native workloads,
code-only channels and shared query behavior answer to the base binary and their existing
registered flows. MCP HTTP copies the complete restored stdio answers. A script pass is an
implementer receipt; an independent checker still reads the sources and judges it.

From the repository root, with immutable binaries copied inside the build gate:

```bash
python3 .claude/skills/prove-meada/scripts/custom-iac.py \
  --head-bin "$RUN/bin/meada-head" --base-bin "$RUN/bin/meada-base" \
  --run "$RUN/custom-iac" --http
```

The optional base must predate #671, while containing its cloud contract prerequisites. It
should export zero curated provisioners for the neutral inventory; preserve that RED output.
Head/base binary hashes and all raw stdout, stderr and exit captures remain in the run.

The driver supplies 43 distinct names, physical object identities and whole expected
provisioner tuples directly, without importing product type tables, feature fixtures or
goldens. It asks context for every entry and search for an unlisted adjacent group through
stdio and HTTP. The exact inventory is Strimzi KafkaTopic, Rabbit Queue, 12 ACK types, six
Config Connector types, ten Upbound AWS types, six Upbound GCP types and seven Upbound Azure
types, matching the capabilities inventory. Cloud SQL instance/database and Service Bus
topic/subscription remain composite identities; Pub/Sub subscriptions retain their marker.

The second generated workspace drives all six registry sources, evaluated private names,
exact-source precedence, prefix-neighbor refusal, a custom key path, false/unknown creation,
zero count, unknown names and generated prefixes. It compares the full provisioner inventory
and image runner sides. Separate image builder apps prove Lambda and ECS environments stay
with their own image/container; a disabled container cannot supply an environment. Scalar
Lambda source and opaque package paths produce exactly two executions, with source removal,
outside symlink refusal, package removal and restoration. Unsupported list and parent paths
and unknown package creation produce no execution.

A real Rabbit operator Queue/Exchange/Binding routes a real publish and consumer. The driver
checks complete binding, link and provisioning sets, with context references, trace up from
publish and down from Queue, and CLI status. Removing private mappings removes their sides;
restoration restores the entire inventory. An unknown entry must fail and name both entry
and meada.toml. `checks.json` records driven surfaces without claiming an independent verdict.

Official identity/schema sources (read independently, never during indexing):

- [Strimzi KafkaTopic](https://strimzi.io/docs/operators/latest/configuring.html#type-KafkaTopic-reference)
- [RabbitMQ messaging topology CRDs](https://github.com/rabbitmq/messaging-topology-operator/tree/main/config/crd/bases)
- [ACK controllers](https://github.com/aws-controllers-k8s), each controller's `config/crd/bases`
- [Config Connector resource CRDs](https://github.com/GoogleCloudPlatform/k8s-config-connector/tree/master/config/crds/resources)
- Upjet provider CRDs: [AWS](https://github.com/crossplane-contrib/provider-upjet-aws/tree/main/package/crds),
  [GCP](https://github.com/crossplane-contrib/provider-upjet-gcp/tree/main/package/crds),
  [Azure](https://github.com/crossplane-contrib/provider-upjet-azure/tree/main/package/crds)
- [terraform-aws-modules](https://github.com/terraform-aws-modules), source `variables.tf` and
  `main.tf`, with ECS's `modules/service` explicitly selected

Limits: no claim for all CRDs, all provider versions, downloaded module internals or arbitrary
packaging forms. The driver does not replace the full native workload/channel regression
groups, hostile input probes, all-corpus shared-code base parity, or console viewer parity.
Those existing Scope rows remain applicable and are recorded separately by integration proof.
