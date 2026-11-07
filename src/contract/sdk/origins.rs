//! Complete, memoized constructor evidence. An opaque alternative invalidates the evidence;
//! kind policies retain receiver-carried names without a second value-resolution walk.
use std::collections::HashMap;
use std::hash::Hash;

use super::{Client, Language, go, java};
use crate::lang::{Invocation, Value};
use crate::resolve::{Code, Frame, Of, Target};

/// Syntax-neutral evidence supplied by the contract kind. Only listed constructors and
/// child factories can establish a handle; an unknown step is absent evidence.
pub(crate) trait Policy<'a> {
    type Evidence: Clone + Eq + Hash;
    type Argument: Clone + Eq + Hash + Default;
    fn constructed(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a str,
        args: &'a [Value],
        frames: &[Frame<'a>],
    ) -> Option<Self::Evidence>;
    fn made(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        frames: &[Frame<'a>],
        awaited: bool,
    ) -> Option<Self::Evidence>;
    fn step(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        parent: Self::Evidence,
        frames: &[Frame<'a>],
    ) -> Option<Self::Evidence>;
    /// Legacy boolean evidence does not depend on constructor arguments.
    fn contextual(&self) -> bool {
        true
    }
    /// The identity-bearing part of an argument, for memoizing equivalent exact frames.
    fn argument(
        &self,
        _code: &Code<'a>,
        _path: &'a str,
        _scope: &'a str,
        _value: &'a Value,
        _frames: &[Frame<'a>],
        _site: (&'a str, usize),
    ) -> Self::Argument {
        Self::Argument::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'a> {
    Name(&'a str),
    Call(usize, usize),
    New(&'a str, usize),
    Unknown,
}
#[derive(Clone, PartialEq, Eq, Hash)]
struct Argument<E, A> {
    client: Option<E>,
    name: A,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Context<'a, E, A> {
    function: Target<'a>,
    arguments: Vec<Argument<E, A>>,
}

type Key<'a, E, A> = (&'a str, &'a str, Node<'a>, bool, Option<Context<'a, E, A>>);
type Known<'a, E, A> = HashMap<Key<'a, E, A>, Option<E>>;

pub(crate) struct Evidence<'c, 'a, P: Policy<'a>> {
    code: &'c Code<'a>,
    policy: P,
    budget: usize,
    known: Known<'a, P::Evidence, P::Argument>,
    frames: Vec<Frame<'a>>,
    contexts: Vec<Context<'a, P::Evidence, P::Argument>>,
}

impl<'c, 'a, P: Policy<'a>> Evidence<'c, 'a, P> {
    pub(crate) fn new(code: &'c Code<'a>, policy: P) -> Self {
        Self {
            code,
            policy,
            budget: crate::resolve::EVAL_BUDGET,
            known: HashMap::new(),
            frames: Vec::new(),
            contexts: Vec::new(),
        }
    }

    pub(crate) fn call(&mut self, path: &'a str, index: usize) -> Option<P::Evidence> {
        let call = self.code.call(path, index)?;
        self.receiver(path, index, &call.scope, 0)
    }

    fn receiver(
        &mut self,
        path: &'a str,
        index: usize,
        scope: &'a str,
        depth: usize,
    ) -> Option<P::Evidence> {
        let call = self.code.call(path, index)?;
        if let Some(receiver) = self.code.receiver(path, index) {
            return self.value(path, scope, Of::Value(receiver), false, depth);
        }
        // Packs without a stored receiver write an in-place receiver as a node kind.
        // Their next same-start call is the longest proper receiver, in parse order.
        if call.callee.starts_with('(') {
            let next = self.code.files()[path].calls.get(index + 1)?;
            if next.range.start != call.range.start || next.range.end >= call.range.end {
                return None;
            }
            return self.called(path, index + 1, None, false, depth);
        }
        self.value(
            path,
            scope,
            Of::Name(call.callee.rsplit_once('.')?.0),
            false,
            depth,
        )
    }

    fn value(
        &mut self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        awaited: bool,
        depth: usize,
    ) -> Option<P::Evidence> {
        let of = match of {
            Of::Value(Value::Ref(n)) => Of::Name(n),
            other => other,
        };
        let node = match of {
            Of::Name(n) => Node::Name(n),
            Of::Value(Value::Call(c)) => Node::Call(c.range.start, c.range.end),
            Of::Value(Value::New(n, a)) => Node::New(n, a.as_ptr() as usize),
            // Await has its own node: awaiting an already-awaited value is unknown.
            Of::Value(Value::Await(v)) => {
                if awaited || depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
                    return None;
                }
                self.budget -= 1;
                return self.value(path, scope, Of::Value(v), true, depth + 1);
            }
            _ => Node::Unknown,
        };
        let key = (path, scope, node, awaited, self.contexts.last().cloned());
        if let Some(known) = self.known.get(&key) {
            return known.clone();
        }
        if depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        self.known.insert(key.clone(), None);
        let answer = self.read(path, scope, of, awaited, depth + 1);
        self.known.insert(key, answer.clone());
        answer
    }

    fn read(
        &mut self,
        path: &'a str,
        scope: &'a str,
        of: Of<'a>,
        awaited: bool,
        depth: usize,
    ) -> Option<P::Evidence> {
        match of {
            Of::Name(written) => {
                let (path, scope, name) = if let Some(field) = written.strip_prefix("this.") {
                    let (at, within, _) = self.code.written(path, scope, written)?;
                    (at, within, field)
                } else {
                    (path, scope, written)
                };
                if let Some((at, within, value)) = self.code.constant_value(path, scope, name) {
                    return self.value(at, within, Of::Value(value), awaited, depth);
                }
                if let Some(bound) = self.code.local(path, scope, name) {
                    return self.value(
                        path,
                        &bound.within,
                        Of::Value(&bound.value),
                        awaited,
                        depth,
                    );
                }
                if let Some((at, index)) = self.code.first_result(path, scope, name) {
                    return self.called(at, index, None, awaited, depth);
                }
                let (function, n) = self.code.param_of(path, scope, name)?;
                if let Some(frame) = self.contexts.last() {
                    return (frame.function == function)
                        .then(|| frame.arguments.get(n)?.client.clone())
                        .flatten();
                }
                let selected = self
                    .frames
                    .iter()
                    .rev()
                    .find(|f| f.def == function)
                    .map(|f| (f.path, f.index));
                let values = self.code.param_values(function, n, selected)?;
                self.values(values, awaited, depth)
            }
            Of::Value(Value::New(ty, args)) if !awaited => {
                self.policy
                    .constructed(self.code, path, scope, ty, args, &self.frames)
            }
            Of::Value(Value::Call(call)) => self.called(
                path,
                self.code.call_index(path, &call.range)?,
                Some(call),
                awaited,
                depth,
            ),
            _ => None,
        }
    }

    fn values(
        &mut self,
        values: Vec<(&'a str, &'a str, &'a Value)>,
        awaited: bool,
        depth: usize,
    ) -> Option<P::Evidence> {
        let mut answer = None;
        for (at, within, value) in values {
            let next = self.value(at, within, Of::Value(value), awaited, depth)?;
            if answer.as_ref().is_some_and(|first| first != &next) {
                return None;
            }
            answer = Some(next);
        }
        answer
    }

    fn called(
        &mut self,
        path: &'a str,
        index: usize,
        invocation: Option<&'a Invocation>,
        awaited: bool,
        depth: usize,
    ) -> Option<P::Evidence> {
        if depth >= crate::resolve::EVAL_DEPTH || self.budget == 0 {
            return None;
        }
        let call = self.code.call(path, index)?;
        if let Some(function) = self.code.reaches(path, index) {
            let coroutine = self.code.coroutine(function);
            if coroutine && !(awaited || call.awaited) {
                return None;
            }
            let contextual = self.policy.contextual();
            if contextual {
                let mut arguments = Vec::new();
                let mut n = 0;
                while self.code.param(function, n).is_some() {
                    if self.budget == 0 {
                        return None;
                    }
                    self.budget -= 1;
                    let values = self.code.param_values(function, n, Some((path, index)));
                    let client = values
                        .as_ref()
                        .and_then(|v| self.values(v.clone(), false, depth + 1));
                    let name = values
                        .as_ref()
                        .and_then(|v| v.first())
                        .map(|&(at, within, v)| {
                            self.policy.argument(
                                self.code,
                                at,
                                within,
                                v,
                                &self.frames,
                                (path, index),
                            )
                        })
                        .unwrap_or_default();
                    arguments.push(Argument { client, name });
                    n += 1;
                }
                self.frames.push(Frame {
                    def: function,
                    path,
                    index,
                });
                self.contexts.push(Context {
                    function,
                    arguments,
                });
            }
            let answer = self.values(
                self.code.returned(path, index),
                awaited && !coroutine,
                depth + 1,
            );
            if contextual {
                self.frames.pop();
                self.contexts.pop();
            }
            return answer;
        }
        if let Some(made) = self.policy.made(
            self.code,
            path,
            index,
            &self.frames,
            awaited || call.awaited,
        ) {
            return Some(made);
        }
        let parent = match invocation.and_then(|c| c.receiver.as_deref()) {
            Some(value) => self.value(path, &call.scope, Of::Value(value), false, depth + 1),
            None => self.receiver(path, index, &call.scope, depth + 1),
        }?;
        self.policy
            .step(self.code, path, index, parent, &self.frames)
    }
}

struct Aws<'c> {
    language: Language,
    client: &'c Client,
}
impl<'a> Policy<'a> for Aws<'_> {
    type Evidence = ();
    type Argument = ();
    fn constructed(
        &self,
        code: &Code<'a>,
        path: &'a str,
        scope: &'a str,
        ty: &'a str,
        _: &'a [Value],
        _: &[Frame<'a>],
    ) -> Option<()> {
        (self.language == Language::Csharp
            && crate::names::known(code, path, scope, ty, |s| s == self.client.csharp).is_some())
        .then_some(())
    }
    fn made(
        &self,
        code: &Code<'a>,
        path: &'a str,
        index: usize,
        _: &[Frame<'a>],
        _: bool,
    ) -> Option<()> {
        match self.language {
            Language::Java => java::made(code, path, index, self.client.java),
            Language::Go => go::made(code, path, index, self.client.go),
            Language::Csharp => false,
        }
        .then_some(())
    }
    fn step(&self, _: &Code<'a>, _: &'a str, _: usize, _: (), _: &[Frame<'a>]) -> Option<()> {
        None
    }
    fn contextual(&self) -> bool {
        false
    }
}

/// Compatible boolean API for existing AWS operation tables.
pub(super) struct Origins<'c, 'a> {
    code: &'c Code<'a>,
    language: Language,
    readers: HashMap<&'static str, Evidence<'c, 'a, Aws<'c>>>,
}
impl<'c, 'a> Origins<'c, 'a> {
    pub(super) fn new(code: &'c Code<'a>, language: Language) -> Self {
        Self {
            code,
            language,
            readers: HashMap::new(),
        }
    }
    pub(super) fn call(&mut self, path: &'a str, index: usize, client: &'c Client) -> bool {
        self.readers
            .entry(client.java)
            .or_insert_with(|| {
                Evidence::new(
                    self.code,
                    Aws {
                        language: self.language,
                        client,
                    },
                )
            })
            .call(path, index)
            .is_some()
    }
}
