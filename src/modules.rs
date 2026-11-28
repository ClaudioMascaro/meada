use std::path::Path;

use toml::de::DeTable;

use crate::Result;
use crate::msg::Msg;

pub struct Module {
    pub section: &'static str,
    pub feature: &'static str,
    pub built: bool,
}

pub const MODULES: [Module; 13] = [
    Module {
        section: "valkey",
        feature: "valkey",
        built: cfg!(feature = "valkey"),
    },
    Module {
        section: "snapshot",
        feature: "snapshot",
        built: cfg!(feature = "snapshot"),
    },
    Module {
        section: "generations",
        feature: "generation-pointer",
        built: cfg!(feature = "generation-pointer"),
    },
    Module {
        section: "refresh_lock",
        feature: "refresh-lock",
        built: cfg!(feature = "refresh-lock"),
    },
    Module {
        section: "oidc",
        feature: "oidc",
        built: cfg!(feature = "oidc"),
    },
    Module {
        section: "github_app",
        feature: "github-app",
        built: cfg!(feature = "github-app"),
    },
    Module {
        section: "ui",
        feature: "ui",
        built: cfg!(feature = "ui"),
    },
    Module {
        section: "postgres",
        feature: "postgres",
        built: cfg!(feature = "postgres"),
    },
    Module {
        section: "directory",
        feature: "directory",
        built: cfg!(feature = "directory"),
    },
    Module {
        section: "catalog",
        feature: "workspaces",
        built: cfg!(feature = "workspaces"),
    },
    Module {
        section: "workspaces",
        feature: "workspaces",
        built: cfg!(feature = "workspaces"),
    },
    Module {
        section: "proposals",
        feature: "proposals",
        built: cfg!(feature = "proposals"),
    },
    Module {
        section: "accounts",
        feature: "accounts",
        built: cfg!(feature = "accounts"),
    },
];

pub fn check(file: &Path, src: &str) -> Result<()> {
    let found = first_key(src, |key| {
        MODULES.iter().find(|m| !m.built && m.section == key)
    });
    let Some((at, module)) = found else {
        return Ok(());
    };
    let (section, feature) = (module.section, module.feature);
    let what = Msg::NeedsFeature { section, feature }.text();
    Err(located(file, src, at, &what))
}

#[cfg(any(
    feature = "generation-pointer",
    feature = "github-app",
    feature = "directory",
    feature = "accounts"
))]
pub fn needs(file: &Path, src: &str, section: &str, missing: &[&str]) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let what = Msg::NeedsSections {
        section,
        needed: missing,
    }
    .text();
    Err(at_section(file, src, section, &what))
}

#[cfg(any(
    feature = "generation-pointer",
    feature = "github-app",
    feature = "directory",
    feature = "accounts"
))]
pub fn at_section(file: &Path, src: &str, section: &str, what: &str) -> Box<dyn std::error::Error> {
    let at = first_key(src, |key| (key == section).then_some(())).map_or(0, |(at, ())| at);
    located(file, src, at, what)
}

fn first_key<T>(src: &str, pick: impl Fn(&str) -> Option<T>) -> Option<(usize, T)> {
    let table = DeTable::parse(src).ok()?;
    (table.get_ref().iter())
        .find_map(|(key, _)| Some((key.span().start, pick(key.get_ref().as_ref())?)))
}

fn located(file: &Path, src: &str, at: usize, what: &str) -> Box<dyn std::error::Error> {
    let line = crate::config::line_of(src, at);
    format!("{}:{line}: {what}", file.display()).into()
}

#[cfg(any(
    feature = "valkey",
    feature = "snapshot",
    feature = "oidc",
    feature = "ui",
    feature = "postgres"
))]
pub type Problem = (&'static str, String);

#[cfg(any(
    feature = "valkey",
    feature = "snapshot",
    feature = "oidc",
    feature = "postgres"
))]
pub fn validated<S, C>(
    file: &Path,
    src: &str,
    section: Option<S>,
    validate: impl FnOnce(S) -> std::result::Result<C, Problem>,
) -> Result<Option<C>> {
    (section.map(validate).transpose())
        .map_err(|(key, problem)| crate::config::error_at(file, src, key, &problem))
}

#[cfg(any(
    feature = "generation-pointer",
    feature = "oidc",
    feature = "github-app",
    feature = "postgres"
))]
pub fn at_least(
    key: &'static str,
    value: Option<i64>,
    default: i64,
    min: i64,
) -> std::result::Result<u64, Problem> {
    match u64::try_from(value.unwrap_or(default)) {
        Ok(value) if value >= min as u64 => Ok(value),
        _ => Err((key, Msg::AtLeast(min).text())),
    }
}

/// Where a secret comes from: an environment variable or a file. Nothing here is a secret.
#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
#[derive(Clone, Debug, PartialEq)]
pub enum Secret {
    Env(String),
    File(std::path::PathBuf),
}

#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
impl Secret {
    pub fn named(
        env: Option<String>,
        file: Option<String>,
        base: &Path,
        both: impl FnOnce() -> Problem,
    ) -> std::result::Result<Option<Self>, Problem> {
        match (env, file) {
            (Some(_), Some(_)) => Err(both()),
            (Some(var), None) => Ok(Some(Self::Env(var))),
            (None, Some(file)) => Ok(Some(Self::File(base.join(file)))),
            (None, None) => Ok(None),
        }
    }

    pub fn read(&self, max: u64) -> std::result::Result<Option<String>, String> {
        match self {
            Self::Env(var) => Ok(std::env::var(var).ok()),
            Self::File(file) => read_secret(file, max).map(Some),
        }
    }
}

#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
pub fn read_utf8(path: &Path, max: u64) -> std::result::Result<String, String> {
    let bytes = crate::config::read_bounded(path, max)?;
    String::from_utf8(bytes).map_err(|_| format!("{}: not UTF-8", path.display()))
}

#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
pub fn read_secret(path: &Path, max: u64) -> std::result::Result<String, String> {
    let text = read_utf8(path, max)?;
    Ok(text.trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
pub fn block_on<T>(run: impl Future<Output = T>) -> std::io::Result<T> {
    let runtime = (tokio::runtime::Builder::new_current_thread())
        .enable_all()
        .build()?;
    let out = runtime.block_on(run);
    runtime.shutdown_background();
    Ok(out)
}

#[cfg(any(feature = "valkey", feature = "snapshot", feature = "postgres"))]
pub fn within<T>(
    bound: std::time::Duration,
    ask: impl Future<Output = std::result::Result<T, String>>,
    timeout: fn(u64) -> Msg<'static>,
) -> std::result::Result<T, String> {
    let answer = block_on(async { tokio::time::timeout(bound, ask).await });
    let answer = answer.map_err(|e| e.to_string())?;
    answer.unwrap_or_else(|_| Err(timeout(bound.as_millis() as u64).text()))
}

#[cfg(any(feature = "oidc", feature = "github-app"))]
pub fn agent(timeout: std::time::Duration) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .proxy(None)
        .tls_config(tls)
        .build()
        .into()
}

#[cfg(any(
    feature = "valkey",
    feature = "snapshot",
    feature = "oidc",
    feature = "ui",
    feature = "postgres"
))]
pub fn loopback(host: &str) -> bool {
    let ip = (host.strip_prefix('[').and_then(|h| h.strip_suffix(']'))).unwrap_or(host);
    host == "localhost"
        || ip
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(any(feature = "oidc", feature = "github-app", feature = "ui"))]
#[derive(Clone, Copy, PartialEq)]
pub enum Shape {
    #[cfg(any(feature = "oidc", feature = "github-app"))]
    Path,
    Bare,
    Query,
}

#[cfg(any(feature = "oidc", feature = "github-app", feature = "ui"))]
pub fn reachable(url: &str, shape: Shape) -> bool {
    let Ok(uri) = url.parse::<ureq::http::Uri>() else {
        return false;
    };
    let (Some(authority), Some(host)) = (uri.authority(), uri.host()) else {
        return false;
    };
    if authority.as_str().contains('@') || url.contains('#') {
        return false;
    }
    if uri.query().is_some() && shape != Shape::Query {
        return false;
    }
    if shape == Shape::Bare && !matches!(uri.path(), "" | "/") {
        return false;
    }
    match uri.scheme_str() {
        Some("https") => true,
        Some("http") => loopback(host),
        _ => false,
    }
}

#[cfg(feature = "refresh-lock")]
pub fn cli_author() -> String {
    crate::snapshot::lock::cli_author()
}

#[cfg(all(
    not(feature = "refresh-lock"),
    any(
        feature = "generation-pointer",
        all(feature = "ui", feature = "accounts")
    )
))]
pub fn cli_author() -> String {
    let var = |names: [&str; 2]| names.iter().find_map(|n| std::env::var(n).ok());
    let login = var(["USER", "USERNAME"]).unwrap_or_default();
    let host = var(["HOSTNAME", "COMPUTERNAME"]).unwrap_or_default();
    format!("cli:{login}@{}", host.to_lowercase())
}

#[cfg(any(feature = "generation-pointer", feature = "oidc"))]
pub fn now() -> u64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| d.as_secs())
}

#[cfg(any(feature = "generation-pointer", all(feature = "oidc", feature = "ui")))]
pub fn rfc3339(secs: u64) -> String {
    let (days, day) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, s) = (day / 3600, day % 3600 / 60, day % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

#[cfg_attr(
    not(all(feature = "github-app", feature = "postgres", feature = "oidc")),
    expect(dead_code)
)]
pub(crate) fn fold(host: &str, identity: &str) -> String {
    let on_host = (identity.strip_prefix(host)).is_some_and(|rest| rest.starts_with('/'));
    if on_host {
        identity.to_lowercase()
    } else {
        identity.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[cfg(not(feature = "snapshot"))]
    #[test]
    fn a_section_whose_feature_is_missing_names_the_line_the_section_and_the_feature() {
        let src = "index_dir = \"i\"\n\n[snapshot]\nbucket = \"b\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:3: [snapshot] needs a Meada built with the snapshot feature"
        );
        let err = check(Path::new("m.toml"), "[generations]\n")
            .unwrap_err()
            .to_string();
        assert!(err.ends_with("the generation-pointer feature"), "{err}");
    }

    #[cfg(not(feature = "refresh-lock"))]
    #[test]
    fn the_refresh_lock_section_without_its_feature_names_it() {
        let src = "index_dir = \"i\"\n[refresh_lock]\nttl_seconds = 300\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:2: [refresh_lock] needs a Meada built with the refresh-lock feature"
        );
    }

    #[cfg(not(feature = "oidc"))]
    #[test]
    fn the_oidc_section_without_its_feature_names_it() {
        let src = "index_dir = \"i\"\n\n[oidc]\nissuer = \"https://id.example\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:3: [oidc] needs a Meada built with the oidc feature"
        );
    }

    #[cfg(not(feature = "ui"))]
    #[test]
    fn the_ui_section_without_its_feature_names_it() {
        let src = "index_dir = \"i\"\n[ui]\ndir = \"ui\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:2: [ui] needs a Meada built with the ui feature"
        );
    }

    #[cfg(not(feature = "postgres"))]
    #[test]
    fn the_postgres_section_without_its_feature_names_it() {
        let src = "index_dir = \"i\"\n\n\n[postgres]\nschema = \"meada\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:4: [postgres] needs a Meada built with the postgres feature"
        );
    }

    #[cfg(not(feature = "accounts"))]
    #[test]
    fn the_accounts_section_without_its_feature_names_it() {
        let src = "index_dir = \"i\"\n[accounts]\npublic_key_file = \"k.pem\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:2: [accounts] needs a Meada built with the accounts feature"
        );
    }

    #[cfg(not(feature = "github-app"))]
    #[test]
    fn the_github_app_blocks_without_their_feature_name_it() {
        let src = "index_dir = \"i\"\n\n[[github_app]]\nhost = \"github.com\"\n";
        let err = check(Path::new("m.toml"), src).unwrap_err().to_string();
        assert_eq!(
            err,
            "m.toml:3: [github_app] needs a Meada built with the github-app feature"
        );
    }

    #[cfg(any(feature = "valkey", feature = "snapshot", feature = "oidc"))]
    #[test]
    fn loopback_is_localhost_127_slash_8_or_1_and_nothing_that_looks_like_them() {
        for host in ["localhost", "127.0.0.1", "127.9.9.9", "::1", "[::1]"] {
            assert!(loopback(host), "{host}");
        }
        let names = ["localhost.evil.com", "127.0.0.1.nip.io", "LOCALHOST.evil"];
        for host in names
            .into_iter()
            .chain(["[::ffff:127.0.0.1]", "[[::1]]", "10.0.0.1", ""])
        {
            assert!(!loopback(host), "{host}");
        }
    }

    #[test]
    fn unlisted_sections_and_unparsable_text_pass_to_the_typed_read() {
        for src in ["[cache]\nurl = \"x\"\n", "index_dir = \n", ""] {
            assert!(check(Path::new("m.toml"), src).is_ok(), "{src}");
        }
    }
}
