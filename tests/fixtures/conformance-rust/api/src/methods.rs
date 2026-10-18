// Method calls: a receiver whose type the source writes reaches the method of that type; any
// other goes by name. `bump` and `read` are on two types, `reset` on one, so a name match alone
// is `ambiguous` for the first two and `inferred` for the last.
use std::collections::HashMap;

pub trait Probe {
    fn probe(&self) -> u32;

    fn summary(&self) -> u32 {
        self.probe()
    }
}

pub struct Counter {
    hits: u32,
}

pub struct Gauge {
    level: u32,
    marks: HashMap<String, u32>,
}

pub struct Dash {
    counter: Counter,
    gauge: Gauge,
    marks: HashMap<String, u32>,
    label: String,
}

impl Counter {
    pub fn new() -> Self {
        Counter { hits: 0 }
    }

    pub fn bump(&mut self) {
        self.hits += 1;
    }

    pub fn read(&self) -> u32 {
        self.hits
    }

    pub fn reset(&mut self) {}

    pub fn twice(&mut self) {
        self.bump();
        self.bump();
    }
}

impl Gauge {
    pub fn new() -> Gauge {
        Gauge {
            level: 0,
            marks: HashMap::new(),
        }
    }

    pub fn bump(&mut self) {}

    pub fn read(&self) -> u32 {
        self.level
    }

    pub fn insert(&mut self, key: String) {
        self.marks.insert(key, 1);
    }
}

impl Probe for Counter {
    fn probe(&self) -> u32 {
        self.read()
    }
}

impl Probe for Gauge {
    fn probe(&self) -> u32 {
        self.read()
    }
}

pub fn fresh() -> Gauge {
    Gauge::new()
}

pub fn pick() -> u32 {
    0
}

// The type is written on a `let`, on a parameter (`&T`, `&mut T`) or on a field.
pub fn annotated() {
    let c: Counter = Counter::new();
    c.bump();
}

pub fn shared(c: &Counter) {
    c.read();
}

pub fn exclusive(g: &mut Gauge) {
    g.bump();
}

impl Dash {
    pub fn tick(&mut self) {
        self.counter.bump();
        self.gauge.bump();
        self.counter.twice();
    }

    // A field of a type outside the repository is opaque: `insert` is not `Gauge.insert`.
    pub fn mark(&mut self, key: String) {
        self.marks.insert(key, 1);
        self.label.len();
    }
}

// The type is the declared return type of the function the `let` calls, `Self` included.
pub fn built() {
    let c = Counter::new();
    c.bump();
    let g = fresh();
    g.read();
}

// A trait's methods and its default bodies are found through the receiver's type.
pub fn probed(c: Counter) {
    c.probe();
    c.summary();
}

// A `dyn Trait` or a generic bounded by a trait stops at the trait: no implementation is
// searched.
pub fn dynamic(p: &dyn Probe) {
    p.probe();
}

pub fn generic<T: Probe>(p: &T) {
    p.probe();
}

pub fn bounded<T>(p: T)
where
    T: Probe,
{
    p.summary();
}

pub fn opaque_impl(p: impl Probe) {
    p.probe();
}

// The type is not written: the call goes by name.
pub fn named(items: Vec<Counter>) {
    for c in items {
        c.bump();
        c.reset();
    }
}

// A type from outside the repository does not fall to a name match.
pub fn outside(m: HashMap<String, u32>, key: String) {
    m.insert(key, 1);
    let made = HashMap::new();
    made.insert(pick(), 1);
}

// A binding of the name that disagrees hides the typed one.
pub fn shadowed(c: &Counter) {
    let c = pick();
    c.bump();
}

pub fn retyped(c: &Counter) {
    let c: Gauge = fresh();
    c.bump();
}

pub fn blocks(items: Vec<Gauge>) {
    let c = &items[0];
    c.read();
}
