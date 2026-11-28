use super::super::join::{depends, opaque_lead, resolve, text_parts};
use super::super::{consumer, fetch};
use super::backstage;
use super::config::Config;
use crate::contract::{Found, Level, ReceiverIdentity};
use crate::declared::{self, Declared, Types};
use crate::lang::ts::GLOBAL_URL;
use crate::lang::{Part, Step, Ty, Value};
use crate::literals::Literals;
use crate::resolve::{Code, Expand, Expanded, Frame, Target};
use crate::shape::{self, Import, Shape, Source, under};

pub(in crate::http) fn consumers(
    code: &Code,
    config: &Config,
    literals: &mut Literals,
) -> Vec<Found> {
    let mut found = Vec::new();
    let global = code.binds_everywhere("fetch");
    for (&path, facts) in code.files() {
        for (n, call) in facts.calls.iter().enumerate() {
            let is_fetch = global
                && (call.callee == "fetch"
                    || code.param_defaults_to(path, &call.scope, &call.callee, "fetch"));
            if !is_fetch || code.args(path, n).is_none_or(<[_]>::is_empty) {
                continue;
            }
            let expanded = code.expand_with_call(
                Expand::Call(path, n),
                &backstage::discovered,
                backstage::has_route,
                &|args, _| {
                    let mut args = joined(args);
                    let plugin = backstage::based(&mut args);
                    config.decided(args, |args, uncertain| {
                        let identity = slash_base(args);
                        let proxied = config.proxied(args);
                        let target = proxied.as_ref().and_then(|p| p.args.as_deref());
                        let mut read = fetch(target.unwrap_or(args))?;
                        if let (None, Some(id)) = (&read.via, &plugin) {
                            read.via = Some(backstage::client(id));
                        }
                        if let Some(p) = proxied {
                            read.via = backstage::proxied(read.via, &p.endpoint, p.args.is_some());
                        }
                        Some((read, identity, uncertain))
                    })
                },
            );
            for Expanded {
                site: (site, at),
                frames,
                found: read,
            } in expanded
            {
                let shape = response_reads(code, literals.types(), (path, n), &frames);
                for (read, identity, uncertain) in read {
                    let mut side = consumer(site, at, read, shape.clone());
                    side.cap = uncertain.then_some(Level::Inferred);
                    side.identity = identity.or(side.identity);
                    found.push(side);
                }
            }
        }
    }
    found
}

fn slash_base(args: &mut [Value]) -> Option<ReceiverIdentity> {
    let Value::Str(parts) = args.first_mut()? else {
        return None;
    };
    if !parts.iter().any(|p| matches!(p, Part::StripStart)) {
        return None;
    }
    *parts = crate::lang::fold_strips(std::mem::take(parts));
    let starts = parts
        .iter()
        .take_while(|p| matches!(p, Part::StripStart))
        .count();
    if starts == 0 {
        return None;
    }
    let env = matches!(parts.get(starts), Some(Part::Env(_)));
    let ends = parts
        .get(starts + 1..)?
        .iter()
        .map_while(|p| match p {
            Part::StripEnd(strip) => Some(*strip),
            _ => None,
        })
        .take(starts)
        .collect::<Vec<_>>();
    if ends.len() != starts {
        return None;
    }
    parts.drain(starts + 1..=starts * 2);
    parts.drain(..starts);
    env.then(|| {
        let tail = &parts[1..];
        let end = tail
            .iter()
            .position(|part| matches!(part, Part::QuerySuffix))
            .unwrap_or(tail.len());
        let mut suffix = super::super::render(&tail[..end]);
        if let Some(at) = suffix.find(['?', '#']) {
            suffix.truncate(at);
        }
        ReceiverIdentity::HttpBase {
            strips: ends,
            suffix,
        }
    })
}

fn joined(args: &[Value]) -> Vec<Value> {
    let Some(Value::New(callee, urls)) = args.first() else {
        return args.to_vec();
    };
    let ([reference, base], true) = (&urls[..], callee == GLOBAL_URL) else {
        return args.to_vec();
    };
    let base = text_parts(base).unwrap_or_else(|| vec![Part::Hole]);
    let url = text_parts(reference).and_then(|r| {
        let join = resolve(&base, &r).or_else(|| opaque_lead(&base, &r));
        join.or_else(|| depends(&base, &r))
    });
    let mut out = args.to_vec();
    out[0] = url.map_or(Value::Unknown, Value::Str);
    out
}

fn response_reads<'a>(
    code: &Code<'a>,
    types: &mut Types,
    call: (&'a str, usize),
    frames: &[Frame<'a>],
) -> Option<Shape> {
    #[derive(Clone, PartialEq)]
    enum Got<'s> {
        Response,
        Body(Vec<&'s Step>),
    }
    let site = frames.last().map_or(call.0, |f| f.path);
    let mut out = Shape::new(Source::Reads(site.to_string()));
    let (mut got, mut at) = (Got::Response, call);
    let mut generic: Vec<(String, &'a Ty)> = Vec::new();
    let mut wrapper: Option<Target<'a>> = None;
    for level in 0..=frames.len() {
        let mut returned = Vec::new();
        let mut deferred: Vec<(String, &'a Ty)> = Vec::new();
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
                Some(Step::Typed(ty)) if ty.has_param() => deferred.push((base, ty)),
                Some(Step::Typed(ty)) => {
                    let Some(scope) = scope else { continue };
                    let listed = types.read(code, at.0, scope, ty).unwrap_or_default();
                    typed(&mut out, &base, listed);
                }
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
        returned.dedup();
        match <[Got; 1]>::try_from(returned) {
            Ok([one]) if frame.def.path == at.0 && Some(frame.def.name) == scope => {
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

fn typed(shape: &mut Shape, base: &str, listed: Declared) {
    for (field, optional) in listed.fields {
        shape.list(under(base, &field), optional, true);
    }
    for import in listed.imports {
        let at = under(base, &import.at);
        shape.imports.insert(Import { at, ..import });
    }
}

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
