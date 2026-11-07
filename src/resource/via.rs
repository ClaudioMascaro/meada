//! The resource kind's indexed declaration locations and their context (#641).

use std::collections::BTreeSet;

use crate::config_values::sites;
use crate::contract::Link;
use crate::index::Export;
use crate::workspace::Repository;

pub(super) fn read(
    repo: &Repository,
    db: &rusqlite::Connection,
    export: &mut Export,
) -> crate::Result<()> {
    let vias: BTreeSet<&str> = repo
        .config
        .resources
        .iter()
        .flat_map(|r| &r.via)
        .map(String::as_str)
        .filter(|v| v.contains('#'))
        .collect();
    for via in vias {
        export
            .config_sites
            .extend(sites::locate(db, &repo.identity, via)?);
    }
    Ok(())
}

pub(super) fn context(exports: &[Export], links: &[Link], site: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for export in exports {
        if !export.config_sites.iter().any(|s| s.id == site)
            && !export
                .iac
                .iter()
                .any(|n| n.identity(&export.repository) == site)
        {
            continue;
        }
        for (contract, writes) in export
            .providers
            .iter()
            .filter(|p| p.site == site && p.contract.starts_with(super::PREFIX))
            .map(|p| (&p.contract, true))
            .chain(
                export
                    .consumers
                    .iter()
                    .filter(|c| c.site == site)
                    .filter_map(|c| c.contract.as_ref())
                    .filter(|c| c.starts_with(super::PREFIX))
                    .map(|c| (c, false)),
            )
        {
            lines.push(format!(
                "{} {contract}",
                if writes { "provides" } else { "consumes" }
            ));
            for link in links.iter().filter(|l| {
                l.contract == *contract
                    && if writes {
                        l.provider == site
                    } else {
                        l.consumer == site
                    }
            }) {
                lines.push(format!(
                    "  {} {} [{}: {}]",
                    if writes { "reader:" } else { "writer:" },
                    if writes {
                        &link.consumer
                    } else {
                        &link.provider
                    },
                    link.level.name(),
                    link.reason
                ));
            }
        }
    }
    lines
}
