pub fn greet(name: &str) -> String {
    shout(&format!("hello {name}"))
}

pub fn shout(text: &str) -> String {
    text.to_uppercase()
}
