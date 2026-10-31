//! The catalog listing of the role check (#341, ADR 0021 contract 1): which repositories the
//! Apps' installations cover and `[catalog] allow` admits, listed with the installation tokens
//! `Run` hands out, under the same throttle marks as every other REST use of the module. It is
//! coverage and no entitlement: the collaborators of each candidate are listed afterwards, and
//! only a login holding an accepted role on one makes it selectable for that login.
//!
//! A listing that fails fails the role check and nothing is written: a partial candidate set
//! would revoke repositories or mix two sets, and the previous set keeps aging until a run
//! succeeds.

use std::collections::BTreeSet;

use super::*;
use crate::catalog::Config as Catalog;

/// The installations the role check lists the catalog through: every installation of the host's
/// Apps whose owner some `[catalog] allow` pattern names, as owner and `Candidate`, ordered by
/// owner, App id and installation id. Each App's installations are listed again first when due,
/// as a `get` does; a listing that fails fails the run.
async fn installations<'c>(
    run: &mut Run<'_>,
    apps: &[&'c Config],
    owners: &BTreeSet<&str>,
) -> Result<Vec<(String, Candidate<'c>)>, String> {
    let mut found = Vec::new();
    for app in apps {
        let failed = |why: &str| {
            let app = app.app_id;
            Msg::RolesCatalogInstallations { app, why }.text()
        };
        // A listing that fails, or nothing held and no recent listing (the App throttled, or
        // another run's listing unfinished at the lock's end), fails the run: skipping its
        // installations would drop their repositories from every caller's catalog. An App
        // listed with no installation holds nothing but is `listed`.
        run.refresh(app).await.map_err(|e| failed(&e))?;
        let held = run.app_key(app, "installations");
        let listed = run.app_key(app, "listed");
        if !run.exists(&held).await.map_err(|e| failed(&e))?
            && !run.exists(&listed).await.map_err(|e| failed(&e))?
        {
            let throttled = run.app_key(app, "throttled");
            let why = if run.exists(&throttled).await.map_err(|e| failed(&e))? {
                Msg::RolesCatalogThrottled(None)
            } else {
                Msg::RolesCatalogUnlisted
            };
            return Err(failed(&why.text()));
        }
        let all: Vec<(String, String)> =
            (run.valkey.query(cmd("HGETALL").arg(&held)).await).map_err(|e| failed(&e))?;
        for (owner, id) in all {
            let Some(id) = id.parse::<u64>().ok() else {
                continue;
            };
            if owners.contains(owner.as_str()) {
                found.push((owner, Candidate { app, id }));
            }
        }
    }
    found.sort_by(|a, b| (&a.0, a.1.app.app_id, a.1.id).cmp(&(&b.0, b.1.app.app_id, b.1.id)));
    Ok(found)
}

/// The repositories of one installation, as `owner/repo` pairs in lowercase: every page of
/// `GET /installation/repositories` with its token, up to `PAGES_MAX`. The answer is hostile: a
/// page that is not the documented object, a name that is not `<owner>/<repo>` and a list that
/// runs past the last page fail or are skipped, never trusted.
fn repositories(
    agent: &ureq::Agent,
    app: &Config,
    token: &str,
) -> Result<Vec<(String, String)>, Failure> {
    #[derive(Deserialize)]
    struct Page {
        repositories: Vec<Named>,
    }
    #[derive(Deserialize)]
    struct Named {
        full_name: String,
    }
    let mut all = Vec::new();
    for page in 1..=PAGES_MAX {
        let url = format!(
            "{}/installation/repositories?per_page={PER_PAGE}&page={page}",
            app.api
        );
        let (body, _) = call(agent, false, &url, token)?;
        let found: Page = (serde_json::from_slice(&body))
            .map_err(|_| Failure::Other(Msg::GithubAppAnswer(&url).text()))?;
        let full = found.repositories.len() == PER_PAGE;
        for named in found.repositories {
            if let Some(pair) = repository(&named.full_name) {
                all.push(pair);
            }
        }
        if !full {
            return Ok(all);
        }
    }
    Err(Failure::Other(Msg::RolesCatalogPages(PAGES_MAX).text()))
}

/// How many of `found` the cap counts: those `selected` does not hold. `found` is lowercase
/// (`repository`) and the caller folds `selected` as GitHub compares it (#508), so a selected
/// identity written with uppercase letters is not one more.
fn extra(found: &BTreeSet<String>, selected: &BTreeSet<&str>) -> usize {
    found
        .iter()
        .filter(|i| !selected.contains(i.as_str()))
        .count()
}

/// The catalog's candidates on `host`: the identities the installations cover that `config`
/// allows, sorted. A candidate set with more identities than `selected` holds, plus
/// `[catalog] max_repositories`, fails the run (the selected identities are not counted).
pub(super) async fn candidates(
    run: &mut Run<'_>,
    apps: &[&Config],
    host: &str,
    config: &Catalog,
    selected: &BTreeSet<&str>,
) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    for (_, at) in installations(run, apps, &config.owners()).await? {
        let failed = |why: &str| {
            let installation = at.id;
            Msg::RolesCatalogFailed { installation, why }.text()
        };
        let throttled = run.installation_key(host, at.id, "throttled");
        if run.exists(&throttled).await.map_err(|e| failed(&e))? {
            return Err(failed(&Msg::RolesCatalogThrottled(None).text()));
        }
        let Some((token, _)) = run.token(at).await.map_err(|e| failed(&e))? else {
            return Err(failed(&Msg::RolesCatalogThrottled(None).text()));
        };
        match repositories(&run.agent, at.app, &token) {
            Ok(listed) => {
                let allowed = listed.into_iter().filter(|(o, r)| config.allows(o, r));
                found.extend(allowed.map(|(owner, repo)| format!("{host}/{owner}/{repo}")));
            }
            Err(Failure::Throttled(status, wait)) => {
                let secs = run.mark(&throttled, wait).await.map_err(|e| failed(&e))?;
                let what = format!("installation {}", at.id);
                run.notes
                    .push(Msg::GithubAppMarked { what: &what, secs }.text());
                return Err(failed(&Msg::RolesCatalogThrottled(Some(status)).text()));
            }
            Err(Failure::Other(why) | Failure::Missing(why)) => return Err(failed(&why)),
        }
    }
    let extra = extra(&found, selected);
    let max = config.max_repositories;
    if extra > max {
        return Err(Msg::RolesCatalogCap { found: extra, max }.text());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selected_identity_with_uppercase_letters_is_not_an_extra_repository() {
        let found: BTreeSet<String> = ["github.com/acme/api", "github.com/acme/web"]
            .map(String::from)
            .into();
        let written = crate::modules::fold("github.com", "github.com/Acme/API");
        let selected = BTreeSet::from([written.as_str()]);
        assert_eq!(extra(&found, &selected), 1);
    }
}
