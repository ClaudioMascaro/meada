# Kubernetes and Crossplane

- Custom identities/topology (#671) share `OBJECTS_MAX`, `OBJECT_REFERENCES_MAX`, unit/repository
  storage budgets and `COMPOSED_MAX`. Literal input text is bounded by `VALUE_MAX`; projection
  keys/names, scopes, references and binding copies charge before allocation. Hash lookups
  resolve unique endpoints; no endpoint Cartesian product is expanded. Crossplane patch sets
  are indexed once, and missing/duplicate/nested sets stay unread. The full-file custom binding
  fan-out probe runs independently through `custom_topology_reader_stays_bounded_on_a_full_file`.
Ingress HTTP summaries (#665) share `OBJECT_REFERENCES_MAX` (64 host/path rules, read or not)
and `UNIT_STORED_MAX` (4 MiB of borrowed route text before copying, then charged to the unit
again with its other facts). Passing either makes every host of the Ingress one unread mount,
preventing a partial prefix table from choosing a shorter route. Rewrite or `use-regex`
annotations, unknown/duplicated backends and a `pathType` other than `Prefix` (none included)
leave a path an unread mount; a nonliteral or controller path is an unread mount at its literal
head cut at its last slash (a path left out, at the root), and a wildcard host gives no route. `http/infrastructure.rs`
charges internal provider forms before path/reason expansion to `ALIAS_BYTES_MAX` (16 MiB per
query); overflow makes every Ingress mount unread for that query, preserving Service/Compose
routing. No partial alias table may select a shorter mount.

- `kube.rs` (#651, ADR 0025): `OVERLAY_DEPTH_MAX` (16 kustomizations as bases inside each other;
  past it the entry is unresolved, as a base already on the stack is, `cycle`); `WALK_MAX`
  (50,000 kustomization entries one overlay walks, each object a component is handed counting
  one, which bounds bases and components reaching one another along many paths, and many
  objects through a long list; past it the rest of the overlay is left out: 1,000 objects pass
  through at most 50 components, and the component during which it is crossed still applies its
  names, namespace and images to what it was handed, without its own objects or components);
  `OBJECTS_MAX` (10,000 objects per overlay or directory, bases included, the rest left out; a
  file or base one kustomization names twice instantiates once, also when one entry lists it as
  a component, one that several of its bases name, once through each, and a component listed
  twice applies twice);
  `OVERLAY_BYTES_MAX` (16 MiB of objects one overlay copies, each weighed by the size of what was
  read from it, so a large object reached along many paths stops the overlay there, the rest
  left out); `OBJECT_REFERENCES_MAX` (64
  references per object, the first in address order; a Service stops scanning candidates at 64
  matches); `UNIT_STORED_MAX` (4 MiB of addresses, references, images and unresolved entries per
  unit) and `KUBE_STORED_MAX` (16 MiB for the repository), past which the rest is left out; each
  node also counts `NODE_BYTES` (128) however short its names, which keeps a repository to about
  120,000 nodes. `HOST_MAX` (253 bytes, DNS's limit) and `VOLUME_NAME_MAX` (63, a DNS-1123
  label): an Ingress rule whose host is longer, or a volume whose name is, is invalid and gives
  no backend or mount reference. `OBJECT_NAME_MAX` (253, a DNS subdomain): a reference by the
  final name reaches no object whose final name is longer (a ConfigMap or a Secret excepted),
  only one renamed from the name it writes.
  A file of more than `config_values::DOCUMENTS_MAX` documents gives no object. Only a
  `kustomization.yaml` or `kustomization.yml` is a kustomization (not `Kustomization`); only
  `resources`, `bases` and `components` are followed (a component changes and adds to what the
  kustomization that lists it holds), and patches (`patches`,
  `patchesStrategicMerge`, `patchesJson6902`, the files they name are no objects), generators,
  `commonLabels`, `labels`, `replacements`, `helmCharts` and the object a `kind: List` holds
  are not applied or read. A ConfigMap or Secret reference is renamed with its target at each
  stage, so one to a generator's resolves to nothing. A Service reference, and a
  `serviceAccountName` of the kinds Kustomize renames (`ACCOUNT_RENAMED`), resolves once the tree
  is built, among the objects of the referrer's namespace (none and `default` alike) a stage
  renamed or moved from the name as written, by their outermost prefix and suffix stages
  (Kustomize's `selectReferral`), else by that name; another kind's account, or the deprecated
  `serviceAccount`, only by that name. Only the name as written is a previous name: a reference
  by the name a middle stage or a patch gave reaches the object written with that name, if one
  was, else the object finally called so, if any, where Kustomize picks the one renamed from it,
  or keeps the name when another object was also written with it. A cluster-scoped kind is known by
  the list `CLUSTER_SCOPED`; a custom resource outside it takes a Kustomize `namespace`. An
  object with no namespace is `<Kind>[.<group>]/<name>`, which reads as a cluster-scoped one.
  A document is no object when its name holds `{{` (a Helm template). A manifest no
  kustomization lists is in a unit of its directory, labelled by it, and shares the label of a
  kustomization of that directory; two objects of one address in a unit keep the first. A
  kustomization another names as a base is no overlay of its own (an overlay built on another
  yields its nodes under itself alone), and kustomizations that only name each other in a cycle
  are no root, so what they list is plain. An entry cut by `WALK_MAX` or a stored budget is left
  out with no unresolved row (the files of a component left out are plain units of their
  directory, as files no overlay reaches are), and a non-object file a kustomization lists, created or deleted
  alone, leaves the unresolved count as it was until a manifest changes. A
  Kubernetes file changed anywhere evaluates every root and overlay of the repository again
  (the `ponytail:` in `index::refresh_keeping`), reading every YAML or JSON file that says
  `apiVersion`. The Kustomize `images` transformer matches the name before the tag, as written
  (a `newTag` such as `1.20` read by YAML as a number loses its zero).

- `kube/crossplane.rs` (#655, ADR 0025): `COMPOSED_MAX` (64 resources created per composition,
  the first in the order written, each one reference of the composition beside
  `OBJECT_REFERENCES_MAX`; a definition lookup takes the first `OBJECT_REFERENCES_MAX` matches).
  Patches, patch sets, transforms and functions are never evaluated: a name or external name a
  patch writes, and a base with neither, are `*`, so a composed resource is never named from
  its composite, and a patch that writes `metadata.name` with a transform (a fixed prefix) still
  gives `*`. Only `spec.resources` and the `input.resources` of a `pt.fn.crossplane.io` step are
  read; a step of any other function (Go templating, KCL, a custom function) creates no node,
  and nor does an `EnvironmentConfig`'s or a `Usage`'s content. A claim or composite resource is
  known only by an XRD of the same repository (one in another repository of the workspace leaves
  it a plain object); a managed resource only by its `spec.forProvider` mapping. Every
  reference stays in one unit, so a claim in another directory than its XRD and composition
  (the usual layout of `apis/` beside `examples/`) references nothing. A resource kind is
  reached only through a definition in the unit, so a provider's managed resource kind (whose
  CRD the provider installs) is reached by nothing. Two resources of one name in a composition
  keep the first; a v2 XRD's `spec.scope` is not read, so a composite resource keeps the
  namespace it was written with; a `compositionSelector`, a `compositionRevisionRef` and
  managed resource references (`*Ref`, `*Selector` under `spec.forProvider`) are not followed.
  Only the groups `apiextensions.crossplane.io`, `ops.crossplane.io` and `pkg.crossplane.io` are
  cluster-scoped by group: a v1 composite or managed resource takes a Kustomize `namespace` in
  its address, as Kustomize gives it one without its CRD. A `compositionRef`,
  `defaultCompositionRef` or `enforcedCompositionRef` is matched by the composition's name as
  written, every composition written with it and none picked by its prefixes, though Kustomize
  renames the composition under a `namePrefix` and leaves those fields alone.
