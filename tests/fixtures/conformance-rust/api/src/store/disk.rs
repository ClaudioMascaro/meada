pub fn open() {}

impl super::Store {
    pub fn reload() -> u32 {
        open();
        Self::check()
    }

    fn check() -> u32 {
        1
    }
}
