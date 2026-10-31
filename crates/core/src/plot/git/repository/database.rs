use super::*;

impl Repository {
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

}
