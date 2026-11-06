//! Jib build destinations, using the shared Gradle syntax scanner.
use crate::jvm::gradle::{GRADLE_DEPTH, Token, tokens};
use crate::lang::MAX_BYTES;

/// Literal destination images of the Jib plugin. Shares the Gradle token scanner, so comments,
/// computed strings and unrelated image assignments cannot supply a destination.
pub(super) fn gradle(src: &[u8]) -> Vec<String> {
    if src.len() > MAX_BYTES {
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(src) else {
        return Vec::new();
    };
    let tokens = tokens(text);
    let mut blocks = Vec::new();
    let mut out = Vec::new();
    let mut enabled = false;
    for (i, token) in tokens.iter().enumerate() {
        if blocks == ["plugins"] && matches!(token, Token::Word(w) if w == "id") {
            let rest = &tokens[i + 1..];
            enabled |= matches!(rest, [Token::String(s, false), ..] if s == "com.google.cloud.tools.jib")
                || matches!(rest, [Token::Mark('('), Token::String(s, false), Token::Mark(')'), ..] if s == "com.google.cloud.tools.jib");
        }
        if blocks.is_empty() && matches!(token, Token::Word(w) if w == "apply") {
            enabled |= matches!(&tokens[i + 1..], [Token::Word(w), Token::Mark(':'), Token::String(s, false), ..] if w == "plugin" && s == "com.google.cloud.tools.jib");
        }
        match token {
            Token::Mark('{') => {
                if blocks.len() >= GRADLE_DEPTH {
                    return Vec::new();
                }
                blocks.push(
                    i.checked_sub(1)
                        .and_then(|i| tokens[i].word())
                        .unwrap_or(""),
                );
            }
            Token::Mark('}') => {
                blocks.pop();
            }
            Token::Word(word) if enabled && word == "image" && blocks == ["jib", "to"] => {
                if let [
                    Token::Mark('='),
                    Token::String(image, false),
                    Token::Mark('\n' | ';' | '}'),
                    ..,
                ] = &tokens[i + 1..]
                {
                    out.push(image.clone());
                }
            }
            _ => {}
        }
    }
    out
}
