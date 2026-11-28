// ponytail: the receiver and the scope of a read are looked up in the file that holds the
// consumer, so a read passed in from another file's wrapper call is an unknown part.

use std::cell::OnceCell;

use super::minimal::{APP, BUILDERS};
use crate::dotnet::appsettings::Settings;
use crate::lang::{Facts, Part, Value};
use crate::names::{is_type, one, spellings};
use crate::resolve::Code;

const ENVIRONMENT: &str = "System.Environment";
const GET: &str = "GetEnvironmentVariable";
const CONFIGURATION: &str = "Microsoft.Extensions.Configuration.IConfiguration";
const BUILDER: &str = "Microsoft.AspNetCore.Builder.WebApplicationBuilder";

pub(super) struct Reads<'i> {
    configs: &'i [(&'i str, &'i [u8])],
    settings: OnceCell<Settings>,
}

impl<'i> Reads<'i> {
    pub(super) fn new(configs: &'i [(&'i str, &'i [u8])]) -> Self {
        Self {
            configs,
            settings: OnceCell::new(),
        }
    }

    pub(super) fn value<'a>(&self, code: &Code<'a>, path: &'a str, value: Value) -> Value {
        match value {
            Value::Str(parts) => {
                let read = |p| self.part(code, path, p);
                Value::Str(parts.into_iter().flat_map(read).collect())
            }
            other => other,
        }
    }

    fn part<'a>(&self, code: &Code<'a>, path: &'a str, part: Part) -> Vec<Part> {
        match part {
            pending @ Part::Read {
                parameter: true, ..
            } => vec![pending],
            Part::Read {
                scope,
                callee,
                key,
                then,
                ..
            } if then.is_empty() => self.read(code, (path, &scope), &callee, &key),
            Part::Read { .. } => vec![Part::Hole],
            part => vec![part],
        }
    }

    fn read<'a>(
        &self,
        code: &Code<'a>,
        (path, scope): (&'a str, &str),
        callee: &str,
        key: &str,
    ) -> Vec<Part> {
        let unknown = vec![Part::Hole];
        let facts = code.files()[path];
        let Some(scope) = scope_of(facts, scope) else {
            return unknown;
        };
        if key.is_empty() {
            return unknown;
        }
        if let Some(receiver) = callee.strip_suffix("[]") {
            if !configuration(code, (path, scope), receiver) {
                return unknown;
            }
            let variable = key.replace(':', "__");
            let settings = self.settings.get_or_init(|| Settings::read(self.configs));
            let literal = settings.get(&facts.unit, key);
            return match literal.filter(|v| !v.is_empty()) {
                Some(value) => vec![Part::EnvOr(variable, vec![Part::Text(value.to_string())])],
                None => vec![Part::Env(variable)],
            };
        }
        let environment = match callee.rsplit_once('.') {
            Some((ty, GET)) if ty == ENVIRONMENT => true,
            Some(("Environment", GET)) => one(&spellings(code, path, scope, "Environment"), |s| {
                s == ENVIRONMENT
            })
            .is_some(),
            _ => false,
        };
        match environment {
            true => vec![Part::Env(key.to_string())],
            false => unknown,
        }
    }
}

fn scope_of<'a>(facts: &'a Facts, scope: &str) -> Option<&'a str> {
    match scope.is_empty() {
        true => Some(""),
        false => (facts.defs.iter().map(|d| d.name.as_str())).find(|n| *n == scope),
    }
}

fn name_in<'a>(facts: &'a Facts, name: &str) -> Option<&'a str> {
    let params = facts.defs.iter().flat_map(|d| &d.params);
    (facts.receivers.iter().map(|r| r.name.as_str()))
        .chain(params.map(|p| p.name.as_str()))
        .find(|n| *n == name)
}

fn configuration<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), receiver: &str) -> bool {
    let receiver = receiver.strip_prefix("this.").unwrap_or(receiver);
    let facts = code.files()[path];
    match receiver.split_once('.') {
        None => name_in(facts, receiver)
            .is_some_and(|name| declared(code, (path, scope), name, CONFIGURATION)),
        Some((head, "Configuration")) => {
            name_in(facts, head).is_some_and(|name| builder(code, (path, scope), name))
        }
        Some(_) => false,
    }
}

fn declared<'a>(
    code: &Code<'a>,
    (path, scope): (&'a str, &'a str),
    name: &'a str,
    full: &str,
) -> bool {
    let Some((at, within, ty)) = code.written(path, scope, name) else {
        return false;
    };
    is_type(code, at, within, ty, full)
}

fn builder<'a>(code: &Code<'a>, (path, scope): (&'a str, &'a str), name: &'a str) -> bool {
    if code.written(path, scope, name).is_some() {
        return declared(code, (path, scope), name, BUILDER);
    }
    let Some(bound) = code.local(path, scope, name) else {
        return false;
    };
    let Value::Call(call) = &bound.value else {
        return false;
    };
    let spelled = spellings(code, path, &bound.within, &call.callee);
    one(&spelled, |s| {
        s.strip_prefix(APP).is_some_and(|n| BUILDERS.contains(&n))
    })
    .is_some()
}
