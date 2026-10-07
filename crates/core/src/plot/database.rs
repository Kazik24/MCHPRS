use crate::player::PlayerPos;
use once_cell::sync::Lazy;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, RwLock};

#[derive(Clone, Copy, Debug)]
pub struct Warp {
    pub pos: PlayerPos,
    pub yaw: f32,
    pub pitch: f32,
}

impl Warp {
    pub fn is_valid(&self) -> bool {
        self.pos.is_valid() && self.yaw.is_finite() && self.pitch.is_finite()
    }
}

fn init_warps(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS warp(
            name TEXT PRIMARY KEY COLLATE NOCASE NOT NULL,
            x REAL NOT NULL, y REAL NOT NULL, z REAL NOT NULL,
            yaw REAL NOT NULL, pitch REAL NOT NULL
        )",
        [],
    )?;
    Ok(())
}

pub fn set_warp(name: &str, warp: Warp) -> rusqlite::Result<()> {
    set_warp_in(&lock(), name, warp)
}

fn set_warp_in(conn: &Connection, name: &str, warp: Warp) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO warp(name, x, y, z, yaw, pitch) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(name) DO UPDATE SET x=excluded.x, y=excluded.y, z=excluded.z,
             yaw=excluded.yaw, pitch=excluded.pitch",
        params![name, warp.pos.x, warp.pos.y, warp.pos.z, warp.yaw, warp.pitch],
    )?;
    Ok(())
}

pub fn get_warp(name: &str) -> rusqlite::Result<Option<Warp>> {
    get_warp_in(&lock(), name)
}

fn get_warp_in(conn: &Connection, name: &str) -> rusqlite::Result<Option<Warp>> {
    conn.query_row(
        "SELECT x, y, z, yaw, pitch FROM warp WHERE name=?1",
        [name],
        |row| {
            Ok(Warp {
                pos: PlayerPos::new(row.get(0)?, row.get(1)?, row.get(2)?),
                yaw: row.get(3)?,
                pitch: row.get(4)?,
            })
        },
    )
    .optional()
}

pub fn warp_names() -> rusqlite::Result<Vec<String>> {
    warp_names_in(&lock())
}

fn warp_names_in(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT name FROM warp ORDER BY name COLLATE NOCASE")?;
    let names = stmt.query_map([], |row| row.get(0))?.collect();
    names
}

// Interaction checks read memory rather than querying SQLite for every block action.
static MEMBERS: Lazy<RwLock<HashSet<(i32, i32, u128)>>> = Lazy::new(Default::default);

pub fn is_plot_member(x: i32, z: i32, uuid: u128) -> bool {
    MEMBERS.read().unwrap().contains(&(x, z, uuid))
}

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
    get_cached_username_in(&lock(), &uuid)
}

fn get_cached_username_in(conn: &Connection, uuid: &str) -> Option<String> {
    conn.query_row(
        "SELECT
                name
            FROM
                user
            WHERE
                uuid=?1 AND name_current=TRUE",
        params![uuid],
        |row| row.get::<_, String>(0),
    )
    .ok()
}

pub fn get_owned_plots(player: &str) -> rusqlite::Result<Vec<(i32, i32)>> {
    get_owned_plots_in(&lock(), player)
}

fn get_owned_plots_in(conn: &Connection, player: &str) -> rusqlite::Result<Vec<(i32, i32)>> {
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
                    name=?1 COLLATE NOCASE AND name_current=TRUE
                    AND is_owner=TRUE
                ORDER BY plot.id",
    )?;
    let plots = stmt
        .query_map(params![player], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    plots
}

pub fn get_owned_plots_by_uuid(uuid: u128) -> rusqlite::Result<Vec<(i32, i32)>> {
    get_owned_plots_by_uuid_in(&lock(), uuid)
}

fn get_owned_plots_by_uuid_in(conn: &Connection, uuid: u128) -> rusqlite::Result<Vec<(i32, i32)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT plot_x, plot_z FROM plot
         JOIN userplot ON userplot.plot_id=plot.id
         JOIN user ON user.id=userplot.user_id
         WHERE uuid=?1 AND is_owner=TRUE ORDER BY plot.id",
    )?;
    let result = stmt
        .query_map([format!("{uuid:032x}")], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect();
    result
}

pub fn known_usernames() -> rusqlite::Result<Vec<String>> {
    known_usernames_in(&lock())
}

fn known_usernames_in(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT name FROM user WHERE name_current=TRUE ORDER BY name COLLATE NOCASE",
    )?;
    let result = stmt.query_map([], |row| row.get(0))?.collect();
    result
}

pub fn plot_member_names(x: i32, z: i32) -> rusqlite::Result<Vec<String>> {
    plot_member_names_in(&lock(), x, z)
}

fn plot_member_names_in(conn: &Connection, x: i32, z: i32) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT CASE WHEN name_current=TRUE THEN name ELSE uuid END AS name FROM user
         JOIN userplot ON userplot.user_id=user.id
         JOIN plot ON plot.id=userplot.plot_id
         WHERE plot_x=?1 AND plot_z=?2 AND is_owner=FALSE ORDER BY name COLLATE NOCASE",
    )?;
    let result = stmt.query_map(params![x, z], |row| row.get(0))?.collect();
    result
}

#[derive(Debug, PartialEq, Eq)]
pub enum MembershipResult {
    Changed { uuid: u128, name: String },
    Unchanged,
    UnknownPlayer,
    AmbiguousPlayer,
    PlotUnclaimed,
    NotOwner,
    IsOwner,
}

pub fn set_plot_member(
    x: i32,
    z: i32,
    actor: u128,
    admin: bool,
    name: &str,
    add: bool,
) -> rusqlite::Result<MembershipResult> {
    let mut conn = lock();
    let result = set_plot_member_in(&mut conn, x, z, actor, admin, name, add)?;
    if let MembershipResult::Changed { uuid, .. } = &result {
        let mut members = MEMBERS.write().unwrap();
        if add {
            members.insert((x, z, *uuid));
        } else {
            members.remove(&(x, z, *uuid));
        }
    }
    Ok(result)
}

fn set_plot_member_in(
    conn: &mut Connection,
    x: i32,
    z: i32,
    actor: u128,
    admin: bool,
    name: &str,
    add: bool,
) -> rusqlite::Result<MembershipResult> {
    let tx = conn.transaction()?;
    let owner: Option<(i64, String)> = tx
        .query_row(
            "SELECT plot.id, user.uuid FROM plot
         JOIN userplot ON userplot.plot_id=plot.id
         JOIN user ON user.id=userplot.user_id
         WHERE plot_x=?1 AND plot_z=?2 AND is_owner=TRUE",
            params![x, z],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((plot_id, owner)) = owner else {
        return Ok(MembershipResult::PlotUnclaimed);
    };
    if owner != format!("{actor:032x}") && !admin {
        return Ok(MembershipResult::NotOwner);
    }
    // A cached nickname can belong to a retired identity. UUIDs remain usable
    // for removing old memberships, without transferring any plot access.
    let uuid = if matches!(name.len(), 32 | 36) {
        name.parse::<crate::utils::HyphenatedUUID>().ok()
    } else {
        None
    };
    let (query, key) = if let Some(uuid) = uuid {
        (
            "SELECT id, uuid, CASE WHEN name_current=TRUE THEN name ELSE uuid END FROM user WHERE uuid=?1",
            format!("{:032x}", uuid.0),
        )
    } else {
        (
            "SELECT id, uuid, name FROM user WHERE name=?1 COLLATE NOCASE AND name_current=TRUE LIMIT 2",
            name.to_owned(),
        )
    };
    let mut stmt = tx.prepare(query)?;
    let mut users: Vec<(i64, String, String)> = stmt
        .query_map([key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    if users.len() > 1 {
        return Ok(MembershipResult::AmbiguousPlayer);
    }
    let Some((user_id, uuid, name)) = users.pop() else {
        return Ok(MembershipResult::UnknownPlayer);
    };
    if uuid == owner {
        return Ok(MembershipResult::IsOwner);
    }
    let parsed_uuid = u128::from_str_radix(&uuid, 16).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let changed = if add {
        tx.execute(
            "INSERT INTO userplot(user_id, plot_id, is_owner)
             SELECT ?1, ?2, FALSE WHERE NOT EXISTS(
                 SELECT 1 FROM userplot WHERE user_id=?1 AND plot_id=?2)",
            params![user_id, plot_id],
        )?
    } else {
        tx.execute(
            "DELETE FROM userplot WHERE user_id=?1 AND plot_id=?2 AND is_owner=FALSE",
            params![user_id, plot_id],
        )?
    };
    tx.commit()?;
    Ok(if changed > 0 {
        MembershipResult::Changed {
            uuid: parsed_uuid,
            name,
        }
    } else {
        MembershipResult::Unchanged
    })
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

#[derive(Debug, PartialEq, Eq)]
pub enum ClaimResult {
    Claimed,
    AlreadyClaimed,
    LimitReached(usize),
}

pub fn claim_plot(
    plot_x: i32,
    plot_z: i32,
    uuid: &str,
    limit: Option<usize>,
) -> rusqlite::Result<ClaimResult> {
    claim_plot_in(&mut lock(), plot_x, plot_z, uuid, limit)
}

fn claim_plot_in(
    conn: &mut Connection,
    plot_x: i32,
    plot_z: i32,
    uuid: &str,
    limit: Option<usize>,
) -> rusqlite::Result<ClaimResult> {
    let tx = conn.transaction()?;
    let claimed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM plot WHERE plot_x=?1 AND plot_z=?2)",
        params![plot_x, plot_z],
        |row| row.get(0),
    )?;
    if claimed {
        return Ok(ClaimResult::AlreadyClaimed);
    }
    // Resolve the user before writing either row; a failed claim leaves no orphan.
    let user_id: i64 = tx.query_row("SELECT id FROM user WHERE uuid=?1", [uuid], |row| {
        row.get(0)
    })?;
    if let Some(limit) = limit {
        // Count and insert under the same transaction and connection lock.
        // UUID ownership covers every plot, including unloaded plots.
        let owned: u64 = tx.query_row(
            "SELECT COUNT(DISTINCT plot_id) FROM userplot WHERE user_id=?1 AND is_owner=TRUE",
            [user_id],
            |row| row.get(0),
        )?;
        if owned >= limit as u64 {
            return Ok(ClaimResult::LimitReached(limit));
        }
    }
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
    Ok(ClaimResult::Claimed)
}

pub fn ensure_user(uuid: &str, name: &str) -> rusqlite::Result<()> {
    ensure_user_in(&mut lock(), uuid, name)
}

fn ensure_user_in(conn: &mut Connection, uuid: &str, name: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO user(uuid, name, name_current) VALUES (?1, ?2, TRUE)
         ON CONFLICT (uuid) DO UPDATE SET name=excluded.name, name_current=TRUE",
        params![uuid, name],
    )?;
    // The accepted login profile supplies the current UUID/name binding. Retain
    // conflicting UUID rows and their ownership/memberships, but stop resolving
    // their stale names. Rejoining the target alone repairs legacy collisions.
    tx.execute(
        "UPDATE user SET name_current=FALSE WHERE name=?1 COLLATE NOCASE AND uuid<>?2",
        params![name, uuid],
    )?;
    tx.commit()
}

fn init_user_name_cache(conn: &Connection) -> rusqlite::Result<()> {
    let migrated: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('user') WHERE name='name_current')",
        [],
        |row| row.get(0),
    )?;
    if !migrated {
        conn.execute(
            "ALTER TABLE user ADD COLUMN name_current BOOLEAN NOT NULL DEFAULT TRUE",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS user_current_name ON user(name COLLATE NOCASE) WHERE name_current=TRUE",
        [],
    )?;
    Ok(())
}

pub fn init() {
    let conn = lock();
    conn.pragma_update(None, "foreign_keys", true)
        .expect("Could not enable plot foreign keys");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("Could not set plot database timeout");

    init_warps(&conn).expect("Could not initialize saved warps");

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

    init_user_name_cache(&conn).expect("Could not initialize the current player-name cache");

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
    let mut stmt = conn
        .prepare(
            "SELECT plot_x, plot_z, uuid FROM plot
         JOIN userplot ON userplot.plot_id=plot.id
         JOIN user ON user.id=userplot.user_id WHERE is_owner=FALSE",
        )
        .expect("Could not read plot members");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i32>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("Could not read plot members");
    let mut members = MEMBERS.write().unwrap();
    members.clear();
    for row in rows {
        let (x, z, uuid) = row.expect("Could not read plot member");
        members.insert((
            x,
            z,
            u128::from_str_radix(&uuid, 16).expect("Invalid plot member UUID"),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warps_survive_reopening_and_replace_names_case_insensitively() {
        let path = std::env::temp_dir().join(format!("mchprs-warps-{}.db", rand::random::<u128>()));
        let warp = Warp {
            pos: PlayerPos::new(-512.25, 65.75, 1024.5),
            yaw: 123.5,
            pitch: -20.0,
        };
        {
            let conn = Connection::open(&path).unwrap();
            init_warps(&conn).unwrap();
            assert!(warp_names_in(&conn).unwrap().is_empty());
            set_warp_in(&conn, "Spawn", Warp { yaw: 0.0, ..warp }).unwrap();
            set_warp_in(&conn, "sPaWn", warp).unwrap();
            set_warp_in(&conn, "CPU", warp).unwrap();
        }
        {
            let conn = Connection::open(&path).unwrap();
            init_warps(&conn).unwrap();
            let saved = get_warp_in(&conn, "SPAWN").unwrap().unwrap();
            assert_eq!(
                (
                    saved.pos.x,
                    saved.pos.y,
                    saved.pos.z,
                    saved.yaw,
                    saved.pitch
                ),
                (warp.pos.x, warp.pos.y, warp.pos.z, warp.yaw, warp.pitch)
            );
            assert_eq!(warp_names_in(&conn).unwrap(), ["CPU", "Spawn"]);
            assert!(get_warp_in(&conn, "missing").unwrap().is_none());
            assert!(saved.is_valid());
            assert!(!Warp {
                pos: PlayerPos::new(f64::NAN, 64.0, 0.0),
                ..warp
            }
            .is_valid());
            assert!(!Warp {
                yaw: f32::INFINITY,
                ..warp
            }
            .is_valid());
            assert!(!Warp {
                pitch: f32::NAN,
                ..warp
            }
            .is_valid());
        }
        std::fs::remove_file(path).unwrap();
    }

    fn legacy_name_collision() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE user(id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, name TEXT NOT NULL);
            CREATE TABLE plot(id INTEGER PRIMARY KEY, plot_x INTEGER, plot_z INTEGER, UNIQUE(plot_x, plot_z));
            CREATE TABLE userplot(user_id INTEGER REFERENCES user(id), plot_id INTEGER REFERENCES plot(id), is_owner BOOLEAN);").unwrap();
        for (id, name) in [(1, "Owner"), (2, "Kazik24"), (3, "KAZIK24")] {
            conn.execute(
                "INSERT INTO user VALUES(?1,?2,?3)",
                params![id, format!("{id:032x}"), name],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO plot VALUES(1,2,3),(2,4,5);
            INSERT INTO userplot VALUES(1,1,TRUE),(2,2,TRUE),(2,1,FALSE);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn target_rejoin_resolves_legacy_name_collision_without_transferring_access() {
        let mut conn = legacy_name_collision();
        init_user_name_cache(&conn).unwrap();
        init_user_name_cache(&conn).unwrap();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Kazik24", true).unwrap(),
            MembershipResult::AmbiguousPlayer
        );
        ensure_user_in(&mut conn, &format!("{:032x}", 3), "Kazik24").unwrap();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "kAzIk24", true).unwrap(),
            MembershipResult::Changed {
                uuid: 3,
                name: "Kazik24".into()
            }
        );
        assert_eq!(get_cached_username_in(&conn, &format!("{:032x}", 2)), None);
        assert_eq!(
            get_cached_username_in(&conn, &format!("{:032x}", 3)),
            Some("Kazik24".into())
        );
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 2).unwrap(), [(4, 5)]);
        assert!(get_owned_plots_by_uuid_in(&conn, 3).unwrap().is_empty());
        assert!(get_owned_plots_in(&conn, "Kazik24").unwrap().is_empty());
        assert_eq!(
            plot_member_names_in(&conn, 2, 3).unwrap(),
            [format!("{:032x}", 2), "Kazik24".into()]
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM user", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM userplot WHERE user_id=2", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            2
        );
        let uuid = crate::utils::HyphenatedUUID(2).to_string();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, &uuid, false).unwrap(),
            MembershipResult::Changed {
                uuid: 2,
                name: format!("{:032x}", 2)
            }
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, &format!("{:032x}", 1), false).unwrap(),
            MembershipResult::IsOwner
        );
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 2).unwrap(), [(4, 5)]);
        assert_eq!(plot_member_names_in(&conn, 2, 3).unwrap(), ["Kazik24"]);
    }

    #[test]
    fn rename_and_new_uuid_registration_keep_only_current_names_resolvable() {
        let mut conn = legacy_name_collision();
        init_user_name_cache(&conn).unwrap();
        // This also covers a new proxy UUID replacing an old offline identity.
        ensure_user_in(&mut conn, &format!("{:032x}", 4), "kazik24").unwrap();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "KAZIK24", true).unwrap(),
            MembershipResult::Changed {
                uuid: 4,
                name: "kazik24".into()
            }
        );
        ensure_user_in(&mut conn, &format!("{:032x}", 4), "NewKazik").unwrap();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Kazik24", true).unwrap(),
            MembershipResult::UnknownPlayer
        );
        assert_eq!(known_usernames_in(&conn).unwrap(), ["NewKazik", "Owner"]);
        ensure_user_in(&mut conn, &format!("{:032x}", 2), "FormerKazik").unwrap();
        assert_eq!(get_owned_plots_in(&conn, "formerkazik").unwrap(), [(4, 5)]);
        assert_eq!(
            plot_member_names_in(&conn, 2, 3).unwrap(),
            ["FormerKazik", "NewKazik"]
        );
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 2).unwrap(), [(4, 5)]);
    }

    #[test]
    fn failed_name_refresh_rolls_back_registration_and_keeps_legacy_access() {
        let mut conn = legacy_name_collision();
        init_user_name_cache(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_refresh BEFORE UPDATE OF name_current ON user
            WHEN OLD.id=2 BEGIN SELECT RAISE(ABORT,'failed refresh'); END;",
        )
        .unwrap();
        assert!(ensure_user_in(&mut conn, &format!("{:032x}", 4), "Kazik24").is_err());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM user", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM user WHERE name_current=TRUE",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 2).unwrap(), [(4, 5)]);
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Kazik24", true).unwrap(),
            MembershipResult::AmbiguousPlayer
        );
    }

    #[test]
    fn only_owners_or_admins_can_manage_members_and_ownership_is_preserved() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE user(id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, name TEXT);
            CREATE TABLE plot(id INTEGER PRIMARY KEY, plot_x INTEGER, plot_z INTEGER, UNIQUE(plot_x, plot_z));
            CREATE TABLE userplot(user_id INTEGER REFERENCES user(id), plot_id INTEGER REFERENCES plot(id), is_owner BOOLEAN);").unwrap();
        init_user_name_cache(&conn).unwrap();
        for (id, name) in [(1, "Owner"), (2, "Builder"), (3, "Other")] {
            conn.execute(
                "INSERT INTO user(id, uuid, name) VALUES(?1, ?2, ?3)",
                params![id, format!("{id:032x}"), name],
            )
            .unwrap();
        }
        assert_eq!(
            claim_plot_in(&mut conn, 2, 3, &format!("{:032x}", 1), None).unwrap(),
            ClaimResult::Claimed
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 3, false, "Builder", true).unwrap(),
            MembershipResult::NotOwner
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "bUiLdEr", true).unwrap(),
            MembershipResult::Changed {
                uuid: 2,
                name: "Builder".into()
            }
        );
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 1).unwrap(), [(2, 3)]);
        conn.execute("UPDATE user SET name='BUILDER' WHERE id=3", [])
            .unwrap();
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Builder", true).unwrap(),
            MembershipResult::AmbiguousPlayer
        );
        conn.execute("UPDATE user SET name='Other' WHERE id=3", [])
            .unwrap();
        assert!(get_owned_plots_by_uuid_in(&conn, 2).unwrap().is_empty());
        conn.execute("UPDATE user SET name='RenamedOwner' WHERE id=1", [])
            .unwrap();
        assert_eq!(get_owned_plots_by_uuid_in(&conn, 1).unwrap(), [(2, 3)]);
        conn.execute("UPDATE user SET name='Owner' WHERE id=1", [])
            .unwrap();
        assert_eq!(
            claim_plot_in(&mut conn, -10, -20, &format!("{:032x}", 1), None).unwrap(),
            ClaimResult::Claimed
        );
        assert_eq!(
            get_owned_plots_by_uuid_in(&conn, 1).unwrap(),
            [(2, 3), (-10, -20)]
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Builder", true).unwrap(),
            MembershipResult::Unchanged
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 2, false, "Other", true).unwrap(),
            MembershipResult::NotOwner
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Owner", false).unwrap(),
            MembershipResult::IsOwner
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Missing", true).unwrap(),
            MembershipResult::UnknownPlayer
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 20, 30, 1, true, "Builder", true).unwrap(),
            MembershipResult::PlotUnclaimed
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 3, true, "Builder", false).unwrap(),
            MembershipResult::Changed {
                uuid: 2,
                name: "Builder".into()
            }
        );
        assert_eq!(
            set_plot_member_in(&mut conn, 2, 3, 1, false, "Builder", false).unwrap(),
            MembershipResult::Unchanged
        );
        let rows: Vec<(i64, bool)> = conn
            .prepare("SELECT user_id, is_owner FROM userplot")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows, [(1, true), (1, true)]);
    }

    #[test]
    fn claims_are_atomic_and_duplicate_claims_cannot_change_the_owner() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE user(id INTEGER PRIMARY KEY, uuid TEXT UNIQUE, name TEXT);
            CREATE TABLE plot(id INTEGER PRIMARY KEY, plot_x INTEGER, plot_z INTEGER, UNIQUE(plot_x, plot_z));
            CREATE TABLE userplot(user_id INTEGER REFERENCES user(id), plot_id INTEGER REFERENCES plot(id), is_owner BOOLEAN);
            INSERT INTO user VALUES(1, 'first', 'First'), (2, 'second', 'Second');").unwrap();
        assert!(claim_plot_in(&mut conn, 0, 0, "missing", None).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            claim_plot_in(&mut conn, 0, 0, "first", None).unwrap(),
            ClaimResult::Claimed
        );
        assert_eq!(
            claim_plot_in(&mut conn, 0, 0, "second", None).unwrap(),
            ClaimResult::AlreadyClaimed
        );
        let owner: i64 = conn
            .query_row("SELECT user_id FROM userplot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(owner, 1);
        conn.execute_batch("CREATE TRIGGER fail_claim BEFORE INSERT ON userplot BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
        assert!(claim_plot_in(&mut conn, 1, 0, "second", None).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM plot", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
