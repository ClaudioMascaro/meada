//! TypeScript and JavaScript consumers (spec #23): `fetch(url, init)`, only in a pack whose
//! globals bind `fetch` (also a call to a parameter defaulting to `fetch`). A client wrapper is
//! read at each of its call sites through `Code::expand`; a URL whose base is Backstage's
//! `getBaseUrl('<id>')` starts at `/api/<id>` (`backstage`), one through `/api/proxy/<ep>` goes to
//! the target of the endpoint `app-config*.yaml` gives (`config`), and the shape the consumer reads
//! of the response is the field paths `response_reads` reads over the frames climbed. Seams of
//! the root reused: its `fetch(args)` reader (`locate`, `method`, `fragments`) and `consumer`.

use super::super::join::{depends, opaque_lead, resolve, text_parts};
use super::super::{consumer, fetch};
use super::backstage;
use super::config::Config;
use crate::contract::Found;
use crate::declared::{self, Declared, Types};
use crate::lang::ts::GLOBAL_URL;
use crate::lang::{Part, Step, Ty, Value};
use crate::literals::Literals;
use crate::resolve::{Code, Expand, Expanded, Frame, Target};
use crate::shape::{self, Import, Shape, Source, under};

/// `fetch` consumers with the shape each reads of its response (spec #23).
pub(in crate::http) fn consumers(
    code: &Code,
    config: &Config,
    literals: &mut Literals,
) -> Vec<Found> {
    let mut found = Vec::new();
    // `fetch` is a consumer only where the language binds it everywhere.
    let global = code.binds_everywhere("fetch");
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let is_fetch = global
                && (call.callee == "fetch"
                    || code.param_defaults_to(path, &call.scope, &call.callee, "fetch"));
            if !is_fetch || code.args(path, n).is_none_or(<[_]>::is_empty) {
                continue;
            }
            // A URL from Backstage's `discovery.getBaseUrl('<id>')` starts at the plugin's mount.
            let plugin = backstage::base(code, path, n);
            let expanded = code.expand(Expand::Call(path, n), &|args, _| {
                let decided = config.decided(&joined(args));
                let folded =
                    (plugin.as_deref()).and_then(|id| Some((id, backstage::based(&decided, id)?)));
                let args = folded.as_ref().map_or(&decided, |(_, args)| args);
                // A URL through a proxy endpoint goes to the endpoint's target.
                let proxied = config.proxied(args);
                let target = proxied.as_ref().and_then(|p| p.args.as_ref());
                let mut read = fetch(target.unwrap_or(args))?;
                if let (None, Some((id, _))) = (&read.via, folded) {
                    read.via = Some(backstage::client(id));
                }
                if let Some(p) = proxied {
                    read.via = backstage::proxied(read.via, &p.endpoint, p.args.is_some());
                }
                Some(read)
            });
            for Expanded {
                site: (site, at),
                frames,
                found: read,
            } in expanded
            {
                let shape = response_reads(code, literals.types(), (path, n), &frames);
                found.push(consumer(site, at, read, shape));
            }
        }
    }
    found
}

/// `args` with a first argument `new URL(reference, base)` replaced by the join of the two (RFC
/// 3986 section 5.2, which the WHATWG URL Standard's constructor agrees with for these
/// references), before the `fetch` is read. Over a base that is an environment read or a hole, an
/// absolute-path reference keeps the opaque lead; a parameter on either side waits for the call
/// sites; anything else the join does not read leaves the URL unknown.
fn joined(args: &[Value]) -> Vec<Value> {
    let Some(Value::New(callee, urls)) = args.first() else {
        return args.to_vec();
    };
    let ([reference, base], true) = (&urls[..], callee == GLOBAL_URL) else {
        return args.to_vec();
    };
    // A base that does not fold (a member, a call) is a hole: opaque, whatever it holds.
    let base = text_parts(base).unwrap_or_else(|| vec![Part::Hole]);
    let url = text_parts(reference).and_then(|r| {
        let join = resolve(&base, &r).or_else(|| opaque_lead(&base, &r));
        join.or_else(|| depends(&base, &r))
    });
    let mut out = args.to_vec();
    out[0] = url.map_or(Value::Unknown, Value::Str);
    out
}

/// The consumer shape of the `fetch` at `call` (spec #23): the field paths read from the
/// parsed body of its response, by the function making the call and by each client wrapper
/// `frames` climbs from it to the consumer's site, which reads what the one below returns.
/// The body is the result of `.json()` on the response; a wrapper returns the response, the
/// body or a part of the body. `None` when nothing is read.
fn response_reads<'a>(
    code: &Code<'a>,
    types: &mut Types,
    call: (&'a str, usize),
    frames: &[Frame<'a>],
) -> Option<Shape> {
    /// What a value is: the response, or the body below a path (`Body(vec![])`: all of it).
    #[derive(Clone, PartialEq)]
    enum Got<'s> {
        Response,
        Body(Vec<&'s Step>),
    }
    let site = frames.last().map_or(call.0, |f| f.path);
    let mut out = Shape::new(Source::Reads(site.to_string()));
    let (mut got, mut at) = (Got::Response, call);
    // The types the wrapper below declares for the body with its own type parameters, by path:
    // its call gives the arguments that stand for them. And that wrapper.
    let mut generic: Vec<(String, &'a Ty)> = Vec::new();
    let mut wrapper: Option<Target<'a>> = None;
    for level in 0..=frames.len() {
        let mut returned = Vec::new();
        let mut deferred: Vec<(String, &'a Ty)> = Vec::new();
        // Where the call `at` is written; a call that is not there reads nothing.
        let scope = code.files().get(at.0).and_then(|f| f.calls.get(at.1));
        let scope = scope.map(|c| c.scope.as_str());
        for steps in code.uses(at.0, at.1)? {
            let (mut path, rest) = match (&got, steps.first()) {
                (Got::Response, Some(Step::Return)) if steps.len() == 1 => {
                    returned.push(Got::Response);
                    continue;
                }
                (Got::Response, Some(Step::Call(m))) if m == "json" => (Vec::new(), &steps[1..]),
                (Got::Response, _) => continue,
                (Got::Body(path), _) => (path.clone(), &steps[..]),
            };
            // A read stops at a method call: `name.toUpperCase()` reads `name`.
            let read = (rest.iter())
                .take_while(|s| matches!(s, Step::Field(_) | Step::Element))
                .count();
            path.extend(&rest[..read]);
            let base = written(&path);
            for prefix in shape::prefixes(&base) {
                out.list(prefix.to_string(), false, false);
            }
            match rest.get(read) {
                Some(Step::Return) => returned.push(Got::Body(path)),
                // The type declares what the value holds: the scope is the call's.
                Some(Step::Typed(ty)) if ty.has_param() => deferred.push((base, ty)),
                Some(Step::Typed(ty)) => {
                    let Some(scope) = scope else { continue };
                    let listed = types.read(code, at.0, scope, ty).unwrap_or_default();
                    typed(&mut out, &base, listed);
                }
                // The wrapper's type parameters, given here, stand in the types it declared.
                Some(Step::Argument(tys)) => {
                    let (Some(scope), Some(wrapper)) = (scope, wrapper) else {
                        continue;
                    };
                    let names = code.type_params(wrapper.path, wrapper.name);
                    let args = declared::Args {
                        names,
                        tys,
                        at: (at.0, scope),
                    };
                    for (base, ty) in &generic {
                        let listed = types
                            .read_with(code, at.0, scope, ty, Some(args))
                            .unwrap_or_default();
                        typed(&mut out, base, listed);
                    }
                }
                _ => {}
            }
        }
        let Some(frame) = frames.get(level) else {
            break;
        };
        // The frame calls the function making the call `at`, which returns one thing.
        returned.dedup();
        match <[Got; 1]>::try_from(returned) {
            Ok([one]) if frame.def.path == at.0 && Some(frame.def.name) == scope => {
                // A return type that is a type parameter of the wrapper declares what it returns.
                if let (Got::Body(path), Some(ty)) = (&one, code.returns(at.0, frame.def.name))
                    && ty.has_param()
                {
                    deferred.push((written(path), ty));
                }
                got = one;
            }
            _ => break,
        }
        (generic, wrapper) = (deferred, Some(frame.def));
        at = (frame.path, frame.index);
    }
    out.fields.remove("");
    (!out.fields.is_empty() || !out.imports.is_empty()).then_some(out)
}

/// Adds the fields a declared type lists, below `base`, to a consumer's `shape`, with the types
/// and schemas of a package it names for the query to fill.
fn typed(shape: &mut Shape, base: &str, listed: Declared) {
    for (field, optional) in listed.fields {
        shape.list(under(base, &field), optional, true);
    }
    for import in listed.imports {
        let at = under(base, &import.at);
        shape.imports.insert(Import { at, ..import });
    }
}

/// The field path `steps` (fields and elements) read: `items[].id`.
fn written(steps: &[&Step]) -> String {
    let mut out = String::new();
    for step in steps {
        match step {
            Step::Field(name) if out.is_empty() => out += name,
            Step::Field(name) => out += &format!(".{name}"),
            _ => out += "[]",
        }
    }
    out
}
