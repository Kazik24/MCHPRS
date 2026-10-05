use once_cell::sync::Lazy;
use rusqlite::{params, Connection};
use std::sync::{Mutex, MutexGuard};

static CONN: Lazy<Mutex<Connection>> = Lazy::new(|| {
    Mutex::new(Connection::open("./world/plots.db").expect("Error opening plot database!"))
});

fn lock<'a>() -> MutexGuard<'a, Connection> {
    CONN.lock().unwrap()
}

pub fn get_screen_only(plot_x: i32, plot_z: i32) -> bool {
    lock()
        .query_row(
            "SELECT screen_only FROM plot_visual_settings WHERE plot_x=?1 AND plot_z=?2",
            params![plot_x, plot_z],
            |row| row.get(0),
        )
        .unwrap_or(false)
}

pub fn set_screen_only(plot_x: i32, plot_z: i32, enabled: bool) -> rusqlite::Result<()> {
    lock().execute(
        "INSERT INTO plot_visual_settings(plot_x, plot_z, screen_only) VALUES(?1, ?2, ?3)
         ON CONFLICT(plot_x, plot_z) DO UPDATE SET screen_only=excluded.screen_only",
        params![plot_x, plot_z, enabled],
    )?;
    Ok(())
}

pub fn get_plot_owner(plot_x: i32, plot_z: i32) -> Option<String> {
    lock()
        .query_row(
            "SELECT
                uuid
            FROM
                plot
            JOIN
                userplot ON userplot.plot_id = plot.id
            JOIN
                user ON user.id = userplot.user_id
            WHERE
                plot_x=?1
                AND plot_z=?2
                AND is_owner=TRUE",
            params![plot_x, plot_z],
            |row| row.get::<_, String>(0),
        )
        .ok()
}

pub fn get_cached_username(uuid: String) -> Option<String> {
    lock()
        .query_row(
            "SELECT
                name
            FROM
                user
            WHERE
                uuid=?1",
            params![uuid],
            |row| row.get::<_, String>(0),
        )
        .ok()
}

pub fn get_owned_plots(player: &str) -> rusqlite::Result<Vec<(i32, i32)>> {
    let conn = lock();
    let mut stmt = conn.prepare_cached(
        "SELECT
                    plot_x, plot_z
                FROM
                    plot
                JOIN
                    userplot ON userplot.plot_id = plot.id
                JOIN
                    user ON user.id = userplot.user_id
                WHERE
                    name=?1
                    AND is_owner=TRUE",
    )?;
    let plots = stmt
        .query_map(params![player], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    plots
}

pub fn is_claimed(plot_x: i32, plot_z: i32) -> Option<bool> {
    lock()
        .query_row(
            "SELECT EXISTS(SELECT * FROM plot WHERE plot_x = ?1 AND plot_z = ?2)",
            params![plot_x, plot_z],
            |row| row.get::<_, bool>(0),
        )
        .ok()
}

pub fn claim_plot(plot_x: i32, plot_z: i32, uuid: &str) -> rusqlite::Result<bool> {
    claim_plot_in(&mut lock(), plot_x, plot_z, uuid)
}

fn claim_plot_in(
    conn: &mut Connection,
    plot_x: i32,
    plot_z: i32,
    uuid: &str,
) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    let claimed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM plot WHERE plot_x=?1 AND plot_z=?2)",
        params![plot_x, plot_z],
        |row| row.get(0),
    )?;
    if claimed {
        return Ok(false);
    }
    // Resolve the user before writing either row; a failed claim leaves no orphan.
    let user_id: i64 = tx.query_row("SELECT id FROM user WHERE uuid=?1", [uuid], |row| {
        row.get(0)
    })?;
    tx.execute(
        "INSERT INTO plot(plot_x, plot_z) VALUES(?1, ?2)",
        params![plot_x, plot_z],
    )?;
    tx.execute(
        "INSERT INTO userplot(user_id, plot_id, is_owner)
                VALUES(
                    ?1,
                    LAST_INSERT_ROWID(),
                    TRUE
                )",
        params![user_id],
    )?;
    tx.commit()?;
    Ok(true)
}

pub fn ensure_user(uuid: &str, name: &str) -> rusqlite::Result<()> {
    lock().execute(
        "INSERT INTO user(uuid, name)
                VALUES (?1, ?2)
                ON CONFLICT (uuid) DO UPDATE SET name = ?3",
        params![uuid, name, name],
    )?;
    Ok(())
}

pub fn init() {
    let conn = lock();
    conn.pragma_update(None, "foreign_keys", true)
        .expect("Could not enable plot foreign keys");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("Could not set plot database timeout");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS plot_visual_settings(
            plot_x INTEGER NOT NULL,
            plot_z INTEGER NOT NULL,
            screen_only BOOLEAN NOT NULL DEFAULT FALSE,
            PRIMARY KEY(plot_x, plot_z)
        )",
        [],
    )
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS user(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid BLOB(16) UNIQUE NOT NULL,
            name VARCHAR(16) NOT NULL
        )",
        [],
    )
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS plot(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            plot_x INTEGER NOT NULL,
            plot_z INTEGER NOT NULL
        )",
        [],
    )
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS userplot(
            user_id INTEGER NOT NULL,
            plot_id INTEGER NOT NULL,
            is_owner BOOLEAN NOT NULL DEFAULT FALSE,
            FOREIGN KEY(user_id) REFERENCES user(id),
            FOREIGN KEY(plot_id) REFERENCES plot(id)
        )",
        [],
    )
    .unwrap();
    // Fail visibly on legacy duplicate claims instead of selecting an arbitrary owner.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS plot_coordinates ON plot(plot_x, plot_z)",
        [],
    )
    .expect("Duplicate plot coordinates must be repaired before startup");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claims_are_atomic_and_duplicate_claims_cannot_change_the_owner() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE user(id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, name TEXT);
            CREATE TABLE plot(id INTEGER PRIMARY KEY, plot_x INTEGER, plot_z INTEGER, UNIQUE(plot_x, plot_z));
            CREATE TABLE userplot(user_id INTEGER REFERENCES user(id), plot_id INTEGER REFERENCES plot(id), is_owner BOOLEAN);
            INSERT INTO user VALUES(1, 'first', 'First'), (2, 'second', 'Second');").unwrap();
        assert!(claim_plot_in(&mut conn, 0, 0, "missing").is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(claim_plot_in(&mut conn, 0, 0, "first").unwrap());
        assert!(!claim_plot_in(&mut conn, 0, 0, "second").unwrap());
        let owner: i64 = conn
            .query_row("SELECT user_id FROM userplot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(owner, 1);
        conn.execute_batch("CREATE TRIGGER fail_claim BEFORE INSERT ON userplot BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
        assert!(claim_plot_in(&mut conn, 1, 0, "second").is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
