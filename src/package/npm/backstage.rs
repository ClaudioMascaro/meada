//! The Backstage registrars of npm (`Npm::carried`): a backend that
//! `@backstage/backend-defaults`' `createBackend` makes adds a plugin by
//! `<b>.add(import('<package>'))` (`added_plugin`), and `@backstage/backend-test-utils`'
//! `startTestBackend` starts with `features: [import('<package>')]` (`test_features`).

use crate::lang::ts::dynamic_import;
use crate::lang::{Call, Value};
use crate::resolve::{Code, Follow, Form, Of};

/// Whether the one name `name` in `scope` of the file `path` holds exactly the call of `export`,
/// `(package, name)`, and nothing done to its result: one origin, a call, reached through no
/// step.
///
/// `Follow::Constants`, not `Values`: a parameter fed the call by every caller, or a local
/// wrapper's return, would also count, and each may be handed another value. It spends no
/// `EVAL_BUDGET` and goes at most `EVAL_DEPTH` constants deep.
fn made_by<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    name: &'a str,
    export: (&str, &str),
) -> bool {
    let origins = code.origins(path, scope, Of::Name(name), Follow::Constants);
    let [origin] = origins.as_slice() else {
        return false;
    };
    let made = (origin.outside.as_ref())
        .and_then(|o| o.imported.as_ref())
        .map(|(package, name)| (*package, name.as_str()));
    origin.form == Form::Call
        && made == Some(export)
        && origin.routes.iter().all(|r| r.steps.is_empty())
}

/// The bare specifier a Backstage backend adds by `<b>.add(import('<specifier>'))`, whose
/// `default` export `add` takes as the feature. `<b>` must be one name that holds exactly the call
/// of `@backstage/backend-defaults`' `createBackend` (`made_by`: a `const`, no method called on
/// the result), and the argument a plain literal `import()` (`dynamic_import`).
///
/// The arguments come through `Code::args` because the facts `extract` walks hold no values. Its
/// `None` is a miss, not an answer: the call's file may not be read in full yet (when the receiver's
/// `const` is in another file, `origins` reads that one first), and the index parses it again
/// (`src/index.rs`, the reparse loop). The two filters on the callee read nothing and only spare
/// `origins` the calls that cannot match (`a.b.add`, `(call_expression).add`); the channel kind
/// already pays the same `origins` read for every `.add` call (`channel/ts.rs`).
pub(super) fn added_plugin<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    call: &'a Call,
) -> Option<String> {
    let receiver = call.callee.strip_suffix(".add")?;
    if receiver.is_empty() || receiver.contains('.') || receiver.starts_with('(') {
        return None;
    }
    let backend = ("@backstage/backend-defaults", "createBackend");
    if !made_by(code, path, &call.scope, receiver, backend) {
        return None;
    }
    let [argument] = code.args(path, index)? else {
        return None;
    };
    dynamic_import(argument).map(str::to_string)
}

/// The bare specifiers of the `import()` elements of `features` a `startTestBackend` of
/// `@backstage/backend-test-utils` is handed, `startTestBackend({ features: [import('x')] })`:
/// each feature is the module's `default`. The callee is the import itself (`Code::imported`),
/// never a name match; an element that is no plain literal `import()` (`dynamic_import`) gives
/// nothing. The arguments come through `Code::args`, as `added_plugin`'s.
pub(super) fn test_features<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    call: &'a Call,
) -> Vec<String> {
    let started = code.imported(path, &call.scope, &call.callee);
    if started.as_ref().map(|(p, n)| (*p, n.as_str()))
        != Some(("@backstage/backend-test-utils", "startTestBackend"))
    {
        return Vec::new();
    }
    let Some([Value::Object(fields)]) = code.args(path, index) else {
        return Vec::new();
    };
    let features = fields.iter().find(|(key, _)| key == "features");
    let Some((_, Value::List(items))) = features else {
        return Vec::new();
    };
    (items.iter())
        .filter_map(|item| dynamic_import(item).map(str::to_string))
        .collect()
}
