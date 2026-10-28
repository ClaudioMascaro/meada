//! Operational reads. The console only coordinates the owning modules' checked readers;
//! a request's complete service read is bounded, including lists and document downloads.

use super::*;

// ponytail: at most four operational readers run at once; a timed-out reader keeps its
// permit until blocking I/O actually finishes, so later requests cannot leak more workers.
pub(super) const READERS_MAX: usize = 4;

/// Unlike the CLI helper, this runtime is dropped on the worker while its permit is held.
/// A resolver or credential read left on its blocking pool cannot outlive the concurrency cap.
fn read_runtime<T>(
    read: impl Future<Output = std::result::Result<T, String>>,
    reading: &Reading,
) -> std::result::Result<T, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("console: {e}"))?;
    let answer = runtime.block_on(async {
        tokio::time::timeout_at(reading.deadline, read)
            .await
            .unwrap_or_else(|_| Err(timed_out(reading.get())))
    });
    drop(runtime);
    answer
}

struct Reading {
    service: std::sync::Mutex<&'static str>,
    deadline: tokio::time::Instant,
}

impl Reading {
    fn new(service: &'static str) -> Self {
        Self {
            service: std::sync::Mutex::new(service),
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(2),
        }
    }
    fn set(&self, service: &'static str) {
        *self.service.lock().unwrap_or_else(|e| e.into_inner()) = service;
    }
    fn get(&self) -> &'static str {
        *self.service.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Console {
    pub(super) fn operational(self: &Arc<Self>, route: &str, session: &Session) -> Option<Answer> {
        if !matches!(
            route,
            "fleet" | "store" | "workspace" | "workspace/versions" | "repositories" | "credentials"
        ) && !route.starts_with("store/generations/")
            && !route.starts_with("workspace/versions/")
        {
            return None;
        }
        if route
            .strip_prefix("store/generations/")
            .or_else(|| route.strip_prefix("workspace/versions/"))
            .is_some_and(|id| !meada_core::ids::is_id(id))
        {
            return Some(error(
                StatusCode::NOT_FOUND,
                "not_found",
                &Msg::UiNoRoute.text(),
            ));
        }
        let route = route.to_string();
        let session = session.clone();
        let result = self.bounded("console", move |console, service| {
            read_runtime(
                console.operational_read(&route, &session, &service),
                &service,
            )
            .map(|document| document.map(|value| json_answer(StatusCode::OK, &value)))
        });
        Some(match result {
            Ok(Some(answer)) => answer,
            Ok(None) => error(StatusCode::NOT_FOUND, "not_found", &Msg::UiNoRoute.text()),
            Err(message) => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable", &message),
        })
    }

    /// Permit acquisition and the whole worker share one deadline. Its permit moves into the
    /// worker, never into the request future that a timeout drops.
    fn bounded<T: Send + 'static>(
        self: &Arc<Self>,
        initial: &'static str,
        read: impl FnOnce(Arc<Self>, Arc<Reading>) -> std::result::Result<T, String> + Send + 'static,
    ) -> std::result::Result<T, String> {
        let service = Arc::new(Reading::new(initial));
        let completed = tokio::runtime::Handle::current().block_on(async {
            tokio::time::timeout_at(service.deadline, async {
                let permit = self
                    .readers
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|e| e.to_string())?;
                let console = self.clone();
                let reading = service.clone();
                tokio::task::spawn_blocking(move || {
                    let _held = permit;
                    read(console, reading)
                })
                .await
                .map_err(|e| e.to_string())?
            })
            .await
        });
        completed.unwrap_or_else(|_| Err(timed_out(service.get())))
    }

    #[cfg(feature = "oidc")]
    pub(super) fn served_identities(self: &Arc<Self>) -> std::result::Result<Vec<String>, String> {
        #[cfg(feature = "generation-pointer")]
        if self.bootstrap.store {
            return self.bounded("console", |console, service| {
                let config = console
                    .bootstrap
                    .valkey
                    .as_ref()
                    .expect("store workspace needs valkey");
                service.set("valkey");
                read_runtime(
                    async {
                        let valkey = crate::valkey::Valkey::connect(config).await.map_err(vk)?;
                        console.managed_identities(&valkey, config, &service).await
                    },
                    &service,
                )
            });
        }
        Ok(self
            .bootstrap
            .entries
            .iter()
            .map(|entry| entry.repo.clone())
            .collect())
    }

    fn local_repositories(&self, session: &Session) -> Value {
        let rows: Vec<_> = self.bootstrap.entries.iter().filter(|e| session.reads(&e.repo)).map(|entry| {
            match &entry.repository {
                Err(problem) => json!({"identity": entry.repo, "problem": if session.whole() { problem.as_str() } else { "repository is not available" }}),
                Ok(repo) => crate::index::with_busy_timeout(crate::engine::PROBE_BUSY, || {
                    let provenance = crate::index::provenance(&repo.index).ok();
                    let head = crate::repo::head(&repo.root).ok();
                    let behind = crate::index::open_read(&repo.index, &repo.identity, crate::msg::Fix::Index(&self.file.display().to_string()))
                        .and_then(|db| crate::index::fresh(&db, &repo.root, &repo.identity, &repo.config.serve)).ok().map(|fresh| !fresh || provenance.as_ref().and_then(|(_, commit)| commit.as_ref()).zip(head.as_ref()).is_some_and(|(indexed, head)| indexed != head));
                    json!({"identity": entry.repo, "head": head, "indexed_commit": provenance.as_ref().and_then(|(_, head)| head.clone()),
                        "schema": provenance.map(|(schema, _)| schema), "behind": behind})
                }),
            }
        }).collect();
        json!({"repositories": rows})
    }

    async fn operational_read(
        &self,
        route: &str,
        session: &Session,
        service: &Reading,
    ) -> std::result::Result<Option<Value>, String> {
        service.set("repositories");
        #[cfg(feature = "snapshot")]
        if let Some(snapshot) = &self.bootstrap.snapshot {
            if route == "store" || route.starts_with("store/generations/") {
                return self.store_read(route, session, snapshot, service).await;
            }
            #[cfg(feature = "generation-pointer")]
            if self.bootstrap.generations.is_some()
                && (route == "fleet"
                    || route == "repositories"
                    || route == "workspace"
                    || route.starts_with("workspace/versions"))
            {
                let config = self
                    .bootstrap
                    .valkey
                    .as_ref()
                    .expect("generations needs valkey");
                service.set("valkey");
                let valkey = crate::valkey::Valkey::connect(config).await.map_err(vk)?;
                return match route {
                    "fleet" | "repositories" => self
                        .fleet_read(route, session, snapshot, &valkey, config, service)
                        .await
                        .map(Some),
                    _ => {
                        self.workspace_read(route, session, snapshot, &valkey, config, service)
                            .await
                    }
                };
            }
            if route == "repositories" {
                return Ok(Some(missing("generations")));
            }
        }
        #[cfg(feature = "github-app")]
        if route == "credentials" && !self.apps.is_empty() {
            let config = self
                .bootstrap
                .valkey
                .as_ref()
                .expect("github_app needs valkey");
            service.set("valkey");
            let valkey = crate::valkey::Valkey::connect(config).await.map_err(vk)?;
            #[allow(unused_mut)]
            let mut identities: Vec<String> = self
                .bootstrap
                .entries
                .iter()
                .map(|entry| entry.repo.clone())
                .collect();
            #[cfg(feature = "generation-pointer")]
            if self.bootstrap.store {
                identities = self.managed_identities(&valkey, config, service).await?;
                service.set("valkey");
            }
            let reads_owner = |host: &str, owner: &str| {
                session.whole()
                    || identities
                        .iter()
                        .filter(|identity| session.reads(identity))
                        .any(|identity| {
                            identity.split_once('/').is_some_and(|(h, path)| {
                                h == host
                                    && path
                                        .split('/')
                                        .next()
                                        .is_some_and(|o| o.eq_ignore_ascii_case(owner))
                            })
                        })
            };
            return crate::github_app::inspect(&valkey, config, &self.apps, &reads_owner)
                .await
                .map(Some)
                .map_err(vk);
        }
        Ok(Some(match route {
            "repositories" => self.local_repositories(session),
            "fleet" | "workspace" | "workspace/versions" => missing("generations"),
            "credentials" => missing("github_app"),
            _ if route.starts_with("workspace/") => missing("generations"),
            _ => missing("snapshot"),
        }))
    }
}

fn timed_out(service: &str) -> String {
    format!("{service}: {}", Msg::UiReadTimeout(2000).text())
}

fn missing(section: &str) -> Value {
    json!({"requires": [section]})
}

#[cfg(any(feature = "generation-pointer", feature = "github-app"))]
fn vk(problem: String) -> String {
    format!("valkey: {problem}")
}

#[cfg(feature = "snapshot")]
fn store_problem(problem: String) -> String {
    format!("store: {problem}")
}

/// Entries may contain peer identities too; a grant bounds those exactly like top-level ones.
pub(super) fn filter_entries(entries: &mut Vec<Value>, identity: &str, session: &Session) {
    entries.retain(|entry| entry[identity].as_str().is_some_and(|id| session.reads(id)));
    for entry in entries {
        filter_peers(entry, session);
        if let Some(blocks) = entry.get_mut("blocks") {
            filter_peers(blocks, session);
        }
    }
}

fn filter_peers(entry: &mut Value, session: &Session) {
    if let Some(Value::Array(peers)) = entry.get_mut("peer") {
        peers.retain(|peer| peer["repo"].as_str().is_some_and(|id| session.reads(id)));
    }
}

#[cfg(feature = "snapshot")]
impl Console {
    async fn store_read(
        &self,
        route: &str,
        session: &Session,
        snapshot: &crate::snapshot::Config,
        service: &Reading,
    ) -> std::result::Result<Option<Value>, String> {
        use crate::snapshot::{is_id, pull, store::Store};
        let single = route.strip_prefix("store/generations/");
        // Refuse malformed arguments before opening either external service.
        if single.is_some_and(|id| !is_id(id)) {
            return Ok(None);
        }
        let mut retention = json!({"pointer": null, "retained": []});
        #[cfg(feature = "generation-pointer")]
        if let (Some(generations), Some(config)) =
            (&self.bootstrap.generations, &self.bootstrap.valkey)
        {
            service.set("valkey");
            let valkey = crate::valkey::Valkey::connect(config).await.map_err(vk)?;
            retention = crate::snapshot::generations::inspect(&valkey, config, generations.keep)
                .await
                .map_err(vk)?;
        }
        service.set("store");
        let store = Store::connect(snapshot).map_err(store_problem)?;
        if self.bootstrap_generations_absent()
            && let Some(id) = pull::store_current(&store, snapshot)
                .await
                .map_err(store_problem)?
        {
            retention["pointer"] = json!({"id": id, "fence": 0});
        }
        let prefix = snapshot.key("generations");
        let keys = match single {
            Some(id) => vec![snapshot.key(&format!("generations/{id}.json"))],
            None => store.list(&prefix).await.map_err(store_problem)?,
        };
        let mut generations = Vec::new();
        let mut malformed = Vec::new();
        for key in keys {
            let id = key
                .strip_prefix(&format!("{prefix}/"))
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| is_id(id));
            let Some(id) = id else {
                malformed.push(json!({"key": session.whole().then_some(&key), "reason": "not a generation document name"}));
                continue;
            };
            let retained = retention["retained"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["id"] == id));
            let current = retention["pointer"]["id"] == id;
            let mut row = json!({"id": id, "current": current, "retained": retained.is_some(), "kept": retained.is_some_and(|r| r["kept"] == true)});
            if single.is_some() || current || retained.is_some() {
                let key = snapshot.key(&format!("generations/{id}.json"));
                if store.stored(&key).await.map_err(store_problem)?.is_none() {
                    if single.is_some() {
                        return Ok(None);
                    }
                    row["malformed"] = json!("generation document is missing");
                } else {
                    match pull::stored_document(&store, snapshot, id).await {
                        Ok((document, _)) => {
                            let mut bundles_match = true;
                            for entry in document
                                .repositories
                                .iter()
                                .filter(|entry| session.reads(&entry.identity))
                            {
                                let metadata =
                                    store.stored(&entry.bundle).await.map_err(store_problem)?;
                                bundles_match &= metadata.is_some_and(|stored| {
                                    stored.size == entry.size && stored.blake3 == entry.blake3
                                });
                            }
                            if !bundles_match {
                                row["malformed"] =
                                    json!("bundle metadata differs from the generation document");
                                generations.push(row);
                                continue;
                            }
                            let mut document =
                                serde_json::to_value(document).map_err(|e| e.to_string())?;
                            if let Some(Value::Array(entries)) = document.get_mut("repositories") {
                                filter_entries(entries, "identity", session);
                            }
                            row["document"] = document;
                            row["modified"] =
                                json!(store.modified(&key).await.map_err(store_problem)?);
                        }
                        Err(reason) => {
                            row["malformed"] = json!(if session.whole() {
                                reason
                            } else {
                                "malformed generation document".into()
                            })
                        }
                    }
                }
            }
            generations.push(row);
        }
        if single.is_some() {
            return Ok(generations.into_iter().next());
        }
        Ok(Some(
            json!({"generations": generations, "malformed": malformed, "pointer": retention["pointer"], "retained": retention["retained"]}),
        ))
    }

    fn bootstrap_generations_absent(&self) -> bool {
        #[cfg(feature = "generation-pointer")]
        {
            self.bootstrap.generations.is_none()
        }
        #[cfg(not(feature = "generation-pointer"))]
        true
    }
}

#[cfg(feature = "generation-pointer")]
impl Console {
    /// One read of the current managed workspace, used by admission and owner-level grants.
    #[cfg(any(feature = "oidc", feature = "github-app"))]
    async fn managed_identities(
        &self,
        valkey: &crate::valkey::Valkey,
        config: &crate::valkey::Config,
        service: &Reading,
    ) -> std::result::Result<Vec<String>, String> {
        use crate::snapshot::{store::Store, versions};
        let current = versions::current(valkey, config).await.map_err(vk)?;
        let Some((id, _)) = current else {
            return Ok(Vec::new());
        };
        let snapshot = self
            .bootstrap
            .snapshot
            .as_ref()
            .expect("store workspace needs snapshot");
        service.set("store");
        let store = Store::connect(snapshot).map_err(store_problem)?;
        let key = versions::version_key(snapshot, &id);
        let version = versions::stored(&store, &key, &id)
            .await
            .map_err(|_| store_problem("current workspace version is unreadable".into()))?
            .ok_or_else(|| store_problem("current workspace version is missing".into()))?
            .0;
        Ok(version
            .repositories
            .into_iter()
            .filter_map(|entry| entry["repo"].as_str().map(str::to_string))
            .collect())
    }

    async fn workspace_read(
        &self,
        route: &str,
        session: &Session,
        snapshot: &crate::snapshot::Config,
        valkey: &crate::valkey::Valkey,
        config: &crate::valkey::Config,
        service: &Reading,
    ) -> std::result::Result<Option<Value>, String> {
        use crate::snapshot::{is_id, store::Store, versions};
        let single = route.strip_prefix("workspace/versions/");
        if single.is_some_and(|id| !is_id(id)) {
            return Ok(None);
        }
        let current = versions::current(valkey, config).await.map_err(vk)?;
        if route == "workspace" && current.is_none() {
            return Ok(Some(json!({"current": null, "version": null})));
        }
        service.set("store");
        let store = Store::connect(snapshot).map_err(store_problem)?;
        if route == "workspace/versions" {
            let (versions, malformed) = versions::documents(&store, snapshot)
                .await
                .map_err(store_problem)?;
            let versions: Vec<Value> = versions.into_iter().map(|version| json!({"id": version.id, "author": version.author, "time": version.time, "message": version.message})).collect();
            let malformed = if session.whole() {
                malformed
            } else {
                malformed
                    .iter()
                    .map(|_| "malformed workspace version".to_string())
                    .collect()
            };
            return Ok(Some(
                json!({"current": current.map(|(id, fence)| json!({"id": id, "fence": fence})), "versions": versions, "malformed": malformed}),
            ));
        }
        let id = single
            .or_else(|| current.as_ref().map(|(id, _)| id.as_str()))
            .expect("current checked");
        let key = versions::version_key(snapshot, id);
        let Some((version, _)) = versions::stored(&store, &key, id).await.map_err(|reason| {
            store_problem(if session.whole() {
                reason
            } else {
                "malformed workspace version".into()
            })
        })?
        else {
            return if single.is_some() {
                Ok(None)
            } else {
                Err(store_problem("current workspace version is missing".into()))
            };
        };
        let mut version = serde_json::to_value(version).map_err(|e| e.to_string())?;
        if let Some(Value::Array(entries)) = version.get_mut("repositories") {
            filter_entries(entries, "repo", session);
        }
        if single.is_some() {
            return Ok(Some(version));
        }
        Ok(Some(
            json!({"current": current.map(|(id, fence)| json!({"id": id, "fence": fence})), "version": version}),
        ))
    }

    async fn fleet_read(
        &self,
        route: &str,
        session: &Session,
        snapshot: &crate::snapshot::Config,
        valkey: &crate::valkey::Valkey,
        config: &crate::valkey::Config,
        service: &Reading,
    ) -> std::result::Result<Value, String> {
        use crate::snapshot::{fence, generations, heartbeat, pull, store::Store};
        use redis::cmd;
        let generations = self
            .bootstrap
            .generations
            .as_ref()
            .expect("checked section");
        let retention = generations::inspect(valkey, config, generations.keep)
            .await
            .map_err(vk)?;
        let (heartbeats, malformed) = heartbeat::read(valkey, config).await.map_err(vk)?;
        let malformed: Vec<_> = malformed
            .iter()
            .map(|problem| problem.row(config))
            .collect();
        let counter: Option<String> = valkey
            .query(cmd("GET").arg(config.key("fence", true)))
            .await
            .map_err(vk)?;
        let counter = counter
            .map(|text| fence(&text).ok_or_else(|| vk("malformed fence counter".into())))
            .transpose()?;
        let workspace = crate::snapshot::versions::current(valkey, config)
            .await
            .map_err(vk)?;
        let highest = retention["retained"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["fence"].as_u64())
            .chain(retention["pointer"]["fence"].as_u64())
            .chain(workspace.map(|(_, fence)| fence))
            .chain(heartbeats.iter().filter_map(|beat| beat.fence))
            .max()
            .unwrap_or(0);
        let ttl: i64 = valkey
            .query(cmd("PTTL").arg(config.key("lock", true)))
            .await
            .map_err(vk)?;
        let holder: Vec<Option<String>> = valkey
            .query(
                cmd("HMGET")
                    .arg(config.key("holder", true))
                    .arg(&["host", "pid", "started", "holds"]),
            )
            .await
            .map_err(vk)?;
        let holder = match holder.as_slice() {
            [Some(host), Some(pid), Some(started), Some(holds)]
                if crate::snapshot::is_node_name(host)
                    && meada_core::ids::fence(pid)
                        .is_some_and(|pid| pid > 0 && pid <= u32::MAX as u64)
                    && crate::snapshot::versions::is_time(started)
                    && !holds.chars().any(char::is_control) =>
            {
                json!({"host": host, "pid": pid, "started": started, "holds": holds, "ttl_ms": ttl})
            }
            fields if fields.iter().all(Option::is_none) => Value::Null,
            _ => json!({"malformed": "malformed lock holder", "ttl_ms": ttl}),
        };
        let mut ids: std::collections::BTreeSet<String> = heartbeats
            .iter()
            .filter_map(|beat| beat.generation.clone())
            .collect();
        if let Some(id) = retention["pointer"]["id"].as_str() {
            ids.insert(id.to_string());
        }
        service.set("store");
        let store = Store::connect(snapshot).map_err(store_problem)?;
        let mut documents = std::collections::BTreeMap::new();
        for id in ids {
            let read = pull::stored_document(&store, snapshot, &id)
                .await
                .map_err(|reason| {
                    store_problem(if session.whole() {
                        reason
                    } else {
                        "unreadable generation document".into()
                    })
                })?;
            documents.insert(id, read.0);
        }
        if route == "repositories" {
            let mut rows = Vec::new();
            for (id, document) in &documents {
                let nodes: Vec<_> = heartbeats
                    .iter()
                    .filter(|beat| beat.generation.as_ref() == Some(id))
                    .map(|beat| &beat.name)
                    .collect();
                if nodes.is_empty() {
                    continue;
                }
                for entry in document
                    .repositories
                    .iter()
                    .filter(|entry| session.reads(&entry.identity))
                {
                    let schema = snapshot.local_dir.as_ref().and_then(|local| {
                        let index = local
                            .join(id)
                            .join(crate::snapshot::bundle_of(&entry.bundle))
                            .join("index.db");
                        crate::index::with_busy_timeout(crate::engine::PROBE_BUSY, || {
                            crate::index::provenance(&index)
                                .ok()
                                .map(|(schema, _)| schema)
                        })
                    });
                    rows.push(json!({"identity": entry.identity, "generation": id, "commit": entry.head, "schema": schema, "nodes": nodes}));
                }
            }
            return Ok(json!({"repositories": rows}));
        }
        let nodes: Vec<_> = heartbeats.into_iter().map(|beat| {
            let stuck = beat.generation.as_ref().is_some_and(|id| retention["pointer"]["id"] != *id)
                && beat.fence.zip(retention["pointer"]["fence"].as_u64()).is_some_and(|(held, wanted)| held > wanted);
            let workspace = beat.generation.as_ref().and_then(|id| documents.get(id)).and_then(|doc| doc.workspace.clone());
            json!({"name": beat.name, "heartbeat": beat, "workspace": workspace,
                "stuck": stuck.then(|| json!({"kind": "node", "recoveries": ["meada snapshot fence recover", "delete this node's <local_dir>/current"]}))})
        }).collect();
        let last_publish = match retention["pointer"]["id"].as_str() {
            Some(id) => Some(
                json!({"generation": id, "modified": store.modified(&snapshot.key(&format!("generations/{id}.json"))).await.map_err(store_problem)?}),
            ),
            None => None,
        };
        Ok(
            json!({"nodes": nodes, "malformed": malformed, "pointer": retention["pointer"], "retained": retention["retained"], "holder": holder, "fence": counter, "highest_fence": highest,
            "stuck": (counter.unwrap_or(0) < highest).then(|| json!({"kind": "fleet", "recoveries": ["meada snapshot fence recover"]})), "last_publish": last_publish}),
        )
    }
}
