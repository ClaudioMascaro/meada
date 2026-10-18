use extras::boost;
use shared_core::api::Client;
use shared_core::greet;

fn main() {
    welcome();
}

fn welcome() -> String {
    greet("web")
}

fn connect() -> Client {
    Client::create()
}

fn lift() -> u32 {
    boost()
}

fn faraway() -> u32 {
    away::boost()
}

fn haunt() -> u32 {
    ghost::boost()
}

fn peek() -> String {
    shared_core::text::greet("p")
}

fn encode() {
    serde::ser::to_value();
}
