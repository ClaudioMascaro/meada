//! Environment and configuration reads in the URLs of C# consumers (spec #28, #112). The pack
//! writes what looks like a read, without knowing what it reads: a call or a subscript with one
//! string literal (`Part::Read`, or a `Value::Call` the kind turns into one). This module decides.
//!
//! - `Environment.GetEnvironmentVariable("X")`, with `Environment` the framework's
//!   `System.Environment` (a type of the repository with that name hides it), is the variable
//!   `X` (`Part::Env`), also through a local or a field it is bound to.
//! - `configuration["Section:Key"]` on a receiver declared `IConfiguration`, or on
//!   `builder.Configuration` with `builder` a `WebApplicationBuilder` (its declared type, or made
//!   by `WebApplication.CreateBuilder` and its siblings), names a key of the project's base
//!   `appsettings.json` (`dotnet::appsettings`). The key maps to the variable `Section__Key`, as
//!   .NET's environment provider names it, and a literal the file holds for it is that
//!   variable's default (`Part::EnvOr`): `http::fetches` then links by the literal's host when a
//!   provider of the workspace serves it, and otherwise through the variable and `base_url_env`.
//!   A key the file lacks is the variable alone.
//!
//! Any other call or subscript (`GetValue<T>("k")`, `GetSection("s")`, `connectionStrings["x"]`)
//! is no read: an unknown part. Meada never reads an environment value.
//!
//! The variable keeps the spelling of the key as the code writes it, and `base_url_env` compares
//! it as written: .NET compares keys without case, Meada's peers do not.
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

/// The reads of one extraction; the configuration files are parsed the first time one asks.
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

    /// `value`, written in `path`, with the reads in a string decided (each read carries the
    /// scope it was written in).
    pub(super) fn value<'a>(&self, code: &Code<'a>, path: &'a str, value: Value) -> Value {
        match value {
            Value::Str(parts) => {
                let read = |p| self.part(code, path, p);
                Value::Str(parts.into_iter().flat_map(read).collect())
            }
            other => other,
        }
    }

    /// What a part is: a read decided, anything else as it is.
    fn part<'a>(&self, code: &Code<'a>, path: &'a str, part: Part) -> Vec<Part> {
        match part {
            Part::Read { scope, callee, key } => self.read(code, (path, &scope), &callee, &key),
            part => vec![part],
        }
    }

    /// The parts the call or subscript `callee` with the literal `key`, written in `scope` of
    /// `path`, reads (see the module).
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

/// The scope written as `scope` among the file's definitions (`""` is the file's), as the
/// resolver reads it.
fn scope_of<'a>(facts: &'a Facts, scope: &str) -> Option<&'a str> {
    match scope.is_empty() {
        true => Some(""),
        false => (facts.defs.iter().map(|d| d.name.as_str())).find(|n| *n == scope),
    }
}

/// The name `name` as the file writes it for a field, a local or a parameter.
fn name_in<'a>(facts: &'a Facts, name: &str) -> Option<&'a str> {
    let params = facts.defs.iter().flat_map(|d| &d.params);
    (facts.receivers.iter().map(|r| r.name.as_str()))
        .chain(params.map(|p| p.name.as_str()))
        .find(|n| *n == name)
}

/// Whether `receiver`, written in `scope` of `path`, is a configuration: a name declared
/// `IConfiguration`, or `builder.Configuration` of a web application builder.
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

/// Whether the type the source writes for `name` in `scope` of `path` is `full`, in full or
/// through the usings.
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

/// Whether `name` is a web application builder: declared as one, or a local made by
/// `WebApplication.CreateBuilder(..)` or a sibling.
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
