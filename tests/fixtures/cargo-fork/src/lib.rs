pub fn greet(name: &str) -> String {
    format!("hi {name}")
}

pub fn shout(text: &str) -> String {
    text.to_uppercase()
}
