use crate::util as u;

pub struct Invoice {
    pub cents: u32,
}

impl Invoice {
    pub fn new() -> Self {
        Invoice { cents: 0 }
    }

    pub fn amount(&self) -> u32 {
        self.cents
    }
}

pub fn tax() -> u32 {
    u::round(1)
}
