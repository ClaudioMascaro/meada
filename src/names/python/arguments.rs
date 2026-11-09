//! Complete factory evidence: an opaque alternative invalidates a known constructor.
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::lang::{Call, Value, literal};
use crate::resolve::{Code, EVAL_BUDGET, EVAL_DEPTH, Of, Target};

/// The steps the factory questions one reader asks of one file take together, a question taking
/// up to all that is left, and still `FACTORY_QUESTION_FLOOR` once they are spent.
// ponytail: real files spend a few steps a question, a repeated walk answered from the file's
// kept answers; 2^18 stops a file of thousands of questions each walking thousands of call
// sites that a cycle keeps from being kept.
const FACTORY_FILE_BUDGET: usize = 1 << 18;
/// The steps a question takes however much of `FACTORY_FILE_BUDGET` is spent.
// ponytail: a client constructed a few names or one factory away takes under ten steps.
const FACTORY_QUESTION_FLOOR: usize = 16;

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
/// A value visited in the frame of a call, by its index in `Factories::frames`, or in none.
type Visit<'a> = (Key<'a>, Option<usize>);
/// The values a function returns, or those its callers pass one of its parameters.
type Listed<'a> = Rc<[(&'a str, &'a str, &'a Value)]>;

pub(crate) struct Factories<'c, 'a> {
    code: &'c Code<'a>,
    constructors: &'c [&'c str],
    factories: &'c [(&'c str, &'c str)],
    keyword: &'c str,
    budget: usize,
    /// The file of the questions so far, and what is left of its `FACTORY_FILE_BUDGET`.
    file: &'a str,
    left: usize,
    // A visit in progress is unknown, so a cycle supplies no constructor.
    known: HashMap<Visit<'a>, Option<Construction>>,
    /// The answers no ceiling or visit in progress cut, kept for the file's later questions:
    /// about a value, and about the returns of a function in a frame. Each holds how much deeper
    /// than its visit its walk went, and is read again only where that still fits `EVAL_DEPTH`.
    settled: HashMap<Visit<'a>, (Option<Construction>, usize)>,
    returns: HashMap<(usize, bool, bool), (Option<Construction>, usize)>,
    /// Whether the walk so far met a ceiling or a visit in progress, and the deepest it went.
    cut: bool,
    reached: usize,
    /// Each frame of the file's walks once, so a visit names its frame by index, and the
    /// indexes of the frames the walk is in.
    frames: Vec<Rc<Frame<'a>>>,
    framed: HashMap<Rc<Frame<'a>>, usize>,
    stack: Vec<usize>,
    /// What each function returns (`None`) and what its callers pass each parameter, listed
    /// once, then read a step at a time.
    listed: HashMap<(Target<'a>, Option<usize>), Option<Listed<'a>>>,
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
            budget: EVAL_BUDGET,
            file: "",
            left: FACTORY_FILE_BUDGET,
            known: HashMap::new(),
            settled: HashMap::new(),
            returns: HashMap::new(),
            cut: false,
            reached: 0,
            frames: Vec::new(),
            framed: HashMap::new(),
            stack: Vec::new(),
            listed: HashMap::new(),
        }
    }

    /// Complete receiver construction, with at most one external factory level. Each question
    /// has its own bound, at most what its file has left of `FACTORY_FILE_BUDGET` and at least
    /// `FACTORY_QUESTION_FLOOR`, so an opaque chain cannot starve a later direct constructor.
    /// The answers no ceiling or cycle cut are kept for the file's later questions, each visit
    /// of a value still a step; the others belong to one question, so a previously checked
    /// prefix does not shorten its ceiling.
    pub(crate) fn read(&mut self, path: &'a str, call: &'a Call) -> Option<Construction> {
        if path != self.file {
            (self.file, self.left) = (path, FACTORY_FILE_BUDGET);
            self.settled.clear();
            self.returns.clear();
            self.frames.clear();
            self.framed.clear();
        }
        let budget = self.left.max(FACTORY_QUESTION_FLOOR);
        self.budget = budget;
        self.known.clear();
        self.cut = false;
        let made = self.receiver(path, call, true, 0);
        self.left = self.left.saturating_sub(budget - self.budget);
        made
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

    /// Whether a step is left, spending it; past a ceiling the walk is cut.
    fn step(&mut self, depth: usize) -> bool {
        self.reached = self.reached.max(depth);
        if depth >= EVAL_DEPTH || self.budget == 0 {
            self.cut = true;
            return false;
        }
        self.budget -= 1;
        true
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
            if awaited || !self.step(depth) {
                return None;
            }
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
        let visit = (
            (path, scope, node, through, awaited),
            self.stack.last().copied(),
        );
        if !self.step(depth) {
            return None;
        }
        // Visited before in this question: cut there, or in progress.
        if let Some(made) = self.known.get(&visit) {
            self.cut = true;
            return made.clone();
        }
        if let Some((made, height)) = self.settled.get(&visit)
            && depth + height < EVAL_DEPTH
        {
            self.reached = self.reached.max(depth + height);
            return made.clone();
        }
        self.known.insert(visit, None);
        let outer = std::mem::take(&mut self.cut);
        let reached = std::mem::replace(&mut self.reached, depth);
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
                    match self.stack.last().map(|&frame| &self.frames[frame]) {
                        Some(frame) if frame.function == function => {
                            frame.arguments.get(&(n, awaited)).cloned()
                        }
                        Some(_) => None,
                        None => {
                            let code = self.code;
                            let passed = self.listed((function, Some(n)), || {
                                code.param_values(function, n, None)
                            });
                            passed.and_then(|values| {
                                self.values(values.iter().copied(), through, awaited, depth + 1)
                            })
                        }
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
        let height = self.reached - depth;
        self.reached = self.reached.max(reached);
        if self.cut {
            self.known.insert(visit, made.clone());
        } else {
            self.known.remove(&visit);
            self.settled.insert(visit, (made.clone(), height));
        }
        self.cut |= outer;
        made
    }

    fn values(
        &mut self,
        values: impl IntoIterator<Item = (&'a str, &'a str, &'a Value)>,
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

    /// The values listed under `key`, made by `list` the first time they are asked.
    fn listed(
        &mut self,
        key: (Target<'a>, Option<usize>),
        list: impl FnOnce() -> Option<Vec<(&'a str, &'a str, &'a Value)>>,
    ) -> Option<Listed<'a>> {
        (self.listed.entry(key))
            .or_insert_with(|| list().map(Rc::from))
            .clone()
    }

    /// The index of `frame` in `frames`, added the first time it is met.
    fn frame(&mut self, frame: Frame<'a>) -> usize {
        if let Some(&at) = self.framed.get(&frame) {
            return at;
        }
        let frame = Rc::new(frame);
        self.frames.push(Rc::clone(&frame));
        self.framed.insert(frame, self.frames.len() - 1);
        self.frames.len() - 1
    }

    fn called(
        &mut self,
        path: &'a str,
        call: &'a Call,
        through: bool,
        awaited: bool,
        depth: usize,
    ) -> Option<Construction> {
        self.reached = self.reached.max(depth);
        if depth >= EVAL_DEPTH {
            self.cut = true;
            return None;
        }
        let index = self.code.call_index(path, &call.range)?;
        let awaited = awaited || call.awaited;
        if let Some(function) = self.code.reaches(path, index) {
            let coroutine = self.code.coroutine(function);
            if coroutine && !awaited {
                return None;
            }
            let mut arguments = BTreeMap::new();
            let mut n = 0;
            while self.code.param(function, n).is_some() {
                if !self.step(depth) {
                    return None;
                }
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
            let frame = self.frame(Frame {
                function,
                arguments,
            });
            let awaited = awaited && !coroutine;
            if let Some((made, height)) = self.returns.get(&(frame, through, awaited))
                && depth + height < EVAL_DEPTH
            {
                self.reached = self.reached.max(depth + height);
                return made.clone();
            }
            let code = self.code;
            let values = self.listed((function, None), || Some(code.returned(path, index)))?;
            self.stack.push(frame);
            let outer = std::mem::take(&mut self.cut);
            let reached = std::mem::replace(&mut self.reached, depth);
            let made = self.values(values.iter().copied(), through, awaited, depth + 1);
            self.stack.pop();
            let height = self.reached - depth;
            self.reached = self.reached.max(reached);
            if !self.cut {
                self.returns
                    .insert((frame, through, awaited), (made.clone(), height));
            }
            self.cut |= outer;
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
