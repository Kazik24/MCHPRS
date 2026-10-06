use super::snapshot::{hex, Fingerprints, Snapshot};
use anyhow::{bail, ensure, Context, Result};
use chrono::TimeZone;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
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
}

pub(super) fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name != "HEAD"
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !(name.len() >= 8 && name.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Repository {
    pub fn open(root: &Path, plot: (i32, i32), limits: Limits) -> Result<Self> {
        let dir = root.join(format!("p{},{}", plot.0, plot.1));
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("repository.sqlite"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS objects(id TEXT PRIMARY KEY,content TEXT NOT NULL,execution TEXT NOT NULL,blob BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS commits(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,parent TEXT REFERENCES commits(id),snapshot TEXT NOT NULL REFERENCES objects(id),author TEXT NOT NULL,name TEXT NOT NULL,date INTEGER NOT NULL,message TEXT NOT NULL,message_fold TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS branches(name TEXT PRIMARY KEY,tip TEXT NOT NULL REFERENCES commits(id));
            CREATE TABLE IF NOT EXISTS recoveries(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,snapshot TEXT NOT NULL REFERENCES objects(id),parent TEXT NOT NULL REFERENCES commits(id),author TEXT NOT NULL,name TEXT NOT NULL,date INTEGER NOT NULL,source TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS checkout(id INTEGER PRIMARY KEY CHECK(id=1),branch TEXT NOT NULL,commit_id TEXT NOT NULL,before_snapshot TEXT NOT NULL REFERENCES objects(id));")?;
        let identity = format!("1:{}:{}", plot.0, plot.1);
        conn.execute(
            "INSERT OR IGNORE INTO meta VALUES('identity',?1)",
            [&identity],
        )?;
        let stored: String =
            conn.query_row("SELECT value FROM meta WHERE key='identity'", [], |r| {
                r.get(0)
            })?;
        ensure!(
            identity == stored,
            "Repository version or plot identity mismatch"
        );
        conn.execute("INSERT OR IGNORE INTO meta VALUES('active','main')", [])?;
        Ok(Self {
            conn,
            plot,
            root: root.into(),
            limits,
        })
    }

    pub fn head(&self) -> Result<(String, Option<String>)> {
        let active: String =
            self.conn
                .query_row("SELECT value FROM meta WHERE key='active'", [], |r| {
                    r.get(0)
                })?;
        let tip = self
            .conn
            .query_row("SELECT tip FROM branches WHERE name=?1", [&active], |r| {
                r.get(0)
            })
            .optional()?;
        Ok((active, tip))
    }

    pub fn resolve(&self, reference: &str) -> Result<String> {
        if reference == "HEAD" {
            return self
                .head()?
                .1
                .context("No commits yet. Use /git commit <message>.");
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
            "Unknown branch or commit: {reference}"
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
            [] => bail!("Unknown commit: {reference}"),
            _ => bail!("Ambiguous commit prefix; use more characters"),
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
                .context("Invalid Git size header")?,
        ) as usize;
        ensure!(
            size <= self.limits.snapshot,
            "Git snapshot exceeds size limit"
        );
        Ok(size)
    }

    pub fn pending_size(&self) -> Result<usize> {
        let (target, before): (String, String) = self.conn.query_row(
            "SELECT commit_id,before_snapshot FROM checkout WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(self
            .raw_size(&target)?
            .saturating_add(self.object_raw_size(&before)?))
    }

    fn load_object(&self, id: &str) -> Result<Snapshot> {
        let size: usize =
            self.conn
                .query_row("SELECT length(blob) FROM objects WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        ensure!(
            size <= self.limits.snapshot + self.limits.snapshot / 100 + 1024,
            "Oversized Git object"
        );
        let bytes: Vec<u8> =
            self.conn
                .query_row("SELECT blob FROM objects WHERE id=?1", [id], |r| r.get(0))?;
        let snapshot = Snapshot::decode(&bytes, self.plot, self.limits.snapshot)?;
        ensure!(
            snapshot.fingerprints()?.full == id,
            "Git object checksum mismatch"
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
            "Plot Git storage quota is full; existing history was preserved"
        );
        let total: u64 = std::fs::read_dir(&self.root)?
            .filter_map(|e| e.ok())
            .filter_map(|e| std::fs::metadata(e.path().join("repository.sqlite")).ok())
            .map(|m| m.len())
            .sum();
        ensure!(
            total.saturating_add(bytes.len() as u64 + 65536) <= self.limits.total_bytes,
            "Global Git storage quota is full"
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
            .map_err(|_| anyhow::anyhow!("Git storage lock failed"))?;
        let previous_bytes = self.database_bytes()?;
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
            "Plot Git storage quota is full; existing history was preserved"
        );
        let own = self.root.join(format!("p{},{}", self.plot.0, self.plot.1));
        let mut total = bytes;
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
        ensure!(
            total <= self.limits.total_bytes,
            "Global Git storage quota is full"
        );
        Ok(())
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
            "Commit message must contain 1..256 characters"
        );
        let fp = snapshot.fingerprints()?;
        self.transaction(|repo| {
            let (branch, parent) = repo.head()?;
            if let Some(parent) = &parent { ensure!(repo.snapshot_id(parent)? != fp.full, "Nothing changed since the last commit"); }
            repo.store(snapshot,&fp)?;
            let now = chrono::Utc::now().timestamp();
            let id = hex(Sha256::digest(serde_json::to_vec(&(1, &parent,&fp.full,author.to_string(),name,now,message))?));
            repo.conn.execute("INSERT INTO commits(id,parent,snapshot,author,name,date,message,message_fold) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![id,parent,fp.full,author.to_string(),name,now,message,message.to_lowercase()])?;
            repo.conn.execute("INSERT INTO branches VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET tip=excluded.tip",params![branch,id])?;
            Ok(format!("Committed {} on {branch}: {message}",&id[..8]))
        })
    }

    pub fn branch(&mut self, name: &str, reference: &str) -> Result<String> {
        ensure!(
            valid_branch(name),
            "Branch name: 1..48 letters, digits, _ or -; no HEAD or commit-like names"
        );
        self.transaction(|repo| {
            let tip = repo.resolve(reference)?;
            ensure!(repo.names()?.len() < 128, "Git branch limit reached (128)");
            ensure!(
                !repo.names()?.iter().any(|n| n == name),
                "Branch already exists"
            );
            repo.conn
                .execute("INSERT INTO branches VALUES(?1,?2)", params![name, tip])?;
            Ok(format!(
                "Created {name} at {}. Use /git checkout {name} to switch.",
                &tip[..8]
            ))
        })
    }

    pub fn names(&self) -> Result<Vec<String>> {
        Ok(self
            .conn
            .prepare("SELECT name FROM branches ORDER BY name")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn branches(&self) -> Result<String> {
        let active = self.head()?.0;
        let rows = self
            .conn
            .prepare("SELECT name,tip FROM branches ORDER BY name")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(if rows.is_empty() {
            "No branches yet. Use /git commit <message>.".into()
        } else {
            rows.into_iter()
                .map(|(name, id)| {
                    format!(
                        "{} {name} [{}]",
                        if name == active { "*" } else { " " },
                        &id[..8]
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub fn status(&self, snapshot: &Snapshot) -> Result<String> {
        let (branch, tip) = self.head()?;
        let usage = self.database_bytes()?;
        let Some(tip) = tip else {
            return Ok("No commits yet. Use /git commit <message> to create main.".into());
        };
        let saved: (String, String) = self.conn.query_row(
            "SELECT content,execution FROM objects WHERE id=?1",
            [self.snapshot_id(&tip)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let fp = snapshot.fingerprints()?;
        Ok(format!("{branch} [{}]\nBuild changes: {}; execution changes: {}\nGit storage: {:.1} / {:.1} MiB",&tip[..8],fp.content != saved.0,fp.execution != saved.1,usage as f64 / 1048576.0,self.limits.plot_bytes as f64 /1048576.0))
    }

    pub fn log(&self, all: bool, query: Option<&str>, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), "Page must be positive");
        let tip = self.head()?.1.context("No commits yet")?;
        let sql = if all {
            "SELECT id,date,name,message FROM commits WHERE instr(message_fold,?2)>0 ORDER BY seq DESC LIMIT 11 OFFSET ?3"
        } else {
            "WITH RECURSIVE history(id,parent,date,name,message,message_fold,depth) AS (SELECT id,parent,date,name,message,message_fold,0 FROM commits WHERE id=?1 UNION ALL SELECT c.id,c.parent,c.date,c.name,c.message,c.message_fold,h.depth+1 FROM commits c JOIN history h ON c.id=h.parent) SELECT id,date,name,message FROM history WHERE instr(message_fold,?2)>0 ORDER BY depth LIMIT 11 OFFSET ?3"
        };
        // Recursive order follows commit ancestry, independently of clock changes.
        let mut statement = self.conn.prepare(sql)?;
        let rows = statement
            .query_map(
                params![
                    tip,
                    query.unwrap_or("").to_lowercase(),
                    ((page - 1) * 10) as i64
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
        let mut extra = Vec::new();
        for (id, date, name, message) in rows.iter().take(10) {
            let date = chrono::Utc
                .timestamp_opt(*date, 0)
                .single()
                .map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_default();
            extra.push(serde_json::json!({"text":format!("\n[{}] {date} {name}: {message}",&id[..8]),"color":"aqua","click_event":{"action":"run_command","command":format!("/git show {id}")}}));
        }
        let command = if let Some(query) = query {
            format!(
                "/git search {}--page {{page}} {query}",
                if all { "--all " } else { "" }
            )
        } else {
            format!("/git log {}{{page}}", if all { "--all " } else { "" })
        };
        if page > 1 {
            extra.push(button(
                "Previous",
                &command.replace("{page}", &(page - 1).to_string()),
            ));
        }
        if rows.len() > 10 {
            extra.push(button(
                "Next",
                &command.replace("{page}", &(page + 1).to_string()),
            ));
        }
        Ok(serde_json::json!({"text":format!("Git history — page {page}"),"extra":extra}))
    }

    pub fn show(&self, reference: &str) -> Result<String> {
        let id = self.resolve(reference)?;
        let (name, date, message, parent): (String, i64, String, Option<String>) =
            self.conn.query_row(
                "SELECT name,date,message,parent FROM commits WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        Ok(format!(
            "{}\n{name}, {}\n{message}\nParent: {}",
            &id[..8],
            chrono::Utc
                .timestamp_opt(date, 0)
                .single()
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            parent
                .map(|p| p[..8].to_owned())
                .unwrap_or_else(|| "root".into())
        ))
    }

    pub fn recoveries(&self, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), "Page must be positive");
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
                serde_json::json!({"text":format!("\n{} {date} {name} from {source}",&id[..8])})
            })
            .collect();
        if page > 1 {
            extra.push(button("Previous", &format!("/git recoveries {}", page - 1)));
        }
        if rows.len() > 10 {
            extra.push(button("Next", &format!("/git recoveries {}", page + 1)));
        }
        Ok(
            serde_json::json!({"text":format!("Recoveries — page {page}. Use /git recover <id> <new-branch>."),"extra":extra}),
        )
    }

    pub fn recover_branch(
        &mut self,
        recovery: &str,
        branch: &str,
        author: u128,
        name: &str,
    ) -> Result<()> {
        ensure!(valid_branch(branch), "Invalid branch name");
        ensure!(
            (8..=64).contains(&recovery.len()) && recovery.bytes().all(|c| c.is_ascii_hexdigit()),
            "Invalid recovery ID"
        );
        self.transaction(|repo| {
            ensure!(repo.names()?.len()<128,"Git branch limit reached (128)");
            let rows=repo.conn.prepare("SELECT id,snapshot,parent FROM recoveries WHERE id LIKE ?1 LIMIT 2")?.query_map([format!("{recovery}%")],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ensure!(rows.len()==1,"Unknown or ambiguous recovery ID");
            let (recovery,snapshot,parent)=&rows[0];
            let now=chrono::Utc::now().timestamp(); let message=format!("Recovered {}",&recovery[..8]);
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
        branch: &str,
        before: &Snapshot,
        author: u128,
        name: &str,
        save: &Path,
    ) -> Result<(Snapshot, String, super::Reservation)> {
        let target = self.resolve(branch)?;
        let reservation = super::Reservation::new(
            self.raw_size(&target)?
                .saturating_mul(6)
                .saturating_add(4 * 1048576),
        )?;
        let fp = before.fingerprints()?;
        let mut recovery = None;
        self.transaction(|repo| {
            let (source,head)=repo.head()?;
            ensure!(source!=branch,"Already on {branch}; unfinished work was kept");
            let head=head.context("No commits yet")?;
            let target:String=repo.conn.query_row("SELECT tip FROM branches WHERE name=?1",[branch],|r|r.get(0)).optional()?.context("Unknown branch")?;
            repo.load(&target)?; // Validate before creating a recovery or a journal.
            repo.store(before,&fp)?;
            if repo.snapshot_id(&head)?!=fp.full {
                let now=chrono::Utc::now().timestamp();
                let id=hex(Sha256::digest(serde_json::to_vec(&(&fp.full,&head,author.to_string(),name,chrono::Utc::now().timestamp_nanos()))?));
                repo.conn.execute("INSERT INTO recoveries(id,snapshot,parent,author,name,date,source) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,fp.full,head,author.to_string(),name,now,source])?;
                recovery=Some(id);
            }
            repo.conn.execute("INSERT INTO checkout VALUES(1,?1,?2,?3)",params![branch,target,fp.full])?; Ok(())
        })?;
        match self.finish_checkout(save) {
            Ok(snapshot) => Ok((
                snapshot,
                format!(
                    "Checked out {branch}; simulation paused.{}",
                    recovery
                        .map(|id| format!(" Recovery: {} (see /git recoveries)", &id[..8]))
                        .unwrap_or_default()
                ),
                reservation,
            )),
            Err(error) => {
                // Restore the exact captured working state, even if target save succeeded.
                before
                    .data
                    .save_to_file(save)
                    .context("Checkout failed and rollback needs startup recovery")?;
                self.transaction(|repo| {
                    repo.conn.execute("DELETE FROM checkout", [])?;
                    Ok(())
                })?;
                Err(error)
            }
        }
    }

    pub fn finish_checkout(&mut self, save: &Path) -> Result<Snapshot> {
        let (branch, id, before): (String, String, String) = self.conn.query_row(
            "SELECT branch,commit_id,before_snapshot FROM checkout WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut target = self.load(&id)?;
        let previous = self.load_object(&before)?;
        target.data.tps = mchprs_save_data::plot_data::Tps::Limited(0);
        target.data.world_send_rate = previous.data.world_send_rate;
        target.data.piston_animation = previous.data.piston_animation;
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

pub(super) fn button(label: &str, command: &str) -> Value {
    serde_json::json!({"text":format!(" [{label}]"),"color":"aqua","click_event":{"action":"run_command","command":command}})
}
