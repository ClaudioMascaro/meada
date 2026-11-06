//! Complete, memoized constructor evidence. An opaque alternative invalidates the evidence;
//! the shared value walk's surviving origins alone cannot establish that every branch is known.
use std::collections::HashMap;

use super::{Client, Language, go, java};
use crate::lang::Value;
use crate::resolve::{Code, Of};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'a> {
    Name(&'a str),
    Call(usize, usize),
    New(&'a str),
    Unknown,
}

type Key<'a> = (&'a str, &'a str, Node<'a>, &'static str);

pub(super) struct Origins<'c, 'a> {
    code: &'c Code<'a>,
    language: Language,
    budget: usize,
    /// None is a visit in progress, so a cycle supplies no constructor.
    known: HashMap<Key<'a>, Option<bool>>,
}

impl<'c, 'a> Origins<'c, 'a> {
    pub(super) fn new(code: &'c Code<'a>, language: Language) -> Self {
        Self {
            code,
            language,
            budget: crate::resolve::EVAL_BUDGET,
            known: HashMap::new(),
        }
    }

    pub(super) fn call(&mut self, path: &'a str, index: usize, client: &Client) -> bool {
        let Some(call) = self.code.call(path, index) else {
            return false;
        };
        let receiver = self
            .code
            .receiver(path, index)
            .map(Of::Value)
            .or_else(|| call.callee.rsplit_once('.').map(|(name, _)| Of::Name(name)));
        receiver.is_some_and(|value| self.value(path, &call.scope, value, client, 0))
    }

    fn value(
        &mut self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        client: &Client,
        depth: usize,
    ) -> bool {
        let of = match of {
            Of::Value(Value::Ref(name)) => Of::Name(name),
            other => other,
        };
        let node = match of {
            Of::Name(name) => Node::Name(name),
            Of::Value(Value::Call(call)) => Node::Call(call.range.start, call.range.end),
            Of::Value(Value::New(name, _)) => Node::New(name),
            _ => Node::Unknown,
        };
        let key = (path, scope, node, client.java);
        if let Some(known) = self.known.get(&key) {
            return known.unwrap_or(false);
        }
        if depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
            return false;
        }
        self.budget -= 1;
        self.known.insert(key, None);
        let answer = self.read(path, scope, of, client, depth + 1);
        self.known.insert(key, Some(answer));
        answer
    }

    fn read(
        &mut self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        client: &Client,
        depth: usize,
    ) -> bool {
        match of {
            Of::Name(written) => {
                // `this.field` is the instance field even when a local/parameter has its name.
                // `written` resolves its declaring scope through the resolver's indexed maps.
                let (path, scope, name) = if let Some(field) = written.strip_prefix("this.") {
                    let Some((at, within, _)) = self.code.written(path, scope, written) else {
                        return false;
                    };
                    (at, within, field)
                } else {
                    (path, scope, written)
                };
                if let Some((at, within, value)) = self.code.constant_value(path, scope, name) {
                    return self.value(at, within, Of::Value(value), client, depth);
                }
                if let Some(bound) = self.code.local(path, scope, name) {
                    return self.value(path, &bound.within, Of::Value(&bound.value), client, depth);
                }
                if let Some((function, n)) = self.code.param_of(path, scope, name) {
                    let args = self.code.param_args(function, n);
                    return !args.is_empty()
                        && args.into_iter().all(|(at, within, value)| {
                            self.value(at, within, Of::Value(value), client, depth)
                        });
                }
                false
            }
            Of::Value(Value::New(ty, _)) if self.language == Language::Csharp => {
                crate::names::known(self.code, path, scope, ty, |s| s == client.csharp).is_some()
            }
            Of::Value(Value::Call(call)) => {
                let Some(index) = self.code.call_index(path, &call.range) else {
                    return false;
                };
                if self.code.reaches(path, index).is_some() {
                    let values = self.code.returned(path, index);
                    return !values.is_empty()
                        && values.into_iter().all(|(at, within, value)| {
                            self.value(at, within, Of::Value(value), client, depth)
                        });
                }
                match self.language {
                    Language::Java => java::made(self.code, path, index, client.java),
                    Language::Go => go::made(self.code, path, index, client.go),
                    Language::Csharp => false,
                }
            }
            _ => false,
        }
    }
}
