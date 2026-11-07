//! Complete factory evidence: an opaque alternative invalidates a known constructor.
use std::collections::{BTreeMap, HashMap};

use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, Of, Target};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Construction {
    pub name: String,
    pub argument: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'a> {
    Name(&'a str),
    Call(usize, usize),
    Unknown,
}

type Key<'a> = (&'a str, &'a str, Node<'a>, bool, bool);
#[derive(Clone, PartialEq, Eq, Hash)]
struct Frame<'a> {
    function: Target<'a>,
    // Unknown arguments are absent. Only complete constructor evidence affects this walk.
    arguments: BTreeMap<(usize, bool), Construction>,
}

pub(crate) struct Factories<'c, 'a> {
    code: &'c Code<'a>,
    constructors: &'c [&'c str],
    factories: &'c [(&'c str, &'c str)],
    keyword: &'c str,
    budget: usize,
    // A visit in progress is unknown, so a cycle supplies no constructor.
    known: HashMap<(Key<'a>, Option<Frame<'a>>), Option<Construction>>,
    frames: Vec<Frame<'a>>,
}

impl<'c, 'a> Factories<'c, 'a> {
    pub(crate) fn new(
        code: &'c Code<'a>,
        constructors: &'c [&'c str],
        factories: &'c [(&'c str, &'c str)],
        keyword: &'c str,
    ) -> Self {
        Self {
            code,
            constructors,
            factories,
            keyword,
            budget: crate::resolve::EVAL_BUDGET,
            known: HashMap::new(),
            frames: Vec::new(),
        }
    }

    /// Complete receiver construction, with at most one external factory level. The cache
    /// belongs to one question, so a previously checked prefix does not shorten its ceiling;
    /// each question has its own bound, so an opaque chain cannot
    /// starve a later direct constructor.
    pub(crate) fn read(&mut self, path: &'a str, call: &'a Call) -> Option<Construction> {
        self.budget = crate::resolve::EVAL_BUDGET;
        self.known.clear();
        self.receiver(path, call, true, 0)
    }

    fn receiver(
        &mut self,
        path: &'a str,
        call: &'a Call,
        through: bool,
        depth: usize,
    ) -> Option<Construction> {
        let receiver = call.callee.rsplit_once('.')?.0;
        if receiver == super::CALL {
            return self.called(
                path,
                super::head(self.code, path, call)?,
                through,
                false,
                depth,
            );
        }
        self.value(path, &call.scope, Of::Name(receiver), through, false, depth)
    }

    fn value(
        &mut self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        through: bool,
        awaited: bool,
        depth: usize,
    ) -> Option<Construction> {
        if let Of::Value(Value::Await(value)) = of {
            if awaited || depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
                return None;
            }
            self.budget -= 1;
            return self.value(path, scope, Of::Value(value), through, true, depth + 1);
        }
        let of = match of {
            Of::Value(Value::Ref(name)) => Of::Name(name),
            other => other,
        };
        let node = match of {
            Of::Name(name) => Node::Name(name),
            Of::Value(Value::Call(call)) => Node::Call(call.range.start, call.range.end),
            _ => Node::Unknown,
        };
        let key = (path, scope, node, through, awaited);
        let contextual = (key, self.frames.last().cloned());
        if let Some(made) = self.known.get(&contextual) {
            return made.clone();
        }
        if depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        self.known.insert(contextual.clone(), None);
        let made = match of {
            Of::Name(name) => {
                if let Some((at, within, value)) = self.code.constant_value(path, scope, name) {
                    self.value(at, within, Of::Value(value), through, awaited, depth + 1)
                } else if let Some(bound) = self.code.local(path, scope, name) {
                    self.value(
                        path,
                        &bound.within,
                        Of::Value(&bound.value),
                        through,
                        awaited,
                        depth + 1,
                    )
                } else if let Some((function, n)) = self.code.param_of(path, scope, name) {
                    match self.frames.last() {
                        Some(frame) if frame.function == function => {
                            frame.arguments.get(&(n, awaited)).cloned()
                        }
                        Some(_) => None,
                        None => self
                            .code
                            .param_values(function, n, None)
                            .and_then(|values| self.values(values, through, awaited, depth + 1)),
                    }
                } else {
                    None
                }
            }
            Of::Value(Value::Call(call)) => self
                .code
                .call_index(path, &call.range)
                .and_then(|index| self.code.call(path, index))
                .and_then(|call| self.called(path, call, through, awaited, depth + 1)),
            _ => None,
        };
        self.known.insert(contextual, made.clone());
        made
    }

    fn values(
        &mut self,
        values: Vec<(&'a str, &'a str, &'a Value)>,
        through: bool,
        awaited: bool,
        depth: usize,
    ) -> Option<Construction> {
        let mut first = None;
        for (at, within, value) in values {
            let made = self.value(at, within, Of::Value(value), through, awaited, depth)?;
            if first.as_ref().is_some_and(|first| *first != made) {
                return None;
            }
            first = Some(made);
        }
        first
    }

    fn called(
        &mut self,
        path: &'a str,
        call: &'a Call,
        through: bool,
        awaited: bool,
        depth: usize,
    ) -> Option<Construction> {
        if depth >= crate::resolve::EVAL_DEPTH {
            return None;
        }
        let index = self.code.call_index(path, &call.range)?;
        let awaited = awaited || call.awaited;
        if let Some(function) = self.code.reaches(path, index) {
            let coroutine = self.code.coroutine(function);
            if coroutine && !awaited {
                return None;
            }
            let values = self.code.returned(path, index);
            let mut arguments = BTreeMap::new();
            let mut n = 0;
            while self.code.param(function, n).is_some() {
                if self.budget == 0 {
                    return None;
                }
                self.budget -= 1;
                for awaiting_argument in [false, true] {
                    if let Some(made) = self
                        .code
                        .param_values(function, n, Some((path, index)))
                        .and_then(|values| {
                            self.values(values, through, awaiting_argument, depth + 1)
                        })
                    {
                        arguments.insert((n, awaiting_argument), made);
                    }
                }
                n += 1;
            }
            self.frames.push(Frame {
                function,
                arguments,
            });
            let made = self.values(values, through, awaited && !coroutine, depth + 1);
            self.frames.pop();
            return made;
        }
        let name = if let Some(name) = super::imported(self.code, path, &call.scope, &call.callee) {
            name
        } else {
            let method = call.callee.rsplit_once('.')?.1;
            if !through || !self.factories.iter().any(|&(_, factory)| factory == method) {
                return None;
            }
            let parent = self.receiver(path, call, false, depth + 1)?;
            if !self.factories.contains(&(parent.name.as_str(), method)) {
                return None;
            }
            format!("{}.{method}", parent.name)
        };
        let argument = if self.constructors.contains(&name.as_str()) {
            match call.args.first().or_else(|| call.keyword(self.keyword)) {
                Some(Value::Str(parts)) => literal(parts),
                _ => None,
            }
        } else {
            None
        };
        if self.constructors.contains(&name.as_str()) {
            if awaited {
                return None;
            }
            return Some(Construction {
                name: self.constructors.first()?.to_string(),
                argument,
            });
        }
        if awaited && self.factories.iter().any(|&(parent, _)| parent == name) {
            return None;
        }
        let canonical = self
            .factories
            .iter()
            .find(|&&(parent, method)| {
                parent == name
                    && self
                        .constructors
                        .iter()
                        .any(|c| *c == format!("{parent}.{method}"))
            })
            .and_then(|&(_, method)| {
                self.factories.iter().find(|&&(parent, m)| {
                    m == method
                        && self
                            .constructors
                            .iter()
                            .any(|c| *c == format!("{parent}.{m}"))
                })
            })
            .map(|&(parent, _)| parent);
        Some(Construction {
            name: canonical.map_or(name.clone(), str::to_string),
            argument,
        })
    }
}
