use std::collections::HashMap;

use crate::lang::Value;
use crate::resolve::{Code, Follow, Form, Of};

#[derive(Clone)]
pub(crate) struct Factory<'a> {
    pub package: &'a str,
    pub expr: String,
    pub binding: Option<(&'a str, &'a str)>,
}

pub(crate) fn factory<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    chain: &'a str,
) -> Option<Factory<'a>> {
    factory_of(code, path, scope, Of::Name(chain))
}

pub(crate) fn factory_of<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    of: Of<'a>,
) -> Option<Factory<'a>> {
    let found = code.origins(path, scope, of, Follow::Constants);
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

pub(crate) fn field_factory<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    chain: &'a str,
) -> Option<Factory<'a>> {
    field_factory_in(code, path, scope, chain, &mut HashMap::new())
}

fn field_factory_in<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    chain: &'a str,
    known: &mut HashMap<&'a str, Option<Factory<'a>>>,
) -> Option<Factory<'a>> {
    let field = chain.strip_prefix("this.").filter(|f| !f.contains('.'))?;
    let held = code.field_writes(path).iter().find(|w| {
        let (class, name) = w.name.rsplit_once('.').unwrap_or(("", &w.name));
        let within = |r: &str| r.is_empty() || r.starts_with('.');
        name == field && scope.strip_prefix(class).is_some_and(within)
    })?;
    if let Some(answer) = known.get(held.name.as_str()) {
        return answer.clone();
    }
    known.insert(&held.name, None);
    let answer = {
        let mut made = (held.values.iter()).map(|(at, value)| {
            factory_of(code, path, at, Of::Value(value))
                .or_else(|| member_call(code, path, at, value, known))
        });
        let first = made.next().flatten();
        first.filter(|first| {
            made.all(|m| m.is_some_and(|m| m.package == first.package && m.expr == first.expr))
        })
    };
    let answer = answer.map(|first| Factory {
        binding: Some((path, held.name.as_str())),
        ..first
    });
    known.insert(&held.name, answer.clone());
    answer
}

fn member_call<'a>(
    code: &Code<'a>,
    path: &'a str,
    scope: &'a str,
    value: &'a Value,
    known: &mut HashMap<&'a str, Option<Factory<'a>>>,
) -> Option<Factory<'a>> {
    let Value::Call(call) = value else {
        return None;
    };
    let (receiver, method) = call.callee.rsplit_once('.')?;
    let mut made = field_factory_in(code, path, scope, receiver, known)?;
    made.expr = format!("{}.{method}()", made.expr);
    Some(made)
}

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
