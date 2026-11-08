//! External constructions followed through TypeScript constant bindings.
use crate::resolve::{Code, Follow, Form, Of};

/// A client's factory: the package it is imported from, the factory as JavaScript would write it
/// (`new Kafka().producer()`), and the constant the client is bound to (its file and qualified
/// name).
pub(crate) struct Factory<'a> {
    pub package: &'a str,
    pub expr: String,
    pub binding: Option<(&'a str, &'a str)>,
}

/// The `Factory` of `chain`, from its `Constants` origin: `None` unless it comes from an import
/// of something outside the repository.
pub(crate) fn factory<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    chain: &'a str,
) -> Option<Factory<'a>> {
    let found = code.origins(path, scope, Of::Name(chain), Follow::Constants);
    let origin = found.into_iter().next()?;
    let (package, name) = origin.outside?.imported?;
    let route = origin.routes.into_iter().next()?;
    let mut expr = match origin.form {
        Form::New => format!("new {name}()"),
        Form::Call => format!("{name}()"),
        Form::Type | Form::Name => name,
    };
    for step in &route.steps {
        expr += &format!(".{step}()");
    }
    Some(Factory {
        package,
        expr,
        binding: route.held,
    })
}

/// The outside construction of a method receiver and its first constructed argument.
/// The raw argument is checked before expansion so its original scope and imports survive.
pub(crate) fn constructor_argument<'a>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
) -> Option<(Factory<'a>, &'a str, String)> {
    let call = code.call(path, index)?;
    let (receiver, _) = call.callee.rsplit_once('.')?;
    let client = factory(code, path, &call.scope, receiver)?;
    let argument = code.args(path, index)?.first()?;
    let origins = code.origins(path, &call.scope, Of::Value(argument), Follow::Constants);
    let origin = origins.into_iter().next().filter(|o| o.form == Form::New)?;
    let (package, constructor) = origin.outside?.imported?;
    Some((client, package, constructor))
}

/// Expands the object options of the first constructed argument, preserving wrapper sites. A
/// field assigned after the object is made, before the call, is unknown.
pub(crate) fn constructor_options<'a, T>(
    code: &Code<'a>,
    path: &'a str,
    index: usize,
    read: &impl Fn(&[(String, crate::lang::Value)]) -> Option<T>,
) -> Vec<crate::resolve::Expanded<'a, T>> {
    use crate::lang::Value;
    let writes = crate::contract::sdk::mutations::writes;
    code.expand_written(path, index, writes, &|args, _| {
        let Some(Value::New(_, args)) = args.first() else {
            return read(&[]);
        };
        match args.first() {
            Some(Value::Object(fields)) => read(fields),
            Some(Value::Ref(_)) => None,
            _ => read(&[]),
        }
    })
}
