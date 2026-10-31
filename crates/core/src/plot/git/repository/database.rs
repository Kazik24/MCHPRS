use super::*;

pub(super) struct CommitRecord<'a> {
    pub id: &'a str,
    pub parent: Option<&'a str>,
    pub snapshot: &'a str,
    pub author: u128,
    pub name: &'a str,
    pub date: i64,
    pub message: &'a str,
}

pub(super) struct RecoveryRecord<'a> {
    pub id: &'a str,
    pub parent: &'a str,
    pub snapshot: &'a str,
    pub author: u128,
    pub name: &'a str,
    pub date: i64,
    pub source: &'a str,
}

impl Repository {
    pub(super) fn object_fingerprints(&self, id: &str) -> Result<(String, String)> {
        Ok(self.conn.query_row("SELECT content,execution FROM objects WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?)
    }

    pub(super) fn recovery_rows(&self, page: usize) -> Result<Vec<(String, i64, String, String)>> {
        Ok(self.conn.prepare("SELECT id,date,name,source FROM recoveries ORDER BY seq DESC LIMIT 11 OFFSET ?1")?
            .query_map([((page - 1) * 10) as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn recovery_matches(&self, reference: &str) -> Result<Vec<(String, String, String)>> {
        Ok(self.conn.prepare("SELECT id,snapshot,parent FROM recoveries WHERE id LIKE ?1 LIMIT 2")?
            .query_map([format!("{reference}%")], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn insert_recovery(&self, record: RecoveryRecord<'_>) -> Result<()> {
        self.conn.execute("INSERT INTO recoveries(id,snapshot,parent,author,name,date,source) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![record.id, record.snapshot, record.parent, record.author.to_string(), record.name, record.date, record.source])?;
        Ok(())
    }

    pub(super) fn insert_checkout(&self, destination: &str, target: &str, before: &str) -> Result<()> {
        self.conn.execute("INSERT INTO checkout VALUES(1,?1,?2,?3)", params![destination, target, before])?;
        Ok(())
    }

    pub(super) fn clear_checkout(&self) -> Result<()> {
        self.conn.execute("DELETE FROM checkout", [])?;
        Ok(())
    }

    pub(super) fn insert_commit(&self, commit: CommitRecord<'_>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO commits(id,parent,snapshot,author,name,date,message,message_fold) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![commit.id, commit.parent, commit.snapshot, commit.author.to_string(), commit.name, commit.date, commit.message, commit.message.to_lowercase()],
        )?;
        Ok(())
    }

    pub(super) fn set_active(&self, reference: &str) -> Result<()> {
        self.conn.execute("UPDATE meta SET value=?1 WHERE key='active'", [reference])?;
        Ok(())
    }

    pub(super) fn set_branch_tip(&self, branch: &str, tip: &str) -> Result<()> {
        self.conn.execute("INSERT INTO branches VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET tip=excluded.tip", params![branch, tip])?;
        Ok(())
    }

    pub(super) fn insert_branch(&self, branch: &str, tip: &str) -> Result<()> {
        self.conn.execute("INSERT INTO branches VALUES(?1,?2)", params![branch, tip])?;
        Ok(())
    }

    pub(super) fn checkout_record(&self) -> Result<(String, String, String)> {
        Ok(self.conn.query_row("SELECT branch,commit_id,before_snapshot FROM checkout WHERE id=1", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?)
    }

    pub(super) fn commit_history_details(&self, id: &str) -> Result<(Option<String>, i64, String)> {
        Ok(self.conn.query_row("SELECT parent,date,name FROM commits WHERE id=?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?)
    }

    pub(super) fn commit_details(&self, id: &str) -> Result<(String, i64, String, Option<String>)> {
        Ok(self.conn.query_row("SELECT name,date,message,parent FROM commits WHERE id=?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?)
    }

    pub(super) fn active_reference(&self) -> Result<String> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key='active'", [], |row| row.get(0))?)
    }

    pub(super) fn branch_tip(&self, name: &str) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT tip FROM branches WHERE name=?1", [name], |row| row.get(0)).optional()?)
    }

    pub(super) fn commit_matches(&self, reference: &str) -> Result<Vec<String>> {
        let mut query = self.conn.prepare("SELECT id FROM commits WHERE id LIKE ?1 LIMIT 2")?;
        Ok(query.query_map([format!("{}%", reference.to_ascii_lowercase())], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn branch_names(&self) -> Result<Vec<String>> {
        Ok(self.conn.prepare("SELECT name FROM branches ORDER BY name")?.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn checkout_exists(&self) -> Result<bool> {
        Ok(self.conn.query_row("SELECT 1 FROM checkout", [], |_| Ok(())).optional()?.is_some())
    }

    pub fn open(root: &Path, plot: (i32, i32), limits: Limits) -> Result<Self> {
        let memory = super::super::Reservation::new(4 * 1048576)?;
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

    pub(super) fn transaction<T>(&mut self, f: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let _lock = WRITES.lock().map_err(|_| anyhow::anyhow!(messages::GIT_STORAGE_LOCK_FAILED))?;
        let previous_bytes = self.database_bytes()?;
        let global_allowance = self.limits.total_bytes.saturating_sub(self.other_database_bytes()?);
        let allowance = self.limits.plot_bytes.min(global_allowance);
        let quota_message = if global_allowance < self.limits.plot_bytes {
            messages::GIT_GLOBAL_STORAGE_FULL
        } else {
            messages::GIT_PLOT_STORAGE_FULL
        };
        let page_size: u64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        // SQLite enforces admission while writing, before dirty pages can spill.
        // Keep existing files readable/recoverable after a quota downgrade.
        self.conn
            .pragma_update(None, "max_page_count", (allowance.max(previous_bytes) / page_size).max(1))?;
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

    pub(super) fn database_bytes(&self) -> Result<u64> {
        let pages: u64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: u64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        Ok(pages.saturating_mul(size))
    }

    pub(super) fn check_growth(&self, previous: u64) -> Result<()> {
        let bytes = self.database_bytes()?;
        if bytes <= previous {
            return Ok(());
        }
        ensure!(bytes <= self.limits.plot_bytes, messages::GIT_PLOT_STORAGE_FULL);
        // The global quota includes every other plot repository as well as this one.
        let total = bytes.saturating_add(self.other_database_bytes()?);
        ensure!(total <= self.limits.total_bytes, messages::GIT_GLOBAL_STORAGE_FULL);
        Ok(())
    }

    pub(super) fn other_database_bytes(&self) -> Result<u64> {
        let own = self.root.join(format!("p{},{}", self.plot.0, self.plot.1));
        let mut total = 0u64;
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path == own {
                continue;
            }
            match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            }
            match std::fs::metadata(path.join("repository.sqlite")) {
                Ok(meta) => total = total.saturating_add(meta.len()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(total)
    }

    pub(super) fn snapshot_id(&self, commit: &str) -> Result<String> {
        Ok(self.conn.query_row("SELECT snapshot FROM commits WHERE id=?1", [commit], |r| r.get(0))?)
    }


    pub(super) fn object_raw_size(&self, object: &str) -> Result<usize> {
        let prefix: Vec<u8> = self
            .conn
            .query_row("SELECT substr(blob,1,4) FROM objects WHERE id=?1", [object], |r| r.get(0))?;
        let size = u32::from_le_bytes(prefix.as_slice().try_into().context(messages::GIT_INVALID_SIZE_HEADER)?) as usize;
        ensure!(size <= self.limits.snapshot, messages::GIT_OBJECT_SNAPSHOT_SIZE_LIMIT);
        Ok(size)
    }


    pub(super) fn load_object(&self, id: &str) -> Result<Snapshot> {
        let declared = self.object_raw_size(id)?;
        let size: usize = self.conn.query_row("SELECT length(blob) FROM objects WHERE id=?1", [id], |r| r.get(0))?;
        ensure!(size <= compressed_bound(declared), messages::GIT_OVERSIZED_OBJECT);
        let bytes: Vec<u8> = self.conn.query_row("SELECT blob FROM objects WHERE id=?1", [id], |r| r.get(0))?;
        let snapshot = Snapshot::decode(&bytes, self.plot, self.limits.snapshot)?;
        ensure!(snapshot.fingerprints()?.full == id, messages::GIT_CHECKSUM_MISMATCH);
        Ok(snapshot)
    }

    pub(super) fn store(&self, snapshot: &Snapshot, fp: &Fingerprints) -> Result<()> {
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
        let used: u64 = self
            .conn
            .query_row("SELECT COALESCE(sum(length(blob)),0) FROM objects", [], |r| r.get(0))?;
        ensure!(
            used.saturating_add(bytes.len() as u64) <= self.limits.plot_bytes,
            messages::GIT_PLOT_STORAGE_FULL
        );
        let total = self.other_database_bytes()?.saturating_add(self.database_bytes()?);
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


    pub(super) fn branch_tips(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .conn
            .prepare("SELECT name,tip FROM branches ORDER BY name")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }


    pub(super) fn history_rows(&self, all: bool, query: Option<&str>, offset: usize, limit: usize) -> Result<Vec<Value>> {
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
            .query_map(params![tip, query.unwrap_or("").to_lowercase(), offset as i64, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let branches = self.branch_tips()?;
        Ok(rows
            .into_iter()
            .map(|(id, timestamp, author, message)| {
                let date = chrono::Local
                    .timestamp_opt(timestamp, 0)
                    .single()
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default();
                let mut extra = Vec::new();
                let tips: Vec<_> = branches.iter().filter(|(_, tip)| tip == &id).collect();
                if !tips.is_empty() || (active.starts_with('@') && active[1..] == id) {
                    extra.push(json!({"text": " (", "color": "yellow"}));
                    if active.starts_with('@') && active[1..] == id {
                        extra.push(json!({"text": "HEAD", "color": "aqua"}));
                        if !tips.is_empty() {
                            extra.push(json!({"text": ", ", "color": "yellow"}));
                        }
                    }
                    for (index, (branch, _)) in tips.iter().enumerate() {
                        if index > 0 {
                            extra.push(json!({"text": ", ", "color": "yellow"}));
                        }
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
            })
            .collect())
    }

}
