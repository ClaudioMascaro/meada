//! Crossplane objects among the Kubernetes nodes (#655, ADR 0025). A composite resource
//! definition (XRD) defines a composite kind and, when it offers one, a claim kind; an object of
//! either kind anywhere in the repository is a composite resource or a claim. A composition names
//! the composite kind it composes and the resources it creates, each read from its literal `base`
//! (`spec.resources`, or the `input.resources` of a pipeline step that runs function
//! patch-and-transform) into a node under the composition's address, `<composition>/<resource>`,
//! of the base's kind. An object with a `spec.forProvider` mapping is a managed resource.
//!
//! References, in one unit as every Kubernetes one: a claim to the XRD of its composite kind
//! (`composite <Kind>.<group>`), a composite resource to its XRD, a claim or a composite resource
//! to the composition its `compositionRef` names, an XRD to its default and enforced composition,
//! a composition to the XRD of its `compositeTypeRef` and to each resource it creates, and a
//! composed or managed resource to the definition of its kind (an XRD, or a
//! `CustomResourceDefinition`) when the unit holds one.
//!
//! Patches and functions are not evaluated: a base whose name or external name a patch sets, or
//! that has neither, has the name `*`, a pattern; a pipeline step that runs any other function
//! creates no node.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde_json::Value;

use super::{OBJECT_REFERENCES_MAX, Obj, Span, at, list, plain, text};

/// The groups of Crossplane's own kinds, all cluster-scoped.
const GROUPS: &[&str] = &[
    "apiextensions.crossplane.io",
    "ops.crossplane.io",
    "pkg.crossplane.io",
];

const EXTENSIONS: &str = "apiextensions.crossplane.io";

/// The annotation that holds a managed resource's name in the provider's API.
const EXTERNAL_NAME: &str = "crossplane.io/external-name";

/// The resources one composition creates; past it the rest create no node.
// ponytail: real compositions create a few dozen resources; 64 is past them, and each is one
// reference of the composition besides `kube::OBJECT_REFERENCES_MAX`.
pub(super) const COMPOSED_MAX: usize = 64;

/// The patches that write the composite or the environment, never the base.
const OUTWARD: &[&str] = &[
    "ToCompositeFieldPath",
    "CombineToComposite",
    "ToEnvironmentFieldPath",
    "CombineToEnvironment",
];

/// Whether the kinds of `group` are Crossplane's own, which live outside any namespace.
pub(super) fn cluster_scoped(group: &str) -> bool {
    GROUPS.contains(&group)
}

/// A kind with its group, as `<Kind>.<group>` shows it.
type Kind = (String, String);

fn shown((group, kind): &Kind) -> String {
    match group.is_empty() {
        true => kind.clone(),
        false => format!("{kind}.{group}"),
    }
}

/// The group of an `apiVersion`: what comes before its last `/`, none for the core group.
fn group_of(api: &str) -> &str {
    api.rsplit_once('/').map_or("", |(g, _)| g)
}

/// One resource a composition creates, read from its base.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Base {
    /// The resource's `name` in the composition, or its index when it has none.
    pub resource: String,
    pub kind: Kind,
    /// Its physical name: the external-name annotation or `metadata.name` as written; `None`
    /// when a patch sets either, or the base has neither.
    pub name: Option<String>,
    pub line: usize,
    pub end_line: usize,
    pub block: String,
    /// How the composition holds it: `resources` or `pipeline <step>`.
    pub via: String,
    pub custom: Option<super::custom::Name>,
    pub topology: Option<super::topology::Facts>,
    pub namespace: Option<String>,
}

/// What a Crossplane object, or a definition of a kind, offers the roles and the references.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Facts {
    /// The kind an XRD or a `CustomResourceDefinition` defines.
    pub defines: Option<Kind>,
    /// The claim kind an XRD offers.
    pub claim: Option<Kind>,
    /// An XRD's compositions by name, with the key that names each.
    pub compositions: Vec<(&'static str, String)>,
    /// The composite kind a composition composes.
    pub composes: Option<Kind>,
    pub bases: Vec<Base>,
    /// The composition a claim or a composite resource names.
    pub composition: Option<String>,
    /// Whether the object holds a `spec.forProvider` mapping, as every managed resource does.
    pub managed: bool,
    /// Its literal external-name annotation.
    pub external: Option<String>,
}

/// The literal name at `path` of `v`.
fn name_at(v: &Value, path: &[&str]) -> Option<String> {
    text(v, path).filter(|n| plain(n)).map(String::from)
}

fn external(v: &Value) -> Option<String> {
    let name = at(v, &["metadata", "annotations", EXTERNAL_NAME])?.as_str()?;
    plain(name).then(|| name.to_string())
}

/// Whether the field path `path` is the name or the external name of the base.
fn names(path: &str) -> bool {
    path == "metadata.name"
        || path.starts_with("metadata.annotations") && path.contains(EXTERNAL_NAME)
}

/// Whether the patch `p` writes the base's name or external name. A patch from a field path
/// with no `toFieldPath` writes the same path of the base.
fn writes(p: &Value) -> bool {
    let ty = text(p, &["type"]).unwrap_or("FromCompositeFieldPath");
    let from = matches!(ty, "FromCompositeFieldPath" | "FromEnvironmentFieldPath")
        .then(|| text(p, &["fromFieldPath"]))
        .flatten();
    !OUTWARD.contains(&ty) && text(p, &["toFieldPath"]).or(from).is_some_and(names)
}

/// The names of the patch sets `sets` that write the base's name or external name.
fn renaming(sets: &Value) -> HashSet<&str> {
    (sets.as_array().into_iter().flatten())
        .filter(|s| list(s, &["patches"]).any(writes))
        .filter_map(|s| text(s, &["name"]))
        .collect()
}

/// Whether one of `patches`, or a patch set of `renaming` they name, writes the base's name or
/// external name.
fn renamed(patches: &Value, renaming: &HashSet<&str>) -> bool {
    (patches.as_array().into_iter().flatten()).any(|p| match text(p, &["type"]) {
        Some("PatchSet") => text(p, &["patchSetName"]).is_some_and(|n| renaming.contains(n)),
        _ => writes(p),
    })
}

/// Unknown patch sets and nested sets cannot establish a literal identity. Index the sets
/// once per composition; their fields are inspected without copying or expanding resources.
fn patches_affect(
    patches: &Value,
    sets: &HashMap<&str, Option<&Value>>,
    affected: impl Fn(&str) -> bool,
) -> bool {
    let writes = |patch: &Value| {
        let ty = text(patch, &["type"]).unwrap_or("FromCompositeFieldPath");
        if OUTWARD.contains(&ty) {
            return false;
        }
        if !matches!(
            ty,
            "FromCompositeFieldPath"
                | "FromEnvironmentFieldPath"
                | "CombineFromComposite"
                | "CombineFromEnvironment"
        ) {
            return true;
        }
        text(patch, &["toFieldPath"])
            .or_else(|| text(patch, &["fromFieldPath"]))
            .is_none_or(&affected)
    };
    (patches.as_array().into_iter().flatten()).any(|patch| {
        if text(patch, &["type"]) != Some("PatchSet") {
            return writes(patch);
        }
        let set = text(patch, &["patchSetName"])
            .and_then(|name| sets.get(name))
            .copied()
            .flatten();
        set.is_none_or(|set| list(set, &["patches"]).any(writes))
    })
}

/// The resources of the composition `value` whose block starts its key paths with `block`, each
/// with the lines `span` gives it (its step's index in the pipeline, if any, and its own).
fn bases(
    value: &Value,
    block: &str,
    span: Span,
    mappings: &crate::iac::mapping::Config,
) -> Vec<Base> {
    let dot = if block.is_empty() { "" } else { "." };
    let mut out = Vec::new();
    let mut take = |resources: &Value, sets: &Value, step: Option<usize>, via: String| {
        let at_key = match step {
            Some(j) => format!("{block}{dot}spec.pipeline[{j}].input.resources"),
            None => format!("{block}{dot}spec.resources"),
        };
        let renaming = renaming(sets);
        let mut named_sets = HashMap::new();
        for set in sets.as_array().into_iter().flatten() {
            if let Some(name) = text(set, &["name"]) {
                named_sets
                    .entry(name)
                    .and_modify(|v| *v = None)
                    .or_insert(Some(set));
            }
        }
        for (i, r) in (resources.as_array().into_iter().flatten()).enumerate() {
            if out.len() == COMPOSED_MAX {
                return;
            }
            let base = &r["base"];
            let (Some(api), Some(kind)) = (text(base, &["apiVersion"]), text(base, &["kind"]))
            else {
                continue;
            };
            let resource = (text(r, &["name"]).filter(|n| !n.is_empty()))
                .map_or_else(|| i.to_string(), String::from);
            let name = match renamed(&r["patches"], &renaming) {
                true => None,
                false => external(base).or_else(|| name_at(base, &["metadata", "name"])),
            };
            let mut custom = super::custom::read(base, kind, group_of(api), mappings);
            if let Some(custom) = &mut custom
                && patches_affect(&r["patches"], &named_sets, |path| custom.affected(path))
            {
                custom.invalidate();
            }
            let mut topology = super::topology::read(base, kind, group_of(api));
            if let Some(topology) = &mut topology
                && patches_affect(&r["patches"], &named_sets, |path| topology.affected(path))
            {
                topology.invalidate();
            }
            let (line, end_line) = span(step, i).unwrap_or((1, 1));
            out.push(Base {
                resource,
                kind: (group_of(api).into(), kind.into()),
                name,
                line,
                end_line,
                block: format!("{at_key}[{i}]"),
                via: via.clone(),
                custom,
                topology,
                namespace: text(base, &["metadata", "namespace"]).map(String::from),
            });
        }
    };
    take(
        &value["spec"]["resources"],
        &value["spec"]["patchSets"],
        None,
        "resources".into(),
    );
    for (j, step) in list(value, &["spec", "pipeline"]).enumerate() {
        let input = &step["input"];
        if !text(input, &["apiVersion"]).is_some_and(|a| a.starts_with("pt.fn.crossplane.io/")) {
            continue;
        }
        let via = format!("pipeline {}", text(step, &["step"]).unwrap_or_default());
        take(&input["resources"], &input["patchSets"], Some(j), via);
    }
    out
}

/// The Crossplane facts of the object `value` of `kind` in `group`, whose block is `block`.
pub(super) fn facts(
    value: &Value,
    kind: &str,
    group: &str,
    block: &str,
    span: Span,
    mappings: &crate::iac::mapping::Config,
) -> Facts {
    let mut facts = Facts::default();
    let spec = &value["spec"];
    let kind_at = |path: &[&str]| Some((text(spec, &["group"])?.to_string(), name_at(spec, path)?));
    match (group, kind) {
        (EXTENSIONS, "CompositeResourceDefinition") => {
            facts.defines = kind_at(&["names", "kind"]);
            facts.claim = kind_at(&["claimNames", "kind"]);
            for key in ["defaultCompositionRef", "enforcedCompositionRef"] {
                if let Some(name) = name_at(spec, &[key, "name"]) {
                    facts.compositions.push((key, name));
                }
            }
        }
        ("apiextensions.k8s.io", "CustomResourceDefinition") => {
            facts.defines = kind_at(&["names", "kind"]);
        }
        (EXTENSIONS, "Composition") => {
            let r = &spec["compositeTypeRef"];
            facts.composes = (text(r, &["apiVersion"]).zip(name_at(r, &["kind"])))
                .map(|(api, kind)| (group_of(api).to_string(), kind));
            facts.bases = bases(value, block, span, mappings);
        }
        _ if cluster_scoped(group) => {}
        _ => {
            facts.composition = name_at(spec, &["compositionRef", "name"])
                .or_else(|| name_at(spec, &["crossplane", "compositionRef", "name"]));
            facts.managed = spec.get("forProvider").is_some_and(Value::is_object);
            facts.external = external(value).filter(|_| facts.managed);
        }
    }
    facts
}

/// The composite and claim kinds the XRDs of the repository define.
#[derive(Debug, Default)]
pub(super) struct Kinds {
    composite: HashSet<Kind>,
    claim: HashSet<Kind>,
}

impl Kinds {
    pub(super) fn of<'o>(objs: impl Iterator<Item = &'o Obj>) -> Kinds {
        let mut kinds = Kinds::default();
        for o in objs.filter(|o| own(o, "CompositeResourceDefinition")) {
            kinds.composite.extend(o.facts.xp.defines.clone());
            kinds.claim.extend(o.facts.xp.claim.clone());
        }
        kinds
    }

    /// What `o` is to Crossplane, if anything.
    pub(super) fn role(&self, o: &Obj) -> Option<Role> {
        let kind = (o.group.clone(), o.kind.clone());
        Some(match () {
            _ if own(o, "CompositeResourceDefinition") => Role::Definition,
            _ if own(o, "Composition") => Role::Composition,
            _ if self.claim.contains(&kind) => Role::Claim,
            _ if self.composite.contains(&kind) => Role::Composite,
            _ if o.facts.xp.managed => Role::Managed,
            _ => return None,
        })
    }
}

/// Whether `o` is Crossplane's own `kind` of `apiextensions.crossplane.io`.
fn own(o: &Obj, kind: &str) -> bool {
    o.group == EXTENSIONS && o.kind == kind
}

/// What a node is to Crossplane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Definition,
    Composition,
    Claim,
    Composite,
    Managed,
    /// A resource a composition creates.
    Composed,
}

impl Role {
    /// As `context` shows it on its `role:` line.
    pub(super) fn text(self) -> &'static str {
        match self {
            Role::Definition => "crossplane composite resource definition",
            Role::Composition => "crossplane composition",
            Role::Claim => "crossplane claim",
            Role::Composite => "crossplane composite resource",
            Role::Managed => "crossplane managed resource",
            Role::Composed => "crossplane composed resource",
        }
    }
}

/// The targets the Crossplane references of a unit's objects name, by the object's index.
pub(super) struct Targets<'o> {
    objs: &'o [Obj],
    /// XRDs and `CustomResourceDefinition`s by the kind they define.
    defines: HashMap<&'o Kind, Vec<usize>>,
    /// XRDs by the claim kind they offer.
    claims: HashMap<&'o Kind, Vec<usize>>,
    /// Compositions by name as written.
    compositions: HashMap<&'o str, Vec<usize>>,
}

impl<'o> Targets<'o> {
    pub(super) fn new(objs: &'o [Obj]) -> Targets<'o> {
        let mut t = Targets {
            objs,
            defines: HashMap::new(),
            claims: HashMap::new(),
            compositions: HashMap::new(),
        };
        for (i, o) in objs.iter().enumerate() {
            if let Some(kind) = &o.facts.xp.defines {
                t.defines.entry(kind).or_default().push(i);
            }
            if let Some(kind) = &o.facts.xp.claim {
                t.claims.entry(kind).or_default().push(i);
            }
            if own(o, "Composition") {
                t.compositions.entry(&o.orig).or_default().push(i);
            }
        }
        t
    }

    /// The first `OBJECT_REFERENCES_MAX` objects `map` holds under `key`.
    fn get<K: std::hash::Hash + Eq + ?Sized>(map: &HashMap<&K, Vec<usize>>, key: &K) -> Vec<usize> {
        let found = map.get(key).into_iter().flatten();
        found.take(OBJECT_REFERENCES_MAX).copied().collect()
    }

    /// The objects `o`, whose role is `role`, references, each with how it names it.
    pub(super) fn of(&self, o: &Obj, role: Option<Role>, out: &mut BTreeSet<(usize, String)>) {
        let xp = &o.facts.xp;
        let kind = (o.group.clone(), o.kind.clone());
        if role == Some(Role::Claim) {
            for j in Self::get(&self.claims, &kind) {
                // The XRD's composite kind, which the claim stands for.
                let composite = self.composite_of(j);
                out.insert((j, format!("composite {composite}")));
            }
        }
        if matches!(role, Some(Role::Composite | Role::Managed)) {
            for j in Self::get(&self.defines, &kind) {
                out.insert((j, format!("kind {}", shown(&kind))));
            }
        }
        let composes = matches!(role, Some(Role::Claim | Role::Composite));
        for name in xp.composition.iter().filter(|_| composes) {
            for j in Self::get(&self.compositions, name.as_str()) {
                out.insert((j, "compositionRef".into()));
            }
        }
        for (key, name) in &xp.compositions {
            for j in Self::get(&self.compositions, name.as_str()) {
                out.insert((j, (*key).into()));
            }
        }
        if let Some(kind) = &xp.composes {
            for j in Self::get(&self.defines, kind) {
                out.insert((j, format!("compositeTypeRef {}", shown(kind))));
            }
        }
    }

    /// The composite kind the XRD at `j` defines, as shown.
    fn composite_of(&self, j: usize) -> String {
        self.objs[j]
            .facts
            .xp
            .defines
            .as_ref()
            .map_or_else(String::new, shown)
    }

    /// The objects the resource `base` of a composition references: the definitions of its kind.
    pub(super) fn of_base(&self, base: &Base) -> Vec<(usize, String)> {
        (Self::get(&self.defines, &base.kind).into_iter())
            .map(|j| (j, format!("kind {}", shown(&base.kind))))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Output, evaluate};
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
        (list.iter())
            .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
            .collect()
    }

    /// Each node as `<address> <type> <name>[ (pattern)] <role>`, sorted.
    fn nodes(out: &Output) -> Vec<String> {
        let mut n: Vec<String> = (out.nodes.iter())
            .map(|n| {
                let pattern = if n.pattern { " (pattern)" } else { "" };
                let name = n.name.as_deref().unwrap_or("-");
                let role = n.role.as_deref().unwrap_or("-");
                format!("{} {} {name}{pattern} {role}", n.address, n.ty)
            })
            .collect();
        n.sort();
        n
    }

    fn shown(out: &Output) -> Vec<String> {
        let mut r: Vec<String> = (out.references.iter())
            .map(|r| {
                format!(
                    "{} -> {} via {}",
                    r.source,
                    r.target,
                    r.via.as_deref().unwrap_or("")
                )
            })
            .collect();
        r.sort();
        r
    }

    const XRD: &str = "apiVersion: apiextensions.crossplane.io/v1\nkind: CompositeResourceDefinition\n\
        metadata:\n  name: xdbs.acme.io\n  namespace: ignored\nspec:\n  group: acme.io\n  names:\n    \
        kind: XDB\n    plural: xdbs\n  claimNames:\n    kind: DB\n    plural: dbs\n  \
        defaultCompositionRef:\n    name: xdbs.aws\n";

    const COMPOSITION: &str = "apiVersion: apiextensions.crossplane.io/v1\nkind: Composition\n\
        metadata:\n  name: xdbs.aws\nspec:\n  compositeTypeRef:\n    apiVersion: acme.io/v1\n    \
        kind: XDB\n  resources:\n    - name: bucket\n      base:\n        \
        apiVersion: s3.aws.upbound.io/v1beta1\n        kind: Bucket\n        metadata:\n          \
        name: literal-bucket\n    - base:\n        apiVersion: sqs.aws.upbound.io/v1beta1\n        \
        kind: Queue\n";

    #[test]
    fn definitions_compositions_claims_and_managed_resources_are_recognized() {
        let out = evaluate(&files(&[
            ("xp/xrd.yaml", XRD),
            ("xp/composition.yaml", COMPOSITION),
            (
                "xp/objects.yaml",
                "apiVersion: acme.io/v1\nkind: DB\nmetadata:\n  name: orders\n  namespace: shop\n---\n\
                 apiVersion: acme.io/v1\nkind: XDB\nmetadata:\n  name: orders-x\n---\n\
                 apiVersion: s3.aws.upbound.io/v1beta1\nkind: Bucket\nmetadata:\n  name: assets\n  \
                 annotations:\n    crossplane.io/external-name: acme-assets\nspec:\n  forProvider:\n    \
                 region: eu-west-1\n---\n\
                 apiVersion: other.io/v1\nkind: DB\nmetadata:\n  name: unrelated\n  \
                 annotations:\n    crossplane.io/external-name: not-managed\n",
            ),
        ]));
        assert_eq!(
            nodes(&out),
            [
                "Bucket.s3.aws.upbound.io/assets Bucket acme-assets crossplane managed resource",
                "CompositeResourceDefinition.apiextensions.crossplane.io/xdbs.acme.io \
                 CompositeResourceDefinition xdbs.acme.io crossplane composite resource definition",
                "Composition.apiextensions.crossplane.io/xdbs.aws Composition xdbs.aws \
                 crossplane composition",
                "Composition.apiextensions.crossplane.io/xdbs.aws/1 Queue * (pattern) \
                 crossplane composed resource",
                "Composition.apiextensions.crossplane.io/xdbs.aws/bucket Bucket literal-bucket \
                 crossplane composed resource",
                "DB.acme.io/shop/orders DB orders crossplane claim",
                "DB.other.io/unrelated DB unrelated -",
                "XDB.acme.io/orders-x XDB orders-x crossplane composite resource",
            ]
        );
    }

    #[test]
    fn a_claim_reaches_its_composite_kind_and_a_composition_its_type_and_resources() {
        let out = evaluate(&files(&[
            ("xp/xrd.yaml", XRD),
            ("xp/composition.yaml", COMPOSITION),
            (
                "xp/claim.yaml",
                "apiVersion: acme.io/v1\nkind: DB\nmetadata:\n  name: orders\n  namespace: shop\n\
                 spec:\n  compositionRef:\n    name: xdbs.aws\n",
            ),
            (
                "xp/crd.yaml",
                "apiVersion: apiextensions.k8s.io/v1\nkind: CustomResourceDefinition\nmetadata:\n  \
                 name: buckets.s3.aws.upbound.io\nspec:\n  group: s3.aws.upbound.io\n  names:\n    \
                 kind: Bucket\n",
            ),
        ]));
        let xrd = "CompositeResourceDefinition.apiextensions.crossplane.io/xdbs.acme.io";
        let comp = "Composition.apiextensions.crossplane.io/xdbs.aws";
        let crd = "CustomResourceDefinition.apiextensions.k8s.io/buckets.s3.aws.upbound.io";
        assert_eq!(
            shown(&out),
            [
                format!("{xrd} -> {comp} via defaultCompositionRef"),
                format!("{comp} -> {xrd} via compositeTypeRef XDB.acme.io"),
                format!("{comp} -> {comp}/1 via resources"),
                format!("{comp} -> {comp}/bucket via resources"),
                format!("{comp}/bucket -> {crd} via kind Bucket.s3.aws.upbound.io"),
                format!("DB.acme.io/shop/orders -> {xrd} via composite XDB.acme.io"),
                format!("DB.acme.io/shop/orders -> {comp} via compositionRef"),
            ]
        );
    }

    #[test]
    fn a_name_a_patch_or_a_patch_set_writes_is_a_pattern_and_an_outward_patch_is_not() {
        let resource = |name: &str, patches: &str| {
            format!(
                "    - name: {name}\n      base:\n        apiVersion: s3.aws.upbound.io/v1beta1\n        \
                 kind: Bucket\n        metadata:\n          name: {name}-literal\n      \
                 patches:\n{patches}"
            )
        };
        let text = format!(
            "apiVersion: apiextensions.crossplane.io/v1\nkind: Composition\nmetadata:\n  name: c\n\
             spec:\n  patchSets:\n    - name: ext\n      patches:\n        - fromFieldPath: a\n          \
             toFieldPath: metadata.annotations[\"crossplane.io/external-name\"]\n  resources:\n{}{}{}{}{}{}",
            resource(
                "direct",
                "        - fromFieldPath: metadata.uid\n          toFieldPath: metadata.name\n"
            ),
            resource(
                "set",
                "        - type: PatchSet\n          patchSetName: ext\n"
            ),
            resource(
                "outward",
                "        - type: ToCompositeFieldPath\n          fromFieldPath: metadata.name\n          \
                 toFieldPath: metadata.name\n",
            ),
            resource(
                "other",
                "        - fromFieldPath: spec.size\n          toFieldPath: spec.forProvider.size\n"
            ),
            // With no `toFieldPath`, a patch writes the path it reads.
            resource(
                "copied",
                "        - fromFieldPath: metadata.annotations[crossplane.io/external-name]\n"
            ),
            resource(
                "combined",
                "        - type: CombineFromComposite\n          toFieldPath: metadata.name\n"
            ),
        );
        let out = evaluate(&files(&[("c.yaml", &text)]));
        let names: Vec<String> = nodes(&out)
            .into_iter()
            .filter(|n| n.contains("composed"))
            .collect();
        assert_eq!(
            names,
            [
                "Composition.apiextensions.crossplane.io/c/combined Bucket * (pattern) crossplane composed resource",
                "Composition.apiextensions.crossplane.io/c/copied Bucket * (pattern) crossplane composed resource",
                "Composition.apiextensions.crossplane.io/c/direct Bucket * (pattern) crossplane composed resource",
                "Composition.apiextensions.crossplane.io/c/other Bucket other-literal crossplane composed resource",
                "Composition.apiextensions.crossplane.io/c/outward Bucket outward-literal crossplane composed resource",
                "Composition.apiextensions.crossplane.io/c/set Bucket * (pattern) crossplane composed resource",
            ]
        );
    }

    #[test]
    fn an_enforced_composition_and_a_composite_or_v2_composition_ref_are_references() {
        let xrd = XRD.replace("defaultCompositionRef", "enforcedCompositionRef");
        let out = evaluate(&files(&[
            ("xp/xrd.yaml", xrd.as_str()),
            ("xp/composition.yaml", COMPOSITION),
            (
                "xp/objects.yaml",
                "apiVersion: acme.io/v1\nkind: XDB\nmetadata:\n  name: x1\nspec:\n  \
                 compositionRef:\n    name: xdbs.aws\n---\napiVersion: acme.io/v1\nkind: XDB\n\
                 metadata:\n  name: x2\n  namespace: shop\nspec:\n  crossplane:\n    \
                 compositionRef:\n      name: xdbs.aws\n",
            ),
        ]));
        let comp = "Composition.apiextensions.crossplane.io/xdbs.aws";
        let refs: Vec<String> = shown(&out)
            .into_iter()
            .filter(|r| r.ends_with("CompositionRef") || r.ends_with("compositionRef"))
            .collect();
        assert_eq!(
            refs,
            [
                format!(
                    "CompositeResourceDefinition.apiextensions.crossplane.io/xdbs.acme.io -> {comp} \
                     via enforcedCompositionRef"
                ),
                format!("XDB.acme.io/shop/x2 -> {comp} via compositionRef"),
                format!("XDB.acme.io/x1 -> {comp} via compositionRef"),
            ]
        );
    }

    #[test]
    fn a_pipeline_reads_the_resources_of_patch_and_transform_and_no_other_function() {
        let out = evaluate(&files(&[(
            "p.yaml",
            "apiVersion: apiextensions.crossplane.io/v1\nkind: Composition\nmetadata:\n  name: p\n\
             spec:\n  mode: Pipeline\n  pipeline:\n    - step: pt\n      input:\n        \
             apiVersion: pt.fn.crossplane.io/v1beta1\n        kind: Resources\n        resources:\n          \
             - name: q\n            base:\n              apiVersion: sqs.aws.upbound.io/v1beta1\n              \
             kind: Queue\n              metadata:\n                name: events\n    - step: other\n      \
             input:\n        apiVersion: kcl.fn.crossplane.io/v1beta1\n        kind: KCLInput\n        \
             resources:\n          - name: hidden\n            base:\n              apiVersion: v1\n              \
             kind: ConfigMap\n",
        )]));
        assert_eq!(
            nodes(&out),
            [
                "Composition.apiextensions.crossplane.io/p Composition p crossplane composition",
                "Composition.apiextensions.crossplane.io/p/q Queue events crossplane composed resource",
            ]
        );
        assert_eq!(
            shown(&out),
            [
                "Composition.apiextensions.crossplane.io/p -> Composition.apiextensions.crossplane.io/p/q via pipeline pt"
            ]
        );
        // The resource's lines and block are its own within the composition.
        let q = out.nodes.iter().find(|n| n.ty == "Queue").unwrap();
        assert_eq!((q.line, q.end_line), (13, 18));
        assert_eq!(q.block, "spec.pipeline[0].input.resources[0]");
    }

    #[test]
    fn a_claim_away_from_its_definition_keeps_its_role_and_references_nothing() {
        let out = evaluate(&files(&[
            ("defs/xrd.yaml", XRD),
            (
                "apps/claim.yaml",
                "apiVersion: acme.io/v1\nkind: DB\nmetadata:\n  name: orders\n  namespace: shop\n",
            ),
        ]));
        assert!(nodes(&out).contains(&"DB.acme.io/shop/orders DB orders crossplane claim".into()));
        assert!(out.references.is_empty());
    }

    #[test]
    fn crossplane_kinds_have_no_namespace_even_under_an_overlay() {
        let out = evaluate(&files(&[
            (
                "o/kustomization.yaml",
                "resources:\n  - xrd.yaml\n  - comp.yaml\nnamespace: dev\nnamePrefix: dev-\n",
            ),
            ("o/xrd.yaml", XRD),
            ("o/comp.yaml", COMPOSITION),
        ]));
        let addresses: Vec<&str> = out.nodes.iter().map(|n| n.address.as_str()).collect();
        assert!(
            addresses.contains(
                &"CompositeResourceDefinition.apiextensions.crossplane.io/dev-xdbs.acme.io"
            )
        );
        assert!(addresses.contains(&"Composition.apiextensions.crossplane.io/dev-xdbs.aws/bucket"));
        // The reference matches the composition's name as written, before the prefix.
        assert!(
            shown(&out)
                .iter()
                .any(|r| r.ends_with("via defaultCompositionRef"))
        );
    }

    #[test]
    fn a_composition_past_the_resource_ceiling_creates_the_first_ones() {
        let resources: String = (0..COMPOSED_MAX + 5)
            .map(|i| {
                format!(
                    "    - name: r{i:03}\n      base:\n        apiVersion: s3.aws.upbound.io/v1beta1\n        \
                     kind: Bucket\n"
                )
            })
            .collect();
        let out = evaluate(&files(&[
            ("xrd.yaml", XRD),
            (
                "c.yaml",
                &format!(
                    "apiVersion: apiextensions.crossplane.io/v1\nkind: Composition\nmetadata:\n  \
                     name: c\nspec:\n  compositeTypeRef:\n    apiVersion: acme.io/v1\n    kind: XDB\n  \
                     resources:\n{resources}"
                ),
            ),
        ]));
        let composed = out.nodes.iter().filter(|n| n.ty == "Bucket");
        assert_eq!(composed.count(), COMPOSED_MAX);
        // Every resource created is a reference of the composition past the object ceiling,
        // which its `compositeTypeRef` already takes one of.
        const { assert!(COMPOSED_MAX >= super::super::OBJECT_REFERENCES_MAX) };
        let of = |source: &str| out.references.iter().filter(|r| r.source == source).count();
        assert_eq!(
            of("Composition.apiextensions.crossplane.io/c"),
            COMPOSED_MAX + 1
        );
        assert!(
            out.nodes
                .iter()
                .all(|n| n.address != "Composition.apiextensions.crossplane.io/c/r064")
        );
    }
}
