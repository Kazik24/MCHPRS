use super::*;
use mysql::prelude::*;
use mysql::{OptsBuilder, Pool};
use postgres::{Client, NoTls};

pub(super) struct Database {
    pub(super) config: PermissionsConfig,
    mysql: Option<Pool>,
}
impl Database {
    pub(super) fn new(config: PermissionsConfig) -> Result<Self> {
        if config.table_prefix.is_empty() || !config.table_prefix.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
            bail!("LuckPerms table_prefix must contain only ASCII letters, digits and underscores");
        }
        let mysql = match config.storage {
            Storage::Mysql => Some(Pool::new(
                OptsBuilder::new()
                    .ip_or_hostname(Some(config.host.clone()))
                    .tcp_port(config.port.unwrap_or(3306))
                    .db_name(Some(config.db_name.clone()))
                    .user(Some(config.username.clone()))
                    .pass(Some(config.password.clone())),
            )?),
            Storage::Postgres => None,
        };
        Ok(Self { config, mysql })
    }
    pub(super) fn postgres(&self) -> Result<Client> {
        Ok(postgres::Config::new()
            .host(&self.config.host)
            .port(self.config.port.unwrap_or(5432))
            .dbname(&self.config.db_name)
            .user(&self.config.username)
            .password(&self.config.password)
            .application_name("mchprs-luckperms")
            .connect_timeout(Duration::from_secs(5))
            .options("-c default_transaction_read_only=on -c statement_timeout=5000 -c lock_timeout=1000")
            .connect(NoTls)?)
    }
    /// Read a consistent snapshot using SELECT only. Resolve inheritance locally,
    /// avoiding backend differences in boolean columns and recursive SQL types.
    pub(super) fn read_nodes(&self, uuid: &str) -> Result<(Vec<RawNode>, Vec<RawNode>)> {
        let columns = "permission, value, server, world, expiry, contexts";
        let users = format!("{}user_permissions", self.config.table_prefix);
        let groups = format!("{}group_permissions", self.config.table_prefix);
        match self.config.storage {
            Storage::Postgres => {
                let mut conn = self.postgres()?;
                let mut tx = conn
                    .build_transaction()
                    .isolation_level(postgres::IsolationLevel::RepeatableRead)
                    .read_only(true)
                    .start()?;
                let user_rows = tx.query(&format!("SELECT {columns} FROM {users} WHERE uuid = $1"), &[&uuid])?;
                let group_rows = tx.query(&format!("SELECT name, {columns} FROM {groups}"), &[])?;
                let convert = |row: &postgres::Row, group: bool| -> Result<RawNode> {
                    Ok(RawNode {
                        group: if group { row.try_get("name")? } else { String::new() },
                        permission: row.try_get("permission")?,
                        value: row.try_get("value")?,
                        server: row.try_get("server")?,
                        world: row.try_get("world")?,
                        expiry: row.try_get("expiry")?,
                        contexts: row.try_get("contexts")?,
                    })
                };
                let nodes = (
                    user_rows.iter().map(|r| convert(r, false)).collect::<Result<_>>()?,
                    group_rows.iter().map(|r| convert(r, true)).collect::<Result<_>>()?,
                );
                tx.commit()?;
                Ok(nodes)
            }
            Storage::Mysql => {
                let mut conn = self.mysql.as_ref().context("Missing MySQL pool")?.get_conn()?;
                let mut tx = conn.start_transaction(
                    mysql::TxOpts::default()
                        .set_isolation_level(Some(mysql::IsolationLevel::RepeatableRead))
                        .set_access_mode(Some(mysql::AccessMode::ReadOnly)),
                )?;
                let user_rows: Vec<mysql::Row> = tx.exec(format!("SELECT {columns} FROM {users} WHERE uuid = ?"), (uuid,))?;
                let group_rows: Vec<mysql::Row> = tx.query(format!("SELECT name, {columns} FROM {groups}"))?;
                fn column<T: FromValue>(row: &mysql::Row, name: &str) -> Result<T> {
                    row.get_opt(name)
                        .context("Missing LuckPerms column")?
                        .map_err(|_| anyhow!("Invalid LuckPerms column: {name}"))
                }
                let convert = |row: &mysql::Row, group: bool| -> Result<RawNode> {
                    Ok(RawNode {
                        group: if group { column(row, "name")? } else { String::new() },
                        permission: column(row, "permission")?,
                        value: column::<i32>(row, "value")? > 0,
                        server: column(row, "server")?,
                        world: column(row, "world")?,
                        expiry: column(row, "expiry")?,
                        contexts: column(row, "contexts")?,
                    })
                };
                let nodes = (
                    user_rows.iter().map(|r| convert(r, false)).collect::<Result<_>>()?,
                    group_rows.iter().map(|r| convert(r, true)).collect::<Result<_>>()?,
                );
                tx.commit()?;
                Ok(nodes)
            }
        }
    }
}
