//! The query engine: opens a repository from a directory and answers over its index.

use std::path::Path;

use rusqlite::Connection;

use crate::{Result, index, repo};

pub struct Engine {
    db: Connection,
    identity: String,
}

impl Engine {
    /// Opens the repository that contains `dir` and brings its index up to date.
    pub fn open(dir: &Path) -> Result<Self> {
        let root = repo::root(dir)?;
        let identity = repo::identity(&root);
        let mut db = index::open(&root)?;
        // ponytail: full rebuild on every open; incremental refresh by mtime and size comes
        // with the freshness ticket.
        index::rebuild(&mut db, &root, &identity)?;
        Ok(Self { db, identity })
    }

    /// The canonical export: the whole index as sorted, pretty-printed JSON.
    pub fn export(&self) -> Result<String> {
        let export = index::export(&self.db, &self.identity)?;
        Ok(serde_json::to_string_pretty(&export)? + "\n")
    }
}
