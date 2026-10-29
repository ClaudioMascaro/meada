//! The messages of the checks this crate holds, in pt-BR and English, and the language flag the
//! binary's catalog reads too: the CLI's `msg::init` sets it once for both (ADR 0008).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::manifest::OWNER_MAX;

static PT: AtomicBool = AtomicBool::new(false);

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    PT.store(lang.is_some_and(|l| l.starts_with("pt")), Ordering::Relaxed);
}

/// Whether messages are in pt-BR.
pub fn pt() -> bool {
    PT.load(Ordering::Relaxed)
}

pub enum Msg<'a> {
    UnsafeIdentity,
    LocalIdentity,
    ListedTwice,
    KeyMissing(&'a str),
    NotInRepository(&'a str),
    KeyUnknown { key: &'a str, file: &'a str },
    NotLowercase(&'a str),
    NoLeadingSlash(&'a str),
    Empty,
    OwnerNotString,
    OwnerTooLong,
    OwnerControl,
    OwnerPadded,
}

impl Msg<'_> {
    pub fn text(&self) -> String {
        let pt = pt();
        let pick = |en: String, pt_br: String| if pt { pt_br } else { en };
        match self {
            Msg::UnsafeIdentity => pick(
                "not a repository identity: a segment is empty, `.` or `..`".into(),
                "não é uma identidade de repositório: um segmento está vazio ou é `.` ou `..`"
                    .into(),
            ),
            Msg::LocalIdentity => pick(
                "a local identity: server mode serves repositories by their remote".into(),
                "uma identidade local: o modo servidor serve repositórios pelo remoto".into(),
            ),
            Msg::ListedTwice => pick("listed twice".into(), "listado duas vezes".into()),
            Msg::KeyMissing(key) => pick(format!("`{key}` is missing"), format!("falta `{key}`")),
            Msg::NotInRepository(path) => pick(
                format!("`{path}` is not a path inside the repository"),
                format!("`{path}` não é um caminho dentro do repositório"),
            ),
            Msg::KeyUnknown { key, file } => pick(
                format!("`{key}` is not a {file} key"),
                format!("`{key}` não é uma chave do {file}"),
            ),
            Msg::NotLowercase(value) => pick(
                format!(
                    "`{value}` is not a lowercase identifier \
                     (ASCII letters, digits and hyphens, starting with a letter)"
                ),
                format!(
                    "`{value}` não é um identificador em minúsculas \
                     (letras ASCII, dígitos e hífens, começando por uma letra)"
                ),
            ),
            Msg::NoLeadingSlash(value) => pick(
                format!("`{value}` must start with `/`"),
                format!("`{value}` deve começar com `/`"),
            ),
            Msg::Empty => pick("is empty".into(), "está vazio".into()),
            Msg::OwnerNotString => pick(
                "must be one string, the name of one group".into(),
                "deve ser uma única string, o nome de um grupo".into(),
            ),
            Msg::OwnerTooLong => pick(
                format!("is longer than {OWNER_MAX} characters"),
                format!("tem mais de {OWNER_MAX} caracteres"),
            ),
            Msg::OwnerControl => pick(
                "holds a control character".into(),
                "contém um caractere de controle".into(),
            ),
            Msg::OwnerPadded => pick(
                "has whitespace at either end".into(),
                "tem espaço em branco numa das pontas".into(),
            ),
        }
    }
}
