//! The messages of the checks this crate holds, in pt-BR, English and Spanish, and the language
//! the binary's catalog reads too: the CLI's `msg::init` sets it once for both (ADR 0008).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::manifest::{ADDED_BY_MAX, OWNER_MAX};

/// The language of the CLI's messages. A later minor version may add a language, so a match
/// from outside this crate needs a wildcard arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum Lang {
    En,
    Pt,
    Es,
}

static LANG: AtomicU8 = AtomicU8::new(Lang::En as u8);

/// Picks the language from the value of `LANG`: `pt` or `es` as a prefix; `None` and any other
/// value are English.
pub fn init(lang: Option<&str>) {
    let lang = match lang {
        Some(l) if l.starts_with("pt") => Lang::Pt,
        Some(l) if l.starts_with("es") => Lang::Es,
        _ => Lang::En,
    };
    LANG.store(lang as u8, Ordering::Relaxed);
}

/// The language messages are in.
pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        x if x == Lang::Pt as u8 => Lang::Pt,
        x if x == Lang::Es as u8 => Lang::Es,
        _ => Lang::En,
    }
}

/// Whether messages are in pt-BR.
#[deprecated(since = "1.2.0", note = "use `msg::lang() == msg::Lang::Pt`")]
pub fn pt() -> bool {
    lang() == Lang::Pt
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
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Kind::Table => ("a table", "uma tabela", "una tabla"),
            Kind::Array => ("an array", "uma lista", "una lista"),
            Kind::String => ("a string", "um texto", "un texto"),
            Kind::Integer => ("an integer", "um inteiro", "un entero"),
            Kind::Float => ("a float", "um decimal", "un decimal"),
            Kind::Boolean => ("a boolean", "um booleano", "un booleano"),
            Kind::Other => ("another type", "outro tipo", "otro tipo"),
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
    /// A path that is absolute or climbs out of the repository; the path is not quoted.
    NotInRepository,
    KeyUnknown {
        key: &'a str,
        file: &'a str,
    },
    /// A key with a fixed set of values, and the values it accepts (the schema's, not input).
    UnknownVariant {
        key: &'a str,
        accepted: &'a [&'a str],
    },
    /// An error no key of the file spans: its line is all there is to say.
    NoKey,
    /// A `kind` that is not a lowercase identifier; the value is not quoted.
    NotLowercase,
    /// A `[serve]` rewrite without its leading `/`; the value is not quoted.
    NoLeadingSlash,
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
    /// A version message that is empty, too long, padded or holding a control character.
    NamedMessageBad(usize),
}

impl Msg<'_> {
    pub fn text(&self) -> String {
        let lang = lang();
        let pick = |en: String, pt_br: String, es: String| match lang {
            Lang::En => en,
            Lang::Pt => pt_br,
            Lang::Es => es,
        };
        match self {
            Msg::UnsafeIdentity => pick(
                "not a repository identity: a segment is empty, `.` or `..`".into(),
                "não é uma identidade de repositório: um segmento está vazio ou é `.` ou `..`"
                    .into(),
                "no es una identidad de repositorio: un segmento está vacío o es `.` o `..`".into(),
            ),
            Msg::LocalIdentity => pick(
                "a local identity: server mode serves repositories by their remote".into(),
                "uma identidade local: o modo servidor serve repositórios pelo remoto".into(),
                "una identidad local: el modo servidor sirve repositorios por su remoto".into(),
            ),
            Msg::ListedTwice => pick(
                "listed twice".into(),
                "listado duas vezes".into(),
                "listado dos veces".into(),
            ),
            Msg::KeyMissing(key) => pick(
                format!("`{key}` is missing"),
                format!("falta `{key}`"),
                format!("falta `{key}`"),
            ),
            Msg::WrongType { key, expected } => {
                let (en, pt, es) = expected.words();
                pick(
                    format!("`{key}`: has the wrong type ({en} was expected)"),
                    format!("`{key}`: tem o tipo errado (esperava-se {pt})"),
                    format!("`{key}`: tiene el tipo equivocado (se esperaba {es})"),
                )
            }
            Msg::InvalidValue(key) => pick(
                format!("`{key}`: has an invalid value"),
                format!("`{key}`: tem um valor inválido"),
                format!("`{key}`: tiene un valor no válido"),
            ),
            Msg::NotInRepository => pick(
                "must be a path inside the repository".into(),
                "deve ser um caminho dentro do repositório".into(),
                "debe ser una ruta dentro del repositorio".into(),
            ),
            Msg::KeyUnknown { key, file } => pick(
                format!("`{key}` is not a {file} key"),
                format!("`{key}` não é uma chave do {file}"),
                format!("`{key}` no es una clave de {file}"),
            ),
            Msg::UnknownVariant { key, accepted } => {
                let list = accepted
                    .iter()
                    .map(|v| format!("`{v}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                pick(
                    format!("`{key}`: is not one of the values it accepts ({list})"),
                    format!("`{key}`: não é um dos valores que aceita ({list})"),
                    format!("`{key}`: no es uno de los valores que acepta ({list})"),
                )
            }
            Msg::NoKey => pick(
                "a value is not valid here".into(),
                "um valor não é válido aqui".into(),
                "un valor no es válido aquí".into(),
            ),
            Msg::NotLowercase => pick(
                "must be a lowercase identifier \
                 (ASCII letters, digits and hyphens, starting with a letter)"
                    .into(),
                "deve ser um identificador em minúsculas \
                 (letras ASCII, dígitos e hífens, começando por uma letra)"
                    .into(),
                "debe ser un identificador en minúsculas \
                 (letras ASCII, dígitos y guiones, empezando por una letra)"
                    .into(),
            ),
            Msg::NoLeadingSlash => pick(
                "must start with `/`".into(),
                "deve começar com `/`".into(),
                "debe empezar con `/`".into(),
            ),
            Msg::Empty => pick("is empty".into(), "está vazio".into(), "está vacío".into()),
            Msg::OwnerNotString => pick(
                "must be one string, the name of one group".into(),
                "deve ser uma única string, o nome de um grupo".into(),
                "debe ser una única string, el nombre de un grupo".into(),
            ),
            Msg::OwnerTooLong => pick(
                format!("is longer than {OWNER_MAX} characters"),
                format!("tem mais de {OWNER_MAX} caracteres"),
                format!("tiene más de {OWNER_MAX} caracteres"),
            ),
            Msg::OwnerControl => pick(
                "holds a control character".into(),
                "contém um caractere de controle".into(),
                "contiene un carácter de control".into(),
            ),
            Msg::OwnerPadded => pick(
                "has whitespace at either end".into(),
                "tem espaço em branco numa das pontas".into(),
                "tiene espacio en blanco en alguno de los extremos".into(),
            ),
            Msg::AddedByNotString => pick(
                "must be one string, a GitHub login".into(),
                "deve ser uma única string, um login do GitHub".into(),
                "debe ser una única string, un login de GitHub".into(),
            ),
            Msg::AddedByTooLong => pick(
                format!("is longer than {ADDED_BY_MAX} characters"),
                format!("tem mais de {ADDED_BY_MAX} caracteres"),
                format!("tiene más de {ADDED_BY_MAX} caracteres"),
            ),
            Msg::AddedByChars => pick(
                "holds a character other than lowercase ASCII letters, digits, `-` and `_`".into(),
                "contém um caractere além de letras ASCII minúsculas, dígitos, `-` e `_`".into(),
                "contiene un carácter distinto de letras ASCII minúsculas, dígitos, `-` y `_`"
                    .into(),
            ),
            Msg::NamedReserved(key) => pick(
                format!("`{key}` is reserved: a named workspace's entry does not set it"),
                format!("`{key}` é reservada: a entrada de um workspace nomeado não a define"),
                format!(
                    "`{key}` está reservada: la entrada de un workspace con nombre no la define"
                ),
            ),
            Msg::NamedTextTooLong(max) => pick(
                format!("the text is longer than {max} bytes"),
                format!("o texto tem mais de {max} bytes"),
                format!("el texto tiene más de {max} bytes"),
            ),
            Msg::NamedTooMany(max) => pick(
                format!("more than {max} entries"),
                format!("mais de {max} entradas"),
                format!("más de {max} entradas"),
            ),
            Msg::NamedEnvName(name) => pick(
                format!(
                    "`{name}` is not an environment variable name (a reference, never a value)"
                ),
                format!(
                    "`{name}` não é um nome de variável de ambiente (uma referência, nunca um valor)"
                ),
                format!(
                    "`{name}` no es un nombre de variable de entorno (una referencia, nunca un valor)"
                ),
            ),
            Msg::NamedDashSegment => pick(
                "the first segment `-` is reserved for named workspaces' routes".into(),
                "o primeiro segmento `-` é reservado às rotas de workspaces nomeados".into(),
                "el primer segmento `-` está reservado a las rutas de workspaces con nombre".into(),
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
                format!(
                    "un nombre tiene de 1 a {max} caracteres, sin carácter de control y sin \
                     espacios en los extremos"
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
                format!(
                    "una descripción tiene como máximo {max} caracteres, sin carácter de control y \
                     sin espacios en los extremos"
                ),
            ),
            Msg::NamedMessageBad(max) => pick(
                format!(
                    "a message is 1 to {max} characters, with no control character and no \
                     whitespace at either end"
                ),
                format!(
                    "uma mensagem tem de 1 a {max} caracteres, sem caractere de controle e sem \
                     espaço nas pontas"
                ),
                format!(
                    "un mensaje tiene de 1 a {max} caracteres, sin carácter de control y sin \
                     espacios en los extremos"
                ),
            ),
        }
    }
}
