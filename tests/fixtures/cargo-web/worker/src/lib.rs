pub fn run() -> String {
    shared_core::greet("worker")
}

pub fn loud() -> String {
    kit::shout("worker")
}
