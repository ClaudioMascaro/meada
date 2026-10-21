//! The cloud modules' sections of `meada.workspace.toml` (ADR 0012): the table from each section
//! to the cargo feature that owns it, in every build. A section in a build without its feature
//! is an error naming the feature; a section the table does not list stays an unknown key.
//! A module ticket registers its section here and sets `built` to its own `cfg!(feature = ..)`.

use std::path::Path;

use toml::de::DeTable;

use crate::Result;
use crate::msg::Msg;

pub struct Module {
    pub section: &'static str,
    pub feature: &'static str,
    /// Whether this build has the feature.
    pub built: bool,
}

pub const MODULES: [Module; 6] = [
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
    // The sections below belong to modules not built yet: no build has their feature.
    Module {
        section: "sessions",
        feature: "session-store",
        built: false,
    },
    Module {
        section: "oidc",
        feature: "oidc",
        built: false,
    },
];

/// The first section of `src`, the content of `file`, whose feature this build lacks. A file
/// that does not parse passes: the typed read names its syntax error.
pub fn check(file: &Path, src: &str) -> Result<()> {
    let Ok(table) = DeTable::parse(src) else {
        return Ok(());
    };
    let found = table.get_ref().iter().find_map(|(key, _)| {
        let module = MODULES
            .iter()
            .find(|m| !m.built && m.section == key.get_ref().as_ref())?;
        Some((key.span().start, module))
    });
    let Some((at, module)) = found else {
        return Ok(());
    };
    let (section, feature) = (module.section, module.feature);
    let what = Msg::NeedsFeature { section, feature }.text();
    let line = crate::config::line_of(src, at);
    Err(format!("{}:{line}: {what}", file.display()).into())
}

/// The error for `section` of `src`, the content of `file`, when the sections it needs that
/// `missing` names are not there: `[section] needs [missing...]`, at the section's line.
#[cfg(feature = "generation-pointer")]
pub fn needs(file: &Path, src: &str, section: &str, missing: &[&str]) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let at = (DeTable::parse(src).ok())
        .and_then(|table| {
            let table = table.get_ref();
            let key = table.keys().find(|key| key.get_ref().as_ref() == section)?;
            Some(key.span().start)
        })
        .unwrap_or(0);
    let what = Msg::NeedsSections {
        section,
        needed: missing,
    }
    .text();
    let line = crate::config::line_of(src, at);
    Err(format!("{}:{line}: {what}", file.display()).into())
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
        // A module's feature may be named otherwise than its section.
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

    #[test]
    fn unlisted_sections_and_unparsable_text_pass_to_the_typed_read() {
        for src in ["[cache]\nurl = \"x\"\n", "index_dir = \n", ""] {
            assert!(check(Path::new("m.toml"), src).is_ok(), "{src}");
        }
    }
}
