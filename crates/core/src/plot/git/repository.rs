use super::diff::{self, Diff};
use super::snapshot::{Fingerprints, Snapshot, compressed_bound, hex};
use crate::messages;
use anyhow::{Context, Result, bail, ensure};
use chrono::TimeZone;
use mchprs_blocks::{BlockPos, blocks::Block};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static WRITES: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub snapshot: usize,
    pub plot_bytes: u64,
    pub total_bytes: u64,
}

pub(super) struct Repository {
    conn: Connection,
    pub plot: (i32, i32),
    root: PathBuf,
    limits: Limits,
    _memory: super::Reservation,
}

pub(super) fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 20
        && name != "HEAD"
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !(name.len() >= 8 && name.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Repository {
    pub fn sidebar_head(&self) -> Result<String> {
        let (head, tip) = self.head()?;
        Ok(if tip.is_some() { head } else { String::new() })
    }

    pub fn open(root: &Path, plot: (i32, i32), limits: Limits) -> Result<Self> {
        let memory = super::Reservation::new(4 * 1048576)?;
        let dir = root.join(format!("p{},{}", plot.0, plot.1));
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("repository.sqlite"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;
            PRAGMA journal_mode=DELETE; PRAGMA cache_size=-2048;
            PRAGMA cache_spill=ON; PRAGMA mmap_size=0;
            PRAGMA temp_store=FILE; PRAGMA temp.cache_size=-1024;",
        )?;
        let mut repo = Self {
            conn,
            plot,
            root: root.into(),
            limits,
            _memory: memory,
        };
        repo.transaction(|repo| {
            repo.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS objects(id TEXT PRIMARY KEY,content TEXT NOT NULL,execution TEXT NOT NULL,blob BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS commits(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,parent TEXT REFERENCES commits(id),snapshot TEXT NOT NULL REFERENCES objects(id),author TEXT NOT NULL,name TEXT NOT NULL,date INTEGER NOT NULL,message TEXT NOT NULL,message_fold TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS branches(name TEXT PRIMARY KEY,tip TEXT NOT NULL REFERENCES commits(id));
            CREATE TABLE IF NOT EXISTS recoveries(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,snapshot TEXT NOT NULL REFERENCES objects(id),parent TEXT NOT NULL REFERENCES commits(id),author TEXT NOT NULL,name TEXT NOT NULL,date INTEGER NOT NULL,source TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS checkout(id INTEGER PRIMARY KEY CHECK(id=1),branch TEXT NOT NULL,commit_id TEXT NOT NULL,before_snapshot TEXT NOT NULL REFERENCES objects(id));")?;
            let identity = format!("1:{}:{}", plot.0, plot.1);
            repo.conn.execute(
                "INSERT OR IGNORE INTO meta VALUES('identity',?1)",
                [&identity],
            )?;
            let stored: String = repo.conn.query_row(
                "SELECT value FROM meta WHERE key='identity'", [], |r| r.get(0),
            )?;
            ensure!(identity == stored, messages::GIT_REPOSITORY_IDENTITY_MISMATCH);
            repo.conn.execute("INSERT OR IGNORE INTO meta VALUES('active','master')", [])?;
            repo.conn.execute("UPDATE meta SET value='master' WHERE key='active' AND value='main' AND NOT EXISTS (SELECT 1 FROM commits)", [])?;
            Ok(())
        })?;
        Ok(repo)
    }

    pub fn head(&self) -> Result<(String, Option<String>)> {
        let active: String =
            self.conn
                .query_row("SELECT value FROM meta WHERE key='active'", [], |r| {
                    r.get(0)
                })?;
        let tip = if let Some(id) = active.strip_prefix('@') {
            Some(id.to_owned())
        } else {
            self.conn
                .query_row("SELECT tip FROM branches WHERE name=?1", [&active], |r| {
                    r.get(0)
                })
                .optional()?
        };
        Ok((active, tip))
    }

    pub fn resolve(&self, reference: &str) -> Result<String> {
        if reference == "HEAD" {
            return self.head()?.1.context(messages::GIT_NO_COMMITS_HINT);
        }
        if let Some(tip) = self
            .conn
            .query_row("SELECT tip FROM branches WHERE name=?1", [reference], |r| {
                r.get(0)
            })
            .optional()?
        {
            return Ok(tip);
        }
        ensure!(
            (8..=64).contains(&reference.len()) && reference.bytes().all(|b| b.is_ascii_hexdigit()),
            messages::git_unknown_reference(reference)
        );
        let mut query = self
            .conn
            .prepare("SELECT id FROM commits WHERE id LIKE ?1 LIMIT 2")?;
        let matches = query
            .query_map([format!("{}%", reference.to_ascii_lowercase())], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.as_slice() {
            [id] => Ok(id.clone()),
            [] => bail!(messages::git_unknown_commit(reference)),
            _ => bail!(messages::GIT_AMBIGUOUS_COMMIT),
        }
    }

    fn snapshot_id(&self, commit: &str) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT snapshot FROM commits WHERE id=?1", [commit], |r| {
                r.get(0)
            })?)
    }

    pub fn load(&self, commit: &str) -> Result<Snapshot> {
        self.load_object(&self.snapshot_id(commit)?)
    }

    pub fn raw_size(&self, commit: &str) -> Result<usize> {
        self.object_raw_size(&self.snapshot_id(commit)?)
    }

    fn object_raw_size(&self, object: &str) -> Result<usize> {
        let prefix: Vec<u8> = self.conn.query_row(
            "SELECT substr(blob,1,4) FROM objects WHERE id=?1",
            [object],
            |r| r.get(0),
        )?;
        let size = u32::from_le_bytes(
            prefix
                .as_slice()
                .try_into()
                .context(messages::GIT_INVALID_SIZE_HEADER)?,
        ) as usize;
        ensure!(
            size <= self.limits.snapshot,
            messages::GIT_OBJECT_SNAPSHOT_SIZE_LIMIT
        );
        Ok(size)
    }

    pub fn pending_size(&self) -> Result<usize> {
        let (target, before): (String, String) = self.conn.query_row(
            "SELECT commit_id,before_snapshot FROM checkout WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Startup recovery decodes the before snapshot, drops it, then decodes
        // the target. Reserve the larger peak rather than both at once.
        Ok(self.raw_size(&target)?.max(self.object_raw_size(&before)?))
    }

    fn load_object(&self, id: &str) -> Result<Snapshot> {
        let declared = self.object_raw_size(id)?;
        let size: usize =
            self.conn
                .query_row("SELECT length(blob) FROM objects WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        ensure!(
            size <= compressed_bound(declared),
            messages::GIT_OVERSIZED_OBJECT
        );
        let bytes: Vec<u8> =
            self.conn
                .query_row("SELECT blob FROM objects WHERE id=?1", [id], |r| r.get(0))?;
        let snapshot = Snapshot::decode(&bytes, self.plot, self.limits.snapshot)?;
        ensure!(
            snapshot.fingerprints()?.full == id,
            messages::GIT_CHECKSUM_MISMATCH
        );
        Ok(snapshot)
    }

    fn store(&self, snapshot: &Snapshot, fp: &Fingerprints) -> Result<()> {
        snapshot.validate(self.plot)?;
        if self
            .conn
            .query_row("SELECT 1 FROM objects WHERE id=?1", [&fp.full], |_| Ok(()))
            .optional()?
            .is_some()
        {
            return Ok(());
        }
        let bytes = snapshot.encode(self.limits.snapshot)?;
        let used: u64 = self.conn.query_row(
            "SELECT COALESCE(sum(length(blob)),0) FROM objects",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            used.saturating_add(bytes.len() as u64) <= self.limits.plot_bytes,
            messages::GIT_PLOT_STORAGE_FULL
        );
        let total = self
            .other_database_bytes()?
            .saturating_add(self.database_bytes()?);
        ensure!(
            total.saturating_add(bytes.len() as u64 + 65536) <= self.limits.total_bytes,
            messages::GIT_GLOBAL_STORAGE_FULL
        );
        self.conn.execute(
            "INSERT INTO objects VALUES(?1,?2,?3,?4)",
            params![fp.full, fp.content, fp.execution, bytes],
        )?;
        Ok(())
    }

    fn transaction<T>(&mut self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let _lock = WRITES
            .lock()
            .map_err(|_| anyhow::anyhow!(messages::GIT_STORAGE_LOCK_FAILED))?;
        let previous_bytes = self.database_bytes()?;
        let global_allowance = self
            .limits
            .total_bytes
            .saturating_sub(self.other_database_bytes()?);
        let allowance = self.limits.plot_bytes.min(global_allowance);
        let quota_message = if global_allowance < self.limits.plot_bytes {
            messages::GIT_GLOBAL_STORAGE_FULL
        } else {
            messages::GIT_PLOT_STORAGE_FULL
        };
        let page_size: u64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        // SQLite enforces admission while writing, before dirty pages can spill.
        // Keep existing files readable/recoverable after a quota downgrade.
        self.conn.pragma_update(
            None,
            "max_page_count",
            (allowance.max(previous_bytes) / page_size).max(1),
        )?;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = f(self).and_then(|value| {
            self.check_growth(previous_bytes)?;
            Ok(value)
        });
        match result {
            Ok(value) => {
                if let Err(error) = self.conn.execute_batch("COMMIT") {
                    let _ = self.conn.execute_batch("ROLLBACK");
                    return Err(error.into());
                }
                Ok(value)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                if error
                    .downcast_ref::<rusqlite::Error>()
                    .is_some_and(|e| e.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull))
                {
                    bail!(quota_message);
                }
                Err(error)
            }
        }
    }

    fn database_bytes(&self) -> Result<u64> {
        let pages: u64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: u64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(pages.saturating_mul(size))
    }

    fn check_growth(&self, previous: u64) -> Result<()> {
        let bytes = self.database_bytes()?;
        // Operations that reduce/preserve storage must still allow recovery when
        // an operator lowers a quota below the existing repository's size.
        if bytes <= previous {
            return Ok(());
        }
        ensure!(
            bytes <= self.limits.plot_bytes,
            messages::GIT_PLOT_STORAGE_FULL
        );
        let total = bytes.saturating_add(self.other_database_bytes()?);
        ensure!(
            total <= self.limits.total_bytes,
            messages::GIT_GLOBAL_STORAGE_FULL
        );
        Ok(())
    }

    fn other_database_bytes(&self) -> Result<u64> {
        let own = self.root.join(format!("p{},{}", self.plot.0, self.plot.1));
        let mut total = 0u64;
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path == own {
                continue;
            }
            match std::fs::metadata(path.join("repository.sqlite")) {
                Ok(meta) => total = total.saturating_add(meta.len()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(total)
    }

    pub fn commit(
        &mut self,
        snapshot: &Snapshot,
        author: u128,
        name: &str,
        message: &str,
    ) -> Result<String> {
        ensure!(
            !message.trim().is_empty() && message.chars().count() <= 256,
            messages::GIT_COMMIT_MESSAGE_LIMIT
        );
        let fp = snapshot.fingerprints()?;
        self.transaction(|repo| {
            let (branch, parent) = repo.head()?;
            if let Some(parent) = &parent { ensure!(repo.snapshot_id(parent)? != fp.full, messages::GIT_NOTHING_CHANGED); }
            repo.store(snapshot,&fp)?;
            let now = chrono::Utc::now().timestamp();
            let id = hex(Sha256::digest(serde_json::to_vec(&(1, &parent,&fp.full,author.to_string(),name,now,message))?));
            repo.conn.execute("INSERT INTO commits(id,parent,snapshot,author,name,date,message,message_fold) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![id,parent,fp.full,author.to_string(),name,now,message,message.to_lowercase()])?;
            if branch.starts_with('@') {
                repo.conn.execute("UPDATE meta SET value=?1 WHERE key='active'", [format!("@{id}")])?;
            } else {
                repo.conn.execute("INSERT INTO branches VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET tip=excluded.tip",params![branch,id])?;
            }
            Ok(messages::git_committed(&id[..8], head_label(&branch), message))
        })
    }

    pub fn branch(&mut self, name: &str, reference: &str) -> Result<String> {
        ensure!(valid_branch(name), messages::GIT_BRANCH_NAME_RULES);
        self.transaction(|repo| {
            let tip = repo.resolve(reference)?;
            ensure!(repo.names()?.len() < 128, messages::GIT_BRANCH_LIMIT);
            ensure!(
                !repo.names()?.iter().any(|n| n == name),
                messages::GIT_BRANCH_EXISTS
            );
            repo.conn
                .execute("INSERT INTO branches VALUES(?1,?2)", params![name, tip])?;
            Ok(messages::git_branch_created(name, &tip[..8]))
        })
    }

    pub fn names(&self) -> Result<Vec<String>> {
        Ok(self
            .conn
            .prepare("SELECT name FROM branches ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn branches(&self) -> Result<Value> {
        let active = self.head()?.0;
        let mut extra = Vec::new();
        if let Some(id) = active.strip_prefix('@') {
            extra.push(json!({"text": format!("\n{}", messages::git_detached_head(&id[..8])), "color": "gray"}));
        }
        for (name, id) in self.branch_tips()? {
            let current = name == active;
            extra.push(
                json!({"text": if current { "\n* " } else { "\n  " }, "color": "green", "extra": [
                    {"text": name, "color": if current { "green" } else { "white" }},
                    {"text": format!(" -> {}", &id[..8]), "color": "yellow"}
                ]}),
            );
        }
        if extra.is_empty() {
            extra.push(
                json!({"text": format!("\n{}", messages::GIT_NO_BRANCHES_HINT), "color": "gray"}),
            );
        }
        Ok(json!({"text": messages::GIT_BRANCHES_HEADING, "color": "gray", "extra": extra}))
    }

    fn branch_tips(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .conn
            .prepare("SELECT name,tip FROM branches ORDER BY name")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn status(&self, snapshot: &Snapshot) -> Result<String> {
        let (branch, tip) = self.head()?;
        let usage = self.database_bytes()?;
        let Some(tip) = tip else {
            return Ok(messages::GIT_CREATE_MASTER_HINT.into());
        };
        let saved: (String, String) = self.conn.query_row(
            "SELECT content,execution FROM objects WHERE id=?1",
            [self.snapshot_id(&tip)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let fp = snapshot.fingerprints()?;
        Ok(messages::git_status(
            head_label(&branch),
            &tip[..8],
            fp.content != saved.0,
            fp.execution != saved.1,
            usage as f64 / 1048576.0,
            self.limits.plot_bytes as f64 / 1048576.0,
        ))
    }

    pub fn comparison(
        &self,
        from: &str,
        to: Option<&str>,
        working: Option<&Snapshot>,
    ) -> Result<Diff> {
        let a = self.resolve(from)?;
        let b = to.map(|reference| self.resolve(reference)).transpose()?;
        let size = self.raw_size(&a)?.saturating_add(match &b {
            Some(b) => self.raw_size(b)?,
            None => usize::try_from(bincode::serialized_size(
                working.context(messages::GIT_NO_COMPARISON)?,
            )?)?,
        });
        let reservation = super::Reservation::snapshot(size)?;
        let mut comparison = Diff::new(
            a.clone(),
            b.clone().unwrap_or_default(),
            self.load(&a)?,
            match b {
                Some(b) => self.load(&b)?,
                None => working.unwrap().clone(),
            },
            reservation,
        )?;
        comparison.from_label = from.to_owned();
        comparison.to_label = to.unwrap_or(messages::GIT_WORKING_BUILD).to_owned();
        Ok(comparison)
    }

    pub fn status_chat(&self, snapshot: &Snapshot) -> Result<Value> {
        let (branch, tip) = self.head()?;
        let Some(tip) = tip else {
            return Ok(json!({"text": messages::GIT_CREATE_MASTER_HINT, "color": "gray"}));
        };
        let comparison = self.comparison("HEAD", None, Some(snapshot))?;
        let details = self.status(snapshot)?;
        let secondary = details.split_once('\n').map_or("", |(_, text)| text);
        Ok(
            json!({"text": messages::git_status_heading(head_label(&branch), &tip[..8]), "color": "gray", "extra": [
                {"text": "\n"}, diff::change_summary(comparison.counts),
                {"text": format!("\n{secondary}"), "color": "gray"}
            ]}),
        )
    }

    pub fn recent_commits(&self) -> Result<Vec<String>> {
        Ok(self
            .history_rows(false, None, 0, 50)?
            .into_iter()
            .filter_map(|row| row["click_event"]["value"].as_str().map(str::to_owned))
            .collect())
    }

    fn history_rows(
        &self,
        all: bool,
        query: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let (active, tip) = self.head()?;
        let Some(tip) = tip else {
            return Ok(Vec::new());
        };
        let sql = if all {
            "SELECT id,date,name,message FROM commits WHERE instr(message_fold,?2)>0 ORDER BY seq DESC LIMIT ?4 OFFSET ?3"
        } else {
            "WITH RECURSIVE history(id,parent,date,name,message,message_fold,depth) AS (SELECT id,parent,date,name,message,message_fold,0 FROM commits WHERE id=?1 UNION ALL SELECT c.id,c.parent,c.date,c.name,c.message,c.message_fold,h.depth+1 FROM commits c JOIN history h ON c.id=h.parent ORDER BY 7) SELECT id,date,name,message FROM history WHERE instr(message_fold,?2)>0 LIMIT ?4 OFFSET ?3"
        };
        let rows = self
            .conn
            .prepare(sql)?
            .query_map(
                params![
                    tip,
                    query.unwrap_or("").to_lowercase(),
                    offset as i64,
                    limit as i64
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let branches = self.branch_tips()?;
        Ok(rows.into_iter().map(|(id, timestamp, author, message)| {
            let date = chrono::Local.timestamp_opt(timestamp, 0).single()
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
            let mut extra = Vec::new();
            let tips: Vec<_> = branches.iter().filter(|(_, tip)| tip == &id).collect();
            if !tips.is_empty() || (active.starts_with('@') && active[1..] == id) {
                extra.push(json!({"text": " (", "color": "yellow"}));
                if active.starts_with('@') && active[1..] == id {
                    extra.push(json!({"text": "HEAD", "color": "aqua"}));
                    if !tips.is_empty() { extra.push(json!({"text": ", ", "color": "yellow"})); }
                }
                for (index, (branch, _)) in tips.iter().enumerate() {
                    if index > 0 { extra.push(json!({"text": ", ", "color": "yellow"})); }
                    if branch == &active {
                        extra.push(json!({"text": "HEAD", "color": "aqua"}));
                        extra.push(json!({"text": " -> ", "color": "yellow"}));
                    }
                    extra.push(json!({"text": branch, "color": "green"}));
                }
                extra.push(json!({"text": ")", "color": "yellow"}));
            }
            extra.push(json!({"text": format!("  {message}"), "color": "white"}));
            json!({"text": format!("\n{}", &id[..8]), "color": "yellow", "extra": extra,
                "hover_event": {"action": "show_text", "value": {"text": format!("Author: {author}\nDate: {date}\n{}", &id[..8])}},
                "click_event": {"action": "copy_to_clipboard", "value": &id[..8]}})
        }).collect())
    }

    pub fn log_count(&self, all: bool, count: usize) -> Result<Value> {
        // ponytail: cap one chat response at 1000 rows; use --page for longer trails.
        let rows = self.history_rows(all, None, 0, count.clamp(1, 1000))?;
        Ok(
            json!({"text": if rows.is_empty() { messages::GIT_NO_COMMITS } else { messages::GIT_HISTORY_TITLE }, "color": "gray", "extra": rows}),
        )
    }

    pub fn log(&self, all: bool, query: Option<&str>, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), messages::GIT_INVALID_PAGE);
        let mut rows = self.history_rows(all, query, (page - 1) * 10, 11)?;
        let more = rows.len() > 10;
        rows.truncate(10);
        let command = if let Some(query) = query {
            format!(
                "/git search {}--page {{page}} {query}",
                if all { "--all " } else { "" }
            )
        } else {
            format!(
                "/git log {}--page {{page}}",
                if all { "--all " } else { "" }
            )
        };
        if page > 1 {
            rows.push(button(
                messages::GIT_PREVIOUS_PAGE,
                &command.replace("{page}", &(page - 1).to_string()),
            ));
        }
        if more {
            rows.push(button(
                messages::GIT_NEXT_PAGE,
                &command.replace("{page}", &(page + 1).to_string()),
            ));
        }
        Ok(json!({"text": messages::git_history_heading(page), "color": "gray", "extra": rows}))
    }

    pub fn block_history(&self, pos: BlockPos, current: Block) -> Result<Value> {
        let mut revisions = Vec::new();
        // ponytail: stream full snapshots, index block history if inspection latency matters.
        let mut cache = std::collections::HashMap::new();
        let mut id = self.head()?.1;
        for _ in 0..200 {
            let Some(commit) = id else {
                break;
            };
            let (parent, timestamp, author): (Option<String>, i64, String) = self.conn.query_row(
                "SELECT parent,date,name FROM commits WHERE id=?1",
                [&commit],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let Some(parent) = parent else {
                break;
            };
            let mut states = [0; 2];
            for (index, reference) in [&parent, &commit].into_iter().enumerate() {
                let object = self.snapshot_id(reference)?;
                let state = if let Some(state) = cache.get(&object) {
                    *state
                } else {
                    let _reservation = super::Reservation::snapshot(self.raw_size(reference)?)?;
                    let state = self.load(reference)?.block(pos);
                    cache.insert(object, state);
                    state
                };
                states[index] = state;
            }
            if states[0] != states[1] {
                let date = chrono::Local
                    .timestamp_opt(timestamp, 0)
                    .single()
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                revisions.push(json!({"text": format!("\n{}  {date}  {author}", &commit[..8]), "color": "yellow", "extra": [
                    {"text": "\n  - ", "color": "red"},
                    {"text": diff::description(Block::from_id(states[0])), "color": "#FF9C9C"},
                    {"text": "\n  + ", "color": "green"},
                    {"text": diff::description(Block::from_id(states[1])), "color": "#9CFF9C"}
                ]}));
            }
            id = Some(parent);
        }
        let heading = if revisions.is_empty() {
            messages::GIT_NO_BLOCK_HISTORY.to_owned()
        } else {
            messages::git_block_history_heading(revisions.len())
        };
        Ok(
            json!({"text": messages::git_block_heading(pos.x, pos.y, pos.z), "color": "aqua", "extra": [
                {"text": format!("\n{}", diff::description(current)), "color": "gray"},
                {"text": format!("\n{heading}"), "color": "aqua", "extra": revisions}
            ]}),
        )
    }

    pub fn show(&self, reference: &str) -> Result<String> {
        let id = self.resolve(reference)?;
        let (name, date, message, parent): (String, i64, String, Option<String>) =
            self.conn.query_row(
                "SELECT name,date,message,parent FROM commits WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        Ok(messages::git_commit_details(
            &id[..8],
            name,
            chrono::Utc
                .timestamp_opt(date, 0)
                .single()
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            message,
            parent
                .map(|p| p[..8].to_owned())
                .unwrap_or_else(|| messages::GIT_ROOT_COMMIT.into()),
        ))
    }

    pub fn recoveries(&self, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), messages::GIT_INVALID_PAGE);
        let rows = self
            .conn
            .prepare(
                "SELECT id,date,name,source FROM recoveries ORDER BY seq DESC LIMIT 11 OFFSET ?1",
            )?
            .query_map([((page - 1) * 10) as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut extra: Vec<_> = rows
            .iter()
            .take(10)
            .map(|(id, date, name, source)| {
                let date = chrono::Utc
                    .timestamp_opt(*date, 0)
                    .single()
                    .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                    .unwrap_or_default();
                serde_json::json!({"text":messages::git_recovery_row(&id[..8], date, name, source)})
            })
            .collect();
        if page > 1 {
            extra.push(button(
                messages::GIT_PREVIOUS_PAGE,
                &format!("/git recoveries {}", page - 1),
            ));
        }
        if rows.len() > 10 {
            extra.push(button(
                messages::GIT_NEXT_PAGE,
                &format!("/git recoveries {}", page + 1),
            ));
        }
        Ok(serde_json::json!({"text":messages::git_recoveries_heading(page),"extra":extra}))
    }

    pub fn recover_branch(
        &mut self,
        recovery: &str,
        branch: &str,
        author: u128,
        name: &str,
    ) -> Result<()> {
        ensure!(valid_branch(branch), messages::GIT_INVALID_BRANCH_NAME);
        ensure!(
            (8..=64).contains(&recovery.len()) && recovery.bytes().all(|c| c.is_ascii_hexdigit()),
            messages::GIT_INVALID_RECOVERY_ID
        );
        self.transaction(|repo| {
            ensure!(repo.names()?.len()<128,messages::GIT_BRANCH_LIMIT);
            let rows=repo.conn.prepare("SELECT id,snapshot,parent FROM recoveries WHERE id LIKE ?1 LIMIT 2")?.query_map([format!("{recovery}%")],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ensure!(rows.len()==1,messages::GIT_UNKNOWN_RECOVERY_ID);
            let (recovery,snapshot,parent)=&rows[0];
            let now=chrono::Utc::now().timestamp(); let message=messages::git_recovered_commit(&recovery[..8]);
            let id=hex(Sha256::digest(serde_json::to_vec(&(parent,snapshot,author.to_string(),name,now,&message,branch))?));
            repo.conn.execute("INSERT INTO commits(id,parent,snapshot,author,name,date,message,message_fold) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![id,parent,snapshot,author.to_string(),name,now,message,message.to_lowercase()])?;
            repo.conn.execute("INSERT INTO branches VALUES(?1,?2)",params![branch,id])?; Ok(())
        })
    }

    pub fn has_pending(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM checkout", [], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn checkout(
        &mut self,
        reference: &str,
        before: &Snapshot,
        author: u128,
        name: &str,
        save: &Path,
    ) -> Result<(Snapshot, String, super::Reservation)> {
        let target = self.resolve(reference)?;
        // '@' cannot occur in a branch name. Persist detached HEAD in the same
        // journal field as branches so older repositories need no migration.
        let destination = if reference == "HEAD" {
            self.head()?.0
        } else if self.names()?.iter().any(|name| name == reference) {
            reference.to_owned()
        } else {
            format!("@{target}")
        };
        ensure!(
            self.head()?.0 != destination,
            messages::git_already_on_branch(head_label(&destination))
        );
        let (snapshot, recovery, reservation) =
            self.restore(&target, &destination, before, author, name, save)?;
        Ok((
            snapshot,
            messages::git_checked_out(
                if destination.starts_with('@') {
                    messages::git_detached_head(&target[..8])
                } else {
                    destination
                },
                recovery,
            ),
            reservation,
        ))
    }

    pub fn rebase(
        &mut self,
        source: &str,
        before: &Snapshot,
        author: u128,
        name: &str,
        save: &Path,
    ) -> Result<(Snapshot, String, super::Reservation)> {
        let branch = self.head()?.0;
        ensure!(
            !branch.starts_with('@'),
            messages::GIT_REBASE_NAMED_BRANCH_REQUIRED
        );
        ensure!(
            self.names()?.iter().any(|name| name == source),
            messages::GIT_REBASE_SOURCE_BRANCH_REQUIRED
        );
        let target = self.resolve(source)?;
        // Restore the source snapshot while keeping the current branch and its
        // tip. A later ordinary commit records the copied working state.
        let (snapshot, recovery, reservation) =
            self.restore(&target, &branch, before, author, name, save)?;
        Ok((
            snapshot,
            messages::git_rebased(&branch, source, recovery),
            reservation,
        ))
    }

    pub fn restore_working(
        &mut self,
        reference: &str,
        before: &Snapshot,
        author: u128,
        name: &str,
        save: &Path,
    ) -> Result<(Snapshot, String, super::Reservation)> {
        let target = self.resolve(reference)?;
        let branch = self.head()?.0;
        let (snapshot, recovery, reservation) =
            self.restore(&target, &branch, before, author, name, save)?;
        Ok((
            snapshot,
            messages::git_restored(&target[..8], recovery),
            reservation,
        ))
    }

    fn restore(
        &mut self,
        target: &str,
        destination: &str,
        before: &Snapshot,
        author: u128,
        name: &str,
        save: &Path,
    ) -> Result<(Snapshot, String, super::Reservation)> {
        let reservation = super::Reservation::snapshot(self.raw_size(target)?)?;
        let fp = before.fingerprints()?;
        let mut recovery = None;
        self.transaction(|repo| {
            let (source,head)=repo.head()?;
            let head=head.context(messages::GIT_NO_COMMITS)?;
            repo.load(target)?; // Validate before creating a recovery or a journal.
            repo.store(before,&fp)?;
            if repo.snapshot_id(&head)?!=fp.full {
                let now=chrono::Utc::now().timestamp();
                let id=hex(Sha256::digest(serde_json::to_vec(&(&fp.full,&head,author.to_string(),name,chrono::Utc::now().timestamp_nanos()))?));
                repo.conn.execute("INSERT INTO recoveries(id,snapshot,parent,author,name,date,source) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,fp.full,head,author.to_string(),name,now,source])?;
                recovery=Some(id);
            }
            repo.conn.execute("INSERT INTO checkout VALUES(1,?1,?2,?3)",params![destination,target,fp.full])?; Ok(())
        })?;
        let snapshot = self.finish_or_rollback(before, save)?;
        Ok((
            snapshot,
            recovery
                .map(|id| messages::git_checkout_recovery_suffix(&id[..8]))
                .unwrap_or_default(),
            reservation,
        ))
    }

    fn finish_or_rollback(&mut self, before: &Snapshot, save: &Path) -> Result<Snapshot> {
        match self.finish_checkout_with_settings(
            save,
            Some((before.data.world_send_rate, before.data.piston_animation)),
        ) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                // Restore the exact captured working state, even if target save succeeded.
                before
                    .data
                    .save_to_file(save)
                    .context(messages::GIT_ROLLBACK_NEEDS_RECOVERY)?;
                self.transaction(|repo| {
                    repo.conn.execute("DELETE FROM checkout", [])?;
                    Ok(())
                })?;
                Err(error)
            }
        }
    }

    pub fn finish_checkout(&mut self, save: &Path) -> Result<Snapshot> {
        self.finish_checkout_with_settings(save, None)
    }

    fn finish_checkout_with_settings(
        &mut self,
        save: &Path,
        settings: Option<(
            mchprs_save_data::plot_data::WorldSendRate,
            mchprs_save_data::plot_data::PistonAnimation,
        )>,
    ) -> Result<Snapshot> {
        let (branch, id, before): (String, String, String) = self.conn.query_row(
            "SELECT branch,commit_id,before_snapshot FROM checkout WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let (send_rate, piston_animation) = match settings {
            Some(settings) => settings,
            None => {
                let previous = self.load_object(&before)?;
                (
                    previous.data.world_send_rate,
                    previous.data.piston_animation,
                )
            }
        };
        let mut target = self.load(&id)?;
        target.data.tps = mchprs_save_data::plot_data::Tps::Limited(0);
        target.data.world_send_rate = send_rate;
        target.data.piston_animation = piston_animation;
        target.data.save_to_file(save)?;
        self.transaction(|repo| {
            repo.conn
                .execute("UPDATE meta SET value=?1 WHERE key='active'", [branch])?;
            repo.conn.execute("DELETE FROM checkout", [])?;
            Ok(())
        })?;
        Ok(target)
    }
}

fn head_label(active: &str) -> &str {
    if active.starts_with('@') {
        messages::GIT_DETACHED_LABEL
    } else {
        active
    }
}

pub(super) fn button(label: &str, command: &str) -> Value {
    serde_json::json!({"text":format!(" [{label}]"),"color":"aqua","click_event":{"action":"run_command","command":command}})
}
