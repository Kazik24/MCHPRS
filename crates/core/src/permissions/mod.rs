use crate::utils::HyphenatedUUID;
use anyhow::{Context, Result, anyhow, bail};
use mysql::prelude::*;
use mysql::{OptsBuilder, Pool};
use once_cell::sync::OnceCell;
use postgres::{Client, NoTls};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod rank;
pub use rank::{Rank, RankProfile};

static DATABASE: OnceCell<Database> = OnceCell::new();

#[derive(Default, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Storage {
    #[default]
    Mysql,
    #[serde(alias = "postgresql")]
    Postgres,
}
fn default_prefix() -> String {
    "luckperms_".into()
}
fn global_context() -> String {
    "global".into()
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PermissionsConfig {
    #[serde(default)]
    storage: Storage,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    db_name: String,
    username: String,
    password: String,
    #[serde(default = "default_prefix")]
    table_prefix: String,
    server_context: String,
    #[serde(default = "global_context")]
    world_context: String,
    #[serde(default)]
    plotsquared_compat: bool,
    #[serde(default)]
    pub redstonefun_ranks: bool,
    #[serde(default)]
    pub mchprs_permissions: bool,
}
struct Database {
    config: PermissionsConfig,
    mysql: Option<Pool>,
}
impl Database {
    fn new(config: PermissionsConfig) -> Result<Self> {
        if config.table_prefix.is_empty()
            || !config
                .table_prefix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
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
    fn postgres(&self) -> Result<Client> {
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
    fn read_nodes(&self, uuid: &str) -> Result<(Vec<RawNode>, Vec<RawNode>)> {
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
                let user_rows = tx.query(
                    &format!("SELECT {columns} FROM {users} WHERE uuid = $1"),
                    &[&uuid],
                )?;
                let group_rows = tx.query(&format!("SELECT name, {columns} FROM {groups}"), &[])?;
                let convert = |row: &postgres::Row, group: bool| -> Result<RawNode> {
                    Ok(RawNode {
                        group: if group {
                            row.try_get("name")?
                        } else {
                            String::new()
                        },
                        permission: row.try_get("permission")?,
                        value: row.try_get("value")?,
                        server: row.try_get("server")?,
                        world: row.try_get("world")?,
                        expiry: row.try_get("expiry")?,
                        contexts: row.try_get("contexts")?,
                    })
                };
                let nodes = (
                    user_rows
                        .iter()
                        .map(|r| convert(r, false))
                        .collect::<Result<_>>()?,
                    group_rows
                        .iter()
                        .map(|r| convert(r, true))
                        .collect::<Result<_>>()?,
                );
                tx.commit()?;
                Ok(nodes)
            }
            Storage::Mysql => {
                let mut conn = self
                    .mysql
                    .as_ref()
                    .context("Missing MySQL pool")?
                    .get_conn()?;
                let mut tx = conn.start_transaction(
                    mysql::TxOpts::default()
                        .set_isolation_level(Some(mysql::IsolationLevel::RepeatableRead))
                        .set_access_mode(Some(mysql::AccessMode::ReadOnly)),
                )?;
                let user_rows: Vec<mysql::Row> = tx.exec(
                    format!("SELECT {columns} FROM {users} WHERE uuid = ?"),
                    (uuid,),
                )?;
                let group_rows: Vec<mysql::Row> =
                    tx.query(format!("SELECT name, {columns} FROM {groups}"))?;
                fn column<T: FromValue>(row: &mysql::Row, name: &str) -> Result<T> {
                    row.get_opt(name)
                        .context("Missing LuckPerms column")?
                        .map_err(|_| anyhow!("Invalid LuckPerms column: {name}"))
                }
                let convert = |row: &mysql::Row, group: bool| -> Result<RawNode> {
                    Ok(RawNode {
                        group: if group {
                            column(row, "name")?
                        } else {
                            String::new()
                        },
                        permission: column(row, "permission")?,
                        value: column::<i32>(row, "value")? > 0,
                        server: column(row, "server")?,
                        world: column(row, "world")?,
                        expiry: column(row, "expiry")?,
                        contexts: column(row, "contexts")?,
                    })
                };
                let nodes = (
                    user_rows
                        .iter()
                        .map(|r| convert(r, false))
                        .collect::<Result<_>>()?,
                    group_rows
                        .iter()
                        .map(|r| convert(r, true))
                        .collect::<Result<_>>()?,
                );
                tx.commit()?;
                Ok(nodes)
            }
        }
    }
}
#[derive(Clone, Debug)]
struct RawNode {
    group: String,
    permission: String,
    value: bool,
    server: String,
    world: String,
    expiry: i64,
    contexts: String,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn earliest_expiry(a: i64, b: i64) -> i64 {
    match (a, b) {
        (0, _) => b,
        (_, 0) => a,
        _ => a.min(b),
    }
}
impl RawNode {
    fn applies(&self, config: &PermissionsConfig, time: i64) -> bool {
        if (self.expiry != 0 && self.expiry <= time)
            || (self.server != "global" && self.server != config.server_context)
            || (self.world != "global" && self.world != config.world_context)
        {
            return false;
        }
        let Ok(serde_json::Value::Object(contexts)) = serde_json::from_str(&self.contexts) else {
            return false;
        };
        contexts.iter().all(|(key, value)| {
            let expected = match key.as_str() {
                "server" => &config.server_context,
                "world" => &config.world_context,
                _ => return false, // Unsupported dynamic contexts cannot grant access.
            };
            value.as_str() == Some(expected.as_str())
                || value.as_array().is_some_and(|values| {
                    values.iter().any(|v| v.as_str() == Some(expected.as_str()))
                })
        })
    }
}
#[derive(Debug)]
struct PermissionNode {
    permission: String,
    value: bool,
    // Direct grants, scopes, specificity, group distance and weight.
    priority: (bool, usize, usize, bool, std::cmp::Reverse<usize>, i64),
    expiry: i64,
}
impl PermissionNode {
    fn matches(&self, query: &str) -> bool {
        let mut query = query.split('.');
        for segment in self.permission.split('.') {
            if segment == "*" {
                return true;
            }
            if query.next() != Some(segment) {
                return false;
            }
        }
        query.next().is_none()
    }
}
#[derive(Default, Debug)]
pub struct PlayerPermissionsCache {
    nodes: Vec<PermissionNode>,
    plotsquared_compat: bool,
    pub rank_profile: Option<RankProfile>,
    rank_budget_expiry: i64,
    mchprs_permissions: bool,
    valid_until: Option<Instant>,
}

/// Keep existing handler permission names while isolating MCHPRS from Paper's
/// permission packs. Already-namespaced nodes are used without modification.
fn mchprs_node(name: &str) -> String {
    if name.starts_with("mchprs.") {
        name.to_owned()
    } else if let Some(command) = name.strip_prefix("minecraft.command.") {
        format!("mchprs.commands.{command}")
    } else {
        format!("mchprs.{name}")
    }
}

pub fn dedicated_permissions() -> bool {
    crate::config::CONFIG
        .luckperms
        .as_ref()
        .is_some_and(|config| config.mchprs_permissions)
}

pub fn ranked_chat() -> bool {
    crate::config::CONFIG
        .luckperms
        .as_ref()
        .is_some_and(|config| config.redstonefun_ranks)
}
impl PlayerPermissionsCache {
    #[cfg(test)]
    pub(crate) fn for_test(nodes: &[&str]) -> Self {
        Self {
            nodes: nodes
                .iter()
                .map(|permission| PermissionNode {
                    permission: (*permission).into(),
                    value: true,
                    priority: (true, 0, 0, false, std::cmp::Reverse(0), 0),
                    expiry: 0,
                })
                .collect(),
            ..Default::default()
        }
    }

    pub fn compilation_budget_multiplier(&self) -> usize {
        if (self.rank_budget_expiry != 0 && self.rank_budget_expiry <= now())
            || self.valid_until.is_none_or(|until| Instant::now() >= until)
        {
            return 1;
        }
        self.rank_profile
            .as_ref()
            .map_or(1, |profile| profile.rank.compilation_budget_multiplier())
    }

    /// Numeric limits are ordinary boolean nodes such as mchprs.history.limit.200.
    /// Only effective positive nodes count, so exact denials and expiry apply.
    pub fn numeric_limit(&self, prefix: &str) -> Option<usize> {
        self.nodes
            .iter()
            .filter_map(|node| {
                let limit = node
                    .permission
                    .strip_prefix(prefix)?
                    .parse::<usize>()
                    .ok()?;
                (self.get_node_val(&node.permission) == Some(1)).then_some(limit)
            })
            .max()
    }

    fn stored_node_val(&self, name: &str) -> Option<i32> {
        self.stored_node_val_at(name, now())
    }
    fn stored_node_val_at(&self, name: &str, time: i64) -> Option<i32> {
        if self
            .valid_until
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return None;
        }
        self.nodes
            .iter()
            .filter(|n| (n.expiry == 0 || n.expiry > time) && n.matches(name))
            .max_by_key(|n| (n.priority, !n.value)) // Deny wins an equal tie.
            .map(|n| i32::from(n.value))
    }
    pub fn get_node_val(&self, name: &str) -> Option<i32> {
        if self.mchprs_permissions {
            return self.stored_node_val(&mchprs_node(name));
        }
        if let Some(value) = self.stored_node_val(name) {
            return Some(value);
        }
        if !self.plotsquared_compat {
            return None;
        }
        // PlotSquared 7.3.11 plugin.yml children used by MCHPRS. Keep explicit
        // grants/denials above, and never infer administrative permissions.
        let basic = matches!(
            name,
            "plots.info" | "plots.claim" | "plots.auto" | "plots.visit" | "plots.middle"
        );
        if basic {
            return self.stored_node_val("plots.permpack.basic");
        }
        let equivalent = match name {
            "plots.lock" => "plots.flag",
            "plots.select" => "worldedit.selection.pos",
            "commands.rhistory" | "commands.rback" => "plots.set",
            _ => return None,
        };
        self.stored_node_val(equivalent).or_else(|| {
            if matches!(equivalent, "plots.flag" | "plots.set") {
                self.stored_node_val("plots.permpack.basic")
            } else {
                None
            }
        })
    }
    fn resolve(
        users: Vec<RawNode>,
        groups: Vec<RawNode>,
        config: &PermissionsConfig,
        time: i64,
    ) -> Self {
        let users: Vec<_> = users
            .into_iter()
            .filter(|n| n.applies(config, time))
            .collect();
        let mut grouped: HashMap<String, Vec<RawNode>> = HashMap::new();
        for node in groups.into_iter().filter(|n| n.applies(config, time)) {
            grouped.entry(node.group.clone()).or_default().push(node);
        }
        let denied: HashSet<_> = users
            .iter()
            .filter(|n| !n.value)
            .filter_map(|n| n.permission.strip_prefix("group.").map(str::to_owned))
            .collect();
        let mut roots: VecDeque<_> = users
            .iter()
            .filter(|n| n.value)
            .filter_map(|n| {
                n.permission
                    .strip_prefix("group.")
                    .map(|g| (g.to_owned(), 0, n.expiry))
            })
            .collect();
        if roots.is_empty() && !denied.contains("default") {
            roots.push_back(("default".to_owned(), 0, 0));
        }
        let mut result = Self {
            nodes: Vec::new(),
            plotsquared_compat: config.plotsquared_compat,
            rank_profile: None,
            rank_budget_expiry: 0,
            mchprs_permissions: config.mchprs_permissions,
            valid_until: Some(Instant::now() + Duration::from_secs(30)),
        };
        let mut add = |node: &RawNode, direct: bool, depth: usize, weight: i64, expiry: i64| {
            if ["group.", "weight.", "prefix.", "suffix."]
                .iter()
                .any(|prefix| node.permission.starts_with(prefix))
            {
                return;
            }
            let segments = node.permission.split('.').take_while(|s| *s != "*").count();
            result.nodes.push(PermissionNode {
                permission: node.permission.clone(),
                value: node.value,
                expiry: earliest_expiry(node.expiry, expiry),
                priority: (
                    direct,
                    usize::from(node.server != "global")
                        + usize::from(node.world != "global")
                        + usize::from(node.contexts != "{}"),
                    segments,
                    !node.permission.contains('*'),
                    std::cmp::Reverse(depth),
                    weight,
                ),
            });
        };
        for node in &users {
            add(node, true, 0, 0, 0);
        }
        // A group can have multiple paths: a shorter temporary path and a longer
        // permanent path must both survive. Dominated paths also bound cycles.
        let mut visited: HashMap<String, Vec<(usize, i64)>> = HashMap::new();
        while let Some((name, depth, expiry)) = roots.pop_front() {
            if denied.contains(&name) {
                continue;
            }
            let paths = visited.entry(name.clone()).or_default();
            let lasts_at_least = |a: i64, b: i64| a == 0 || (b != 0 && a >= b);
            if paths
                .iter()
                .any(|&(d, e)| d <= depth && lasts_at_least(e, expiry))
            {
                continue;
            }
            paths.retain(|&(d, e)| !(depth <= d && lasts_at_least(expiry, e)));
            paths.push((depth, expiry));
            let Some(nodes) = grouped.get(&name) else {
                continue;
            };
            let weight = nodes
                .iter()
                .filter(|n| n.value)
                .filter_map(|n| n.permission.strip_prefix("weight.")?.parse::<i64>().ok())
                .max()
                .unwrap_or(0);
            for node in nodes {
                if let Some(parent) = node.permission.strip_prefix("group.") {
                    if node.value {
                        roots.push_back((
                            parent.to_owned(),
                            depth + 1,
                            earliest_expiry(expiry, node.expiry),
                        ));
                    }
                } else {
                    add(node, false, depth, weight, expiry);
                }
            }
        }
        if config.redstonefun_ranks {
            let rank = visited
                .keys()
                .filter_map(|group| Rank::from_group(group))
                .max()
                .unwrap_or_default();
            result.rank_budget_expiry = visited.get(rank.group()).map_or(0, |paths| {
                if paths.iter().any(|&(_, expiry)| expiry == 0) {
                    0
                } else {
                    paths.iter().map(|&(_, expiry)| expiry).max().unwrap_or(0)
                }
            });
            let prefix = grouped
                .get(rank.group())
                .into_iter()
                .flatten()
                .filter(|node| node.value)
                .filter_map(|node| {
                    let text = node.permission.strip_prefix("prefix.")?;
                    let (priority, prefix) = text.split_once('.')?;
                    let priority = priority.parse::<i64>().ok()?;
                    let scope = usize::from(node.server != "global")
                        + usize::from(node.world != "global")
                        + usize::from(node.contexts != "{}");
                    Some(((priority, scope, prefix), prefix))
                })
                .max_by_key(|(priority, _)| *priority)
                .map(|(_, prefix)| prefix.to_owned());
            result.rank_profile = Some(RankProfile { rank, prefix });
        }
        result
    }
}
pub fn init(config: PermissionsConfig) -> Result<()> {
    let database = Database::new(config)?;
    // Validate connection/schema/SELECT access before accepting players.
    database.read_nodes("00000000-0000-0000-0000-000000000000")?;
    DATABASE
        .set(database)
        .map_err(|_| anyhow!("Tried to init permissions more than once"))?;
    Ok(())
}
pub fn load_player_cache(uuid: u128) -> Result<PlayerPermissionsCache> {
    let database = DATABASE
        .get()
        .context("Tried to load permissions before init")?;
    let (users, groups) = database.read_nodes(&HyphenatedUUID(uuid).to_string())?;
    Ok(PlayerPermissionsCache::resolve(
        users,
        groups,
        &database.config,
        now(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> PermissionsConfig {
        toml::from_str("host='localhost'\ndb_name='rf'\nusername='reader'\npassword='unused'\nserver_context='global'\nworld_context='redstoneplots'").unwrap()
    }
    fn node(group: &str, permission: &str, value: bool) -> RawNode {
        RawNode {
            group: group.into(),
            permission: permission.into(),
            value,
            server: "global".into(),
            world: "global".into(),
            expiry: 0,
            contexts: "{}".into(),
        }
    }
    #[test]
    fn compilation_budget_follows_effective_rank_and_rejects_stale_cache() {
        let mut config = config();
        config.redstonefun_ranks = true;
        for (group, multiplier) in [
            ("default", 1),
            ("builder", 1),
            ("advanced", 2),
            ("expert", 4),
            ("engineer", 8),
            ("moderator", 8),
            ("admin", 8),
        ] {
            let mut cache = PlayerPermissionsCache::resolve(
                vec![node("", &format!("group.{group}"), true)],
                vec![],
                &config,
                now(),
            );
            assert_eq!(cache.compilation_budget_multiplier(), multiplier);
            cache.valid_until = Some(Instant::now() - Duration::from_secs(1));
            assert_eq!(cache.compilation_budget_multiplier(), 1);
        }
        let mut expired = node("", "group.admin", true);
        expired.expiry = now() - 1;
        let cache = PlayerPermissionsCache::resolve(vec![expired], vec![], &config, now());
        assert_eq!(cache.compilation_budget_multiplier(), 1);
        let mut temporary = node("", "group.admin", true);
        temporary.expiry = now() + 60;
        let mut cache = PlayerPermissionsCache::resolve(vec![temporary], vec![], &config, now());
        assert_eq!(cache.compilation_budget_multiplier(), 8);
        cache.rank_budget_expiry = now() - 1;
        assert_eq!(cache.compilation_budget_multiplier(), 1);
    }

    #[test]
    fn git_access_can_be_granted_or_denied_including_an_admin_wildcard() {
        let mut config = config();
        config.mchprs_permissions = true;
        let missing = PlayerPermissionsCache::resolve(vec![], vec![], &config, now());
        assert_eq!(missing.get_node_val("commands.git"), None);
        let granted = PlayerPermissionsCache::resolve(
            vec![node("", "mchprs.commands.git", true)],
            vec![],
            &config,
            now(),
        );
        assert_eq!(granted.get_node_val("commands.git"), Some(1));
        let denied = PlayerPermissionsCache::resolve(
            vec![node("", "mchprs.commands.git", false)],
            vec![node("default", "mchprs.*", true)],
            &config,
            now(),
        );
        assert_eq!(denied.get_node_val("commands.git"), Some(0));
        assert_eq!(denied.get_node_val("plots.admin.git"), Some(1));
    }

    #[test]
    fn deployed_git_rank_policy_starts_at_expert_with_owner_storage_tiers() {
        let mut config = config();
        config.server_context = "mchprs".into();
        config.mchprs_permissions = true;
        config.redstonefun_ranks = true;
        let mut groups = vec![
            node("default", "mchprs.*", false),
            node("builder", "group.default", true),
            node("advanced", "group.default", true),
            node("advanced", "group.builder", true),
            node("expert", "group.advanced", true),
            node("expert", "group.builder", true),
            node("expert", "group.default", true),
            node("engineer", "group.advanced", true),
            node("engineer", "group.builder", true),
            node("engineer", "group.default", true),
            node("engineer", "group.expert", true),
            node("moderator", "mchprs.*", true),
            node("admin", "mchprs.*", true),
        ];
        for (group, allowance) in [
            ("expert", 100),
            ("engineer", 1024),
            ("moderator", 1024),
            ("admin", 1024),
        ] {
            groups.push(node(group, "mchprs.commands.git", true));
            groups.push(node(
                group,
                &format!("mchprs.git.storage.{allowance}"),
                true,
            ));
        }
        for node in &mut groups {
            if node.permission.starts_with("mchprs.") {
                node.server = "mchprs".into();
            }
        }
        for rank in [
            Rank::Player,
            Rank::Builder,
            Rank::Advanced,
            Rank::Expert,
            Rank::Engineer,
            Rank::Moderator,
            Rank::Admin,
        ] {
            let cache = PlayerPermissionsCache::resolve(
                vec![node("", &format!("group.{}", rank.group()), true)],
                groups.clone(),
                &config,
                now(),
            );
            assert_eq!(
                cache.get_node_val("commands.git"),
                Some(i32::from(rank >= Rank::Expert)),
                "{rank:?}"
            );
            assert_eq!(
                cache.numeric_limit("mchprs.git.storage."),
                match rank {
                    Rank::Expert => Some(100),
                    Rank::Engineer | Rank::Moderator | Rank::Admin => Some(1024),
                    _ => None,
                },
                "{rank:?}"
            );
            assert_eq!(cache.rank_profile.unwrap().rank, rank);
        }
        let overridden = PlayerPermissionsCache::resolve(
            vec![
                node("", "group.engineer", true),
                node("", "mchprs.commands.git", false),
            ],
            groups,
            &config,
            now(),
        );
        assert_eq!(overridden.get_node_val("commands.git"), Some(0));
    }

    #[test]
    fn git_storage_ranks_respect_inheritance_denials_expiry_and_namespace() {
        for dedicated in [false, true] {
            let mut config = config();
            config.mchprs_permissions = dedicated;
            let prefix = if dedicated {
                "mchprs.git.storage."
            } else {
                "git.storage."
            };
            let grants = vec![
                node("default", &format!("{prefix}100"), true),
                node("engineer", "group.default", true),
                node("engineer", &format!("{prefix}1024"), true),
            ];
            let base = PlayerPermissionsCache::resolve(vec![], grants.clone(), &config, now());
            assert_eq!(base.numeric_limit(prefix), Some(100));
            let promoted = PlayerPermissionsCache::resolve(
                vec![node("", "group.engineer", true)],
                grants.clone(),
                &config,
                now(),
            );
            assert_eq!(promoted.numeric_limit(prefix), Some(1024));
            let denied = PlayerPermissionsCache::resolve(
                vec![
                    node("", "group.engineer", true),
                    node("", &format!("{prefix}1024"), false),
                ],
                grants.clone(),
                &config,
                now(),
            );
            assert_eq!(denied.numeric_limit(prefix), Some(100));
            let mut expired = node("", &format!("{prefix}1024"), true);
            expired.expiry = now() - 1;
            let mut fallback =
                PlayerPermissionsCache::resolve(vec![expired], grants, &config, now());
            assert_eq!(fallback.numeric_limit(prefix), Some(100));
            fallback.valid_until = Some(Instant::now() - Duration::from_secs(1));
            assert_eq!(fallback.numeric_limit(prefix), None);
            let wildcard = PlayerPermissionsCache::resolve(
                vec![node("", &format!("{prefix}*"), true)],
                vec![],
                &config,
                now(),
            );
            assert_eq!(wildcard.numeric_limit(prefix), None);
        }
    }
    #[test]
    fn exact_permissions_do_not_match_prefixes_or_panic() {
        let cache = PlayerPermissionsCache::resolve(
            vec![node("", "plots.info", true)],
            vec![],
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("plots.info"), Some(1));
        assert_eq!(cache.get_node_val("plots"), None);
        assert_eq!(cache.get_node_val("plots.info.other"), None);
    }
    #[test]
    fn temporary_memberships_limit_every_inherited_permission() {
        let mut membership = node("", "group.admin", true);
        membership.expiry = 20;
        let mut inheritance = node("admin", "group.engineer", true);
        inheritance.expiry = 17;
        let cache = PlayerPermissionsCache::resolve(
            vec![membership],
            vec![
                node("admin", "commands.stop", true),
                inheritance,
                node("engineer", "worldedit.*", true),
            ],
            &config(),
            10,
        );
        assert_eq!(cache.stored_node_val_at("commands.stop", 19), Some(1));
        assert_eq!(cache.stored_node_val_at("commands.stop", 20), None);
        assert_eq!(
            cache.stored_node_val_at("worldedit.region.set", 16),
            Some(1)
        );
        assert_eq!(cache.stored_node_val_at("worldedit.region.set", 17), None);
    }
    #[test]
    fn permanent_alternate_group_path_survives_temporary_path_expiry() {
        let mut temporary = node("", "group.admin", true);
        temporary.expiry = 20;
        let cache = PlayerPermissionsCache::resolve(
            vec![temporary, node("", "group.engineer", true)],
            vec![
                node("engineer", "group.admin", true),
                node("admin", "group.engineer", true),
                node("admin", "commands.stop", true),
            ],
            &config(),
            10,
        );
        assert_eq!(cache.stored_node_val_at("commands.stop", 21), Some(1));
    }
    #[test]
    fn stale_cache_fails_closed() {
        let mut cache =
            PlayerPermissionsCache::resolve(vec![node("", "*", true)], vec![], &config(), now());
        cache.valid_until = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(cache.get_node_val("commands.stop"), None);
    }
    #[test]
    fn builder_history_denials_do_not_remove_higher_rank_history_grants() {
        let mut config = config();
        config.mchprs_permissions = true;
        let groups = vec![
            node("default", "mchprs.*", false),
            node("default", "mchprs.access.join", true),
            node("builder", "group.default", true),
            node("builder", "mchprs.build.*", true),
            node("builder", "mchprs.commands.rhistory", false),
            node("builder", "mchprs.commands.rhistory.*", false),
            node("builder", "mchprs.commands.rback", false),
            node("builder", "mchprs.history.limit.*", false),
            node("advanced", "group.builder", true),
            node("advanced", "mchprs.commands.rhistory", true),
            node("advanced", "mchprs.commands.rhistory.*", true),
            node("advanced", "mchprs.commands.rback", true),
            node("advanced", "mchprs.history.limit.200", true),
            node("expert", "group.builder", true),
            node("expert", "group.advanced", true),
            node("expert", "mchprs.commands.rhistory", true),
            node("expert", "mchprs.commands.rhistory.*", true),
            node("expert", "mchprs.commands.rback", true),
            node("engineer", "group.builder", true),
            node("engineer", "group.expert", true),
            node("engineer", "mchprs.commands.rhistory", true),
            node("engineer", "mchprs.commands.rhistory.*", true),
            node("engineer", "mchprs.commands.rback", true),
            node("engineer", "mchprs.history.limit.1000", true),
        ];
        for (group, limit) in [
            ("builder", None),
            ("advanced", Some(200)),
            ("expert", Some(200)),
            ("engineer", Some(1000)),
        ] {
            let cache = PlayerPermissionsCache::resolve(
                vec![node("", &format!("group.{group}"), true)],
                groups.clone(),
                &config,
                now(),
            );
            assert_eq!(
                cache.get_node_val("commands.rhistory"),
                Some(i32::from(group != "builder")),
                "{group}"
            );
            assert_eq!(
                cache.get_node_val("commands.rhistory.enable"),
                Some(i32::from(group != "builder")),
                "{group}"
            );
            assert_eq!(
                cache.numeric_limit("mchprs.history.limit."),
                limit,
                "{group}"
            );
            assert_eq!(cache.get_node_val("plots.admin.interact.other"), Some(0));
        }
    }
    #[test]
    fn default_group_and_recursive_inheritance_are_read_without_writes() {
        let groups = vec![
            node("default", "plots.info", true),
            node("builder", "group.default", true),
            node("builder", "worldedit.*", true),
            node("default", "group.builder", true),
        ];
        let cache = PlayerPermissionsCache::resolve(vec![], groups.clone(), &config(), now());
        assert_eq!(cache.get_node_val("plots.info"), Some(1));
        let cache = PlayerPermissionsCache::resolve(
            vec![node("", "group.builder", true)],
            groups,
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("worldedit.region.set"), Some(1));
        assert_eq!(cache.get_node_val("plots.admin"), None);
    }
    #[test]
    fn specific_denials_and_direct_permissions_override_wildcards() {
        let cache = PlayerPermissionsCache::resolve(
            vec![
                node("", "group.admin", true),
                node("", "plots.claim", false),
            ],
            vec![node("admin", "*", true), node("admin", "plots.info", false)],
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("plots.claim"), Some(0));
        assert_eq!(cache.get_node_val("plots.info"), Some(0));
        assert_eq!(cache.get_node_val("plots.admin.interact.other"), Some(1));
    }
    #[test]
    fn expired_wrong_world_and_unknown_contexts_cannot_grant_groups() {
        let mut expired = node("", "group.admin", true);
        expired.expiry = 1;
        let mut wrong = node("", "group.admin", true);
        wrong.world = "other".into();
        let mut context = node("", "group.admin", true);
        context.contexts = "{\"gamemode\":\"creative\"}".into();
        let cache = PlayerPermissionsCache::resolve(
            vec![expired, wrong, context],
            vec![
                node("admin", "*", true),
                node("default", "plots.info", true),
            ],
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("plots.admin"), None);
        assert_eq!(cache.get_node_val("plots.info"), Some(1));
    }
    #[test]
    fn denied_group_edges_and_metadata_do_not_grant_permissions() {
        let cache = PlayerPermissionsCache::resolve(
            vec![node("", "group.builder", true)],
            vec![
                node("builder", "group.admin", false),
                node("builder", "weight.5", true),
                node("admin", "*", true),
            ],
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("plots.admin"), None);
        assert_eq!(cache.get_node_val("weight.5"), None);
    }
    #[test]
    fn legacy_mysql_config_loads_and_invalid_prefix_is_rejected() {
        let config = config();
        assert!(matches!(config.storage, Storage::Mysql));
        assert_eq!(config.port, None);
        assert_eq!(config.table_prefix, "luckperms_");
        let mut config = config;
        config.table_prefix = "luckperms_;DROP TABLE x".into();
        assert!(Database::new(config).is_err());
    }
    #[test]
    fn plotsquared_packs_preserve_denials_and_do_not_grant_admin() {
        let mut config = config();
        config.plotsquared_compat = true;
        let cache = PlayerPermissionsCache::resolve(
            vec![node("", "plots.claim", false)],
            vec![node("default", "plots.permpack.basic", true)],
            &config,
            now(),
        );
        assert_eq!(cache.get_node_val("plots.info"), Some(1));
        assert_eq!(cache.get_node_val("plots.claim"), Some(0));
        assert_eq!(cache.get_node_val("commands.rback"), Some(1));
        assert_eq!(cache.get_node_val("plots.admin.interact.other"), None);
        assert_eq!(cache.get_node_val("plots.admin.rewind.unlimited"), None);
        assert_eq!(cache.get_node_val("worldedit.region.set"), None);
        assert_eq!(cache.get_node_val("commands.stop"), None);
    }
    #[test]
    fn world_contexts_and_direct_denials_override_inherited_grants() {
        let mut scoped = node("builder", "worldedit.*", true);
        scoped.world = "redstoneplots".into();
        scoped.contexts = "{\"world\":[\"redstoneplots\"]}".into();
        let cache = PlayerPermissionsCache::resolve(
            vec![
                node("", "group.builder", true),
                node("", "worldedit.region.set", false),
            ],
            vec![scoped],
            &config(),
            now(),
        );
        assert_eq!(cache.get_node_val("worldedit.clipboard.copy"), Some(1));
        assert_eq!(cache.get_node_val("worldedit.region.set"), Some(0));
    }
    #[test]
    #[ignore = "Requires MCHPRS_LUCKPERMS_TEST_CONFIG pointing to a private read-only config"]
    fn live_postgres_read_only_permissions() {
        #[derive(Deserialize)]
        struct TestConfig {
            luckperms: PermissionsConfig,
        }
        let path = std::env::var("MCHPRS_LUCKPERMS_TEST_CONFIG").unwrap();
        let config: TestConfig = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let db = Database::new(config.luckperms).unwrap();
        let mut conn = db.postgres().unwrap();
        let mode: String = conn
            .query_one("SHOW default_transaction_read_only", &[])
            .unwrap()
            .get(0);
        assert_eq!(mode, "on");
        let prefix = &db.config.table_prefix;
        for table in [
            "user_permissions",
            "group_permissions",
            "players",
            "groups",
            "tracks",
            "actions",
        ] {
            let table = format!("{prefix}{table}");
            let row = conn
                .query_one(
                    "SELECT has_table_privilege(current_user, $1, 'INSERT,UPDATE,DELETE,TRUNCATE')",
                    &[&table],
                )
                .unwrap();
            assert!(!row.get::<_, bool>(0), "Writer privileges on {table}");
        }
        let players = conn
            .query(
                &format!("SELECT uuid, primary_group FROM {prefix}players"),
                &[],
            )
            .unwrap();
        let mut checked = 0;
        for player in players {
            let uuid: String = player.get(0);
            let group: String = player.get(1);
            let (users, groups) = db.read_nodes(&uuid).unwrap();
            let cache = PlayerPermissionsCache::resolve(users, groups, &db.config, now());
            assert_eq!(
                cache.get_node_val("plots.admin.interact.other") == Some(1),
                matches!(group.as_str(), "admin" | "moderator")
            );
            if group == "default" {
                assert_eq!(cache.get_node_val("plots.plot.1"), Some(1));
            }
            if matches!(
                group.as_str(),
                "builder" | "advanced" | "expert" | "engineer"
            ) {
                assert_eq!(cache.get_node_val("worldedit.clipboard.copy"), Some(1));
            }
            checked += 1;
        }
        assert!(checked > 0);
        println!("Verified read-only access and existing ranks for {checked} players");
    }
}
