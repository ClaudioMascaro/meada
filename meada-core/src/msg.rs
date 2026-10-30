//! The messages of the checks this crate holds, in pt-BR and English, and the language flag the
//! binary's catalog reads too: the CLI's `msg::init` sets it once for both (ADR 0008).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::manifest::{ADDED_BY_MAX, OWNER_MAX};

static PT: AtomicBool = AtomicBool::new(false);

/// Picks the language from the value of `LANG`; `None` is English.
pub fn init(lang: Option<&str>) {
    PT.store(lang.is_some_and(|l| l.starts_with("pt")), Ordering::Relaxed);
}

/// Whether messages are in pt-BR.
pub fn pt() -> bool {
    PT.load(Ordering::Relaxed)
}

/// The kind of value a key expects, worded without the value written under it.
#[derive(Clone, Copy)]
pub enum Kind {
    Table,
    Array,
    String,
    Integer,
    Float,
    Boolean,
    Other,
}

impl Kind {
    fn words(self) -> (&'static str, &'static str) {
        match self {
            Kind::Table => ("a table", "uma tabela"),
            Kind::Array => ("an array", "uma lista"),
            Kind::String => ("a string", "um texto"),
            Kind::Integer => ("an integer", "um inteiro"),
            Kind::Float => ("a float", "um decimal"),
            Kind::Boolean => ("a boolean", "um booleano"),
            Kind::Other => ("another type", "outro tipo"),
        }
    }
}

pub enum Msg<'a> {
    UnsafeIdentity,
    LocalIdentity,
    ListedTwice,
    KeyMissing(&'a str),
    WrongType {
        key: &'a str,
        expected: Kind,
    },
    InvalidValue(&'a str),
    NotInRepository(&'a str),
    KeyUnknown {
        key: &'a str,
        file: &'a str,
    },
    NotLowercase(&'a str),
    NoLeadingSlash(&'a str),
    Empty,
    OwnerNotString,
    OwnerTooLong,
    OwnerControl,
    OwnerPadded,
    AddedByNotString,
    AddedByTooLong,
    AddedByChars,
    /// A key a named workspace's entry may not hold (ADR 0021): the platform's, not the context's.
    NamedReserved(&'a str),
    /// A named workspace's text past its bound, in bytes.
    NamedTextTooLong(usize),
    /// A named workspace with more entries than its bound.
    NamedTooMany(usize),
    /// An environment-variable reference that is not a variable name.
    NamedEnvName(&'a str),
    /// An identity whose first segment is `-`, which the node's workspace route reserves.
    NamedDashSegment,
    /// A name that is empty, too long, padded or holding a control character.
    NamedNameBad(usize),
    /// A description that is too long, padded or holding a control character.
    NamedDescriptionBad(usize),
    /// A version message that is too long, padded or holding a control character.
    NamedMessageBad(usize),
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
            Msg::WrongType { key, expected } => {
                let (en, pt) = expected.words();
                pick(
                    format!("`{key}`: has the wrong type ({en} was expected)"),
                    format!("`{key}`: tem o tipo errado (era esperado {pt})"),
                )
            }
            Msg::InvalidValue(key) => pick(
                format!("`{key}`: has an invalid value"),
                format!("`{key}`: tem um valor inválido"),
            ),
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
            Msg::AddedByNotString => pick(
                "must be one string, a GitHub login".into(),
                "deve ser uma única string, um login do GitHub".into(),
            ),
            Msg::AddedByTooLong => pick(
                format!("is longer than {ADDED_BY_MAX} characters"),
                format!("tem mais de {ADDED_BY_MAX} caracteres"),
            ),
            Msg::AddedByChars => pick(
                "holds a character other than lowercase ASCII letters, digits, `-` and `_`".into(),
                "contém um caractere além de letras ASCII minúsculas, dígitos, `-` e `_`".into(),
            ),
            Msg::NamedReserved(key) => pick(
                format!("`{key}` is reserved: a named workspace's entry does not set it"),
                format!("`{key}` é reservada: a entrada de um workspace nomeado não a define"),
            ),
            Msg::NamedTextTooLong(max) => pick(
                format!("the text is longer than {max} bytes"),
                format!("o texto tem mais de {max} bytes"),
            ),
            Msg::NamedTooMany(max) => pick(
                format!("more than {max} entries"),
                format!("mais de {max} entradas"),
            ),
            Msg::NamedEnvName(name) => pick(
                format!(
                    "`{name}` is not an environment variable name (a reference, never a value)"
                ),
                format!(
                    "`{name}` não é um nome de variável de ambiente (uma referência, nunca um valor)"
                ),
            ),
            Msg::NamedDashSegment => pick(
                "the first segment `-` is reserved for named workspaces' routes".into(),
                "o primeiro segmento `-` é reservado às rotas de workspaces nomeados".into(),
            ),
            Msg::NamedNameBad(max) => pick(
                format!(
                    "a name is 1 to {max} characters, with no control character and no \
                     whitespace at either end"
                ),
                format!(
                    "um nome tem de 1 a {max} caracteres, sem caractere de controle e sem \
                     espaço nas pontas"
                ),
            ),
            Msg::NamedDescriptionBad(max) => pick(
                format!(
                    "a description is at most {max} characters, with no control character and no \
                     whitespace at either end"
                ),
                format!(
                    "uma descrição tem no máximo {max} caracteres, sem caractere de controle e sem \
                     espaço nas pontas"
                ),
            ),
            Msg::NamedMessageBad(max) => pick(
                format!(
                    "a message is at most {max} characters, with no control character and no \
                     whitespace at either end"
                ),
                format!(
                    "uma mensagem tem no máximo {max} caracteres, sem caractere de controle e sem \
                     espaço nas pontas"
                ),
            ),
        }
    }
}
