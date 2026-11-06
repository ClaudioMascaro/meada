//! External constructors and one level of factories followed through Python values.
use crate::lang::Call;
use crate::resolve::{Code, Follow, Of};
const CALL: &str = "(call)";

/// A call that made a client, with the name its imports give what it called, such as
/// `pika.BlockingConnection.channel`.
pub(crate) struct Maker<'a> {
    pub name: String,
    pub call: &'a Call,
}

/// The calls that made the client `form` is called on, written in `path`: the constructors and
/// factories of a package outside the repository that its receiver traces back to, or the
/// receiver itself when it is a call written in place. A factory (a connection's `channel()`, a
/// session's `client()`) is named after the constructor of the client it is called on, a level
/// deep.
// ponytail: only one level of client factories is followed.
pub(crate) fn receivers<'a>(
    code: &Code<'a>,
    path: &'a str,
    form: &'a Call,
    through: bool,
    factories: &[&str],
) -> Vec<Maker<'a>> {
    let receiver = form.callee.rsplit_once('.').map_or("", |(r, _)| r);
    let made: Vec<(&'a str, &'a Call)> = if receiver == CALL {
        head(code, path, form)
            .map(|call| (path, call))
            .into_iter()
            .collect()
    } else {
        (code
            .origins(
                path,
                &form.scope,
                Of::Name(receiver),
                Follow::Values {
                    passes: crate::resolve::passes_nothing,
                },
            )
            .iter())
        .filter_map(|o| o.call)
        .filter_map(|(at, i)| Some((at, code.call(at, i)?)))
        .collect()
    };
    (made.into_iter())
        .flat_map(|(at, call)| made_by(code, at, call, through, factories))
        .collect()
}

/// The client the call `call` made, written in `at`: a constructor of a package outside the
/// repository, or a factory on a client that one made.
fn made_by<'a>(
    code: &Code<'a>,
    at: &'a str,
    call: &'a Call,
    through: bool,
    factories: &[&str],
) -> Vec<Maker<'a>> {
    if let Some(name) = imported(code, at, &call.scope, &call.callee) {
        return vec![Maker { name, call }];
    }
    let method = call.callee.rsplit_once('.').map(|(_, method)| method);
    match method {
        Some(method) if through && factories.contains(&method) => {
            (receivers(code, at, call, false, factories).into_iter())
                .map(|made| Maker {
                    name: format!("{}.{method}", made.name),
                    call,
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The call that `form`, a method called on a call written in place (`X(...).method()`), is
/// called on: the last to end of the calls that start where `form` does, inside it.
fn head<'a>(code: &Code<'a>, path: &'a str, form: &Call) -> Option<&'a Call> {
    let calls = &code.files()[path].calls;
    // Packs emit an enclosing call before its receiver. The next same-start call is
    // therefore its longest proper receiver, without scanning the file per SDK call.
    let at = code.call_index(path, &form.range)? + 1;
    let receiver = calls.get(at)?;
    (receiver.range.start == form.range.start && receiver.range.end < form.range.end)
        .then(|| code.call(path, at))
        .flatten()
}

/// What `callee`, written in `scope` of `path`, names in a package outside the repository,
/// spelled from the package's import: `pika.BlockingConnection` however it is imported. A
/// method of a local (`connection.channel`) is none.
pub(crate) fn imported<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    callee: &'a str,
) -> Option<String> {
    let (package, spelled) = code.imported(path, scope, callee)?;
    Some(format!("{package}.{spelled}"))
}

/// The unanimous literal argument of known external factories. Unknown constructors or
/// disagreeing services cannot identify the receiver.
pub(crate) fn factory_argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    call: &'a Call,
    constructors: &[&str],
    factories: &[&str],
    keyword: &str,
) -> Option<String> {
    use crate::lang::{Value, literal};
    let makers = receivers(code, path, call, true, factories);
    let argument = |maker: &Maker| {
        if !constructors.contains(&maker.name.as_str()) {
            return None;
        }
        match maker
            .call
            .args
            .first()
            .or_else(|| maker.call.keyword(keyword))
        {
            Some(Value::Str(parts)) => literal(parts),
            _ => None,
        }
    };
    let first = makers.first().and_then(&argument)?;
    makers
        .iter()
        .all(|m| argument(m).as_deref() == Some(first.as_str()))
        .then_some(first)
}
