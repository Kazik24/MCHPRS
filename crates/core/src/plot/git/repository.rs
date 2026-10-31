mod database;
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
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && !(name.len() >= 8 && name.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Repository {
    pub fn sidebar_head(&self) -> Result<String> {
        let (head, tip) = self.head()?;
        Ok(if tip.is_some() { head } else { String::new() })
    }

    pub fn head(&self) -> Result<(String, Option<String>)> {
        let active = self.active_reference()?;
        let tip = if let Some(id) = active.strip_prefix('@') {
            Some(id.to_owned())
        } else {
            self.branch_tip(&active)?
        };
        Ok((active, tip))
    }

    pub fn resolve(&self, reference: &str) -> Result<String> {
        if reference == "HEAD" {
            return self.head()?.1.context(messages::GIT_NO_COMMITS_HINT);
        }
        if let Some(tip) = self.branch_tip(reference)? {
            return Ok(tip);
        }
        ensure!(
            (8..=64).contains(&reference.len()) && reference.bytes().all(|b| b.is_ascii_hexdigit()),
            messages::git_unknown_reference(reference)
        );
        let matches = self.commit_matches(reference)?;
        match matches.as_slice() {
            [id] => Ok(id.clone()),
            [] => bail!(messages::git_unknown_commit(reference)),
            _ => bail!(messages::GIT_AMBIGUOUS_COMMIT),
        }
    }

    pub fn load(&self, commit: &str) -> Result<Snapshot> {
        self.load_object(&self.snapshot_id(commit)?)
    }

    pub fn raw_size(&self, commit: &str) -> Result<usize> {
        self.object_raw_size(&self.snapshot_id(commit)?)
    }

    pub fn pending_size(&self) -> Result<usize> {
        let (_, target, before) = self.checkout_record()?;
        // Startup recovery decodes the before snapshot, drops it, then decodes
        // the target. Reserve the larger peak rather than both at once.
        Ok(self.raw_size(&target)?.max(self.object_raw_size(&before)?))
    }

    pub fn commit(&mut self, snapshot: &Snapshot, author: u128, name: &str, message: &str) -> Result<String> {
        ensure!(
            !message.trim().is_empty() && message.chars().count() <= 256,
            messages::GIT_COMMIT_MESSAGE_LIMIT
        );
        let fp = snapshot.fingerprints()?;
        self.transaction(|repo| {
            let (branch, parent) = repo.head()?;
            if let Some(parent) = &parent {
                ensure!(repo.snapshot_id(parent)? != fp.full, messages::GIT_NOTHING_CHANGED);
            }
            repo.store(snapshot, &fp)?;
            let now = chrono::Utc::now().timestamp();
            let id = hex(Sha256::digest(serde_json::to_vec(&(
                1,
                &parent,
                &fp.full,
                author.to_string(),
                name,
                now,
                message,
            ))?));
            repo.insert_commit(database::CommitRecord { id: &id, parent: parent.as_deref(), snapshot: &fp.full, author, name, date: now, message })?;
            if branch.starts_with('@') {
                repo.set_active(&format!("@{id}"))?;
            } else {
                repo.set_branch_tip(&branch, &id)?;
            }
            Ok(messages::git_committed(&id[..8], head_label(&branch), message))
        })
    }

    pub fn branch(&mut self, name: &str, reference: &str) -> Result<String> {
        ensure!(valid_branch(name), messages::GIT_BRANCH_NAME_RULES);
        self.transaction(|repo| {
            let tip = repo.resolve(reference)?;
            ensure!(repo.names()?.len() < 128, messages::GIT_BRANCH_LIMIT);
            ensure!(!repo.names()?.iter().any(|n| n == name), messages::GIT_BRANCH_EXISTS);
            repo.insert_branch(name, &tip)?;
            Ok(messages::git_branch_created(name, &tip[..8]))
        })
    }

    pub fn names(&self) -> Result<Vec<String>> {
        self.branch_names()
    }

    pub fn branches(&self) -> Result<Value> {
        let active = self.head()?.0;
        let mut extra = Vec::new();
        if let Some(id) = active.strip_prefix('@') {
            extra.push(json!({"text": format!("\n{}", messages::git_detached_head(&id[..8])), "color": "gray"}));
        }
        for (name, id) in self.branch_tips()? {
            let current = name == active;
            extra.push(json!({"text": if current { "\n* " } else { "\n  " }, "color": "green", "extra": [
                {"text": name, "color": if current { "green" } else { "white" }},
                {"text": format!(" -> {}", &id[..8]), "color": "yellow"}
            ]}));
        }
        if extra.is_empty() {
            extra.push(json!({"text": format!("\n{}", messages::GIT_NO_BRANCHES_HINT), "color": "gray"}));
        }
        Ok(json!({"text": messages::GIT_BRANCHES_HEADING, "color": "gray", "extra": extra}))
    }

    pub fn status(&self, snapshot: &Snapshot) -> Result<String> {
        let (branch, tip) = self.head()?;
        let usage = self.database_bytes()?;
        let Some(tip) = tip else {
            return Ok(messages::GIT_CREATE_MASTER_HINT.into());
        };
        let saved = self.object_fingerprints(&self.snapshot_id(&tip)?)?;
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

    pub fn comparison(&self, from: &str, to: Option<&str>, working: Option<&Snapshot>) -> Result<Diff> {
        let a = self.resolve(from)?;
        let b = to.map(|reference| self.resolve(reference)).transpose()?;
        let size = self.raw_size(&a)?.saturating_add(match &b {
            Some(b) => self.raw_size(b)?,
            None => usize::try_from(bincode::serialized_size(working.context(messages::GIT_NO_COMPARISON)?)?)?,
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

    pub fn log_count(&self, all: bool, count: usize) -> Result<Value> {
        // ponytail: cap one chat response at 1000 rows; use --page for longer trails.
        let rows = self.history_rows(all, None, 0, count.clamp(1, 1000))?;
        Ok(json!({"text": if rows.is_empty() { messages::GIT_NO_COMMITS } else { messages::GIT_HISTORY_TITLE }, "color": "gray", "extra": rows}))
    }

    pub fn log(&self, all: bool, query: Option<&str>, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), messages::GIT_INVALID_PAGE);
        let mut rows = self.history_rows(all, query, (page - 1) * 10, 11)?;
        let more = rows.len() > 10;
        rows.truncate(10);
        let command = if let Some(query) = query {
            format!("/git search {}--page {{page}} {query}", if all { "--all " } else { "" })
        } else {
            format!("/git log {}--page {{page}}", if all { "--all " } else { "" })
        };
        if page > 1 {
            rows.push(button(messages::GIT_PREVIOUS_PAGE, &command.replace("{page}", &(page - 1).to_string())));
        }
        if more {
            rows.push(button(messages::GIT_NEXT_PAGE, &command.replace("{page}", &(page + 1).to_string())));
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
            let (parent, timestamp, author) = self.commit_history_details(&commit)?;
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
                revisions.push(
                    json!({"text": format!("\n{}  {date}  {author}", &commit[..8]), "color": "yellow", "extra": [
                        {"text": "\n  - ", "color": "red"},
                        {"text": diff::description(Block::from_id(states[0])), "color": "#FF9C9C"},
                        {"text": "\n  + ", "color": "green"},
                        {"text": diff::description(Block::from_id(states[1])), "color": "#9CFF9C"}
                    ]}),
                );
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
        let (name, date, message, parent) = self.commit_details(&id)?;
        Ok(messages::git_commit_details(
            &id[..8],
            name,
            chrono::Utc.timestamp_opt(date, 0).single().map(|d| d.to_rfc3339()).unwrap_or_default(),
            message,
            parent.map(|p| p[..8].to_owned()).unwrap_or_else(|| messages::GIT_ROOT_COMMIT.into()),
        ))
    }

    pub fn recoveries(&self, page: usize) -> Result<Value> {
        ensure!((1..=100_000).contains(&page), messages::GIT_INVALID_PAGE);
        let rows = self.recovery_rows(page)?;
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
            extra.push(button(messages::GIT_PREVIOUS_PAGE, &format!("/git recoveries {}", page - 1)));
        }
        if rows.len() > 10 {
            extra.push(button(messages::GIT_NEXT_PAGE, &format!("/git recoveries {}", page + 1)));
        }
        Ok(serde_json::json!({"text":messages::git_recoveries_heading(page),"extra":extra}))
    }

    pub fn recover_branch(&mut self, recovery: &str, branch: &str, author: u128, name: &str) -> Result<()> {
        ensure!(valid_branch(branch), messages::GIT_INVALID_BRANCH_NAME);
        ensure!(
            (8..=64).contains(&recovery.len()) && recovery.bytes().all(|c| c.is_ascii_hexdigit()),
            messages::GIT_INVALID_RECOVERY_ID
        );
        self.transaction(|repo| {
            ensure!(repo.names()?.len() < 128, messages::GIT_BRANCH_LIMIT);
            let rows = repo.recovery_matches(recovery)?;
            ensure!(rows.len() == 1, messages::GIT_UNKNOWN_RECOVERY_ID);
            let (recovery, snapshot, parent) = &rows[0];
            let now = chrono::Utc::now().timestamp();
            let message = messages::git_recovered_commit(&recovery[..8]);
            let id = hex(Sha256::digest(serde_json::to_vec(&(
                parent,
                snapshot,
                author.to_string(),
                name,
                now,
                &message,
                branch,
            ))?));
            repo.insert_commit(database::CommitRecord { id: &id, parent: Some(parent), snapshot, author, name, date: now, message: &message })?;
            repo.insert_branch(branch, &id)?;
            Ok(())
        })
    }

    pub fn has_pending(&self) -> Result<bool> {
        self.checkout_exists()
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
        ensure!(self.head()?.0 != destination, messages::git_already_on_branch(head_label(&destination)));
        let (snapshot, recovery, reservation) = self.restore(&target, &destination, before, author, name, save)?;
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
        ensure!(!branch.starts_with('@'), messages::GIT_REBASE_NAMED_BRANCH_REQUIRED);
        ensure!(
            self.names()?.iter().any(|name| name == source),
            messages::GIT_REBASE_SOURCE_BRANCH_REQUIRED
        );
        let target = self.resolve(source)?;
        // Restore the source snapshot while keeping the current branch and its
        // tip. A later ordinary commit records the copied working state.
        let (snapshot, recovery, reservation) = self.restore(&target, &branch, before, author, name, save)?;
        Ok((snapshot, messages::git_rebased(&branch, source, recovery), reservation))
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
        let (snapshot, recovery, reservation) = self.restore(&target, &branch, before, author, name, save)?;
        Ok((snapshot, messages::git_restored(&target[..8], recovery), reservation))
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
            let (source, head) = repo.head()?;
            let head = head.context(messages::GIT_NO_COMMITS)?;
            repo.load(target)?; // Validate before creating a recovery or a journal.
            repo.store(before, &fp)?;
            if repo.snapshot_id(&head)? != fp.full {
                let now = chrono::Utc::now().timestamp();
                let id = hex(Sha256::digest(serde_json::to_vec(&(
                    &fp.full,
                    &head,
                    author.to_string(),
                    name,
                    chrono::Utc::now().timestamp_nanos_opt().unwrap(),
                ))?));
                repo.insert_recovery(database::RecoveryRecord { id: &id, parent: &head, snapshot: &fp.full, author, name, date: now, source: &source })?;
                recovery = Some(id);
            }
            repo.insert_checkout(destination, target, &fp.full)?;
            Ok(())
        })?;
        let snapshot = self.finish_or_rollback(before, save)?;
        Ok((
            snapshot,
            recovery.map(|id| messages::git_checkout_recovery_suffix(&id[..8])).unwrap_or_default(),
            reservation,
        ))
    }

    fn finish_or_rollback(&mut self, before: &Snapshot, save: &Path) -> Result<Snapshot> {
        match self.finish_checkout_with_settings(save, Some((before.data.world_send_rate, before.data.piston_animation))) {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                // Restore the exact captured working state, even if target save succeeded.
                before.data.save_to_file(save).context(messages::GIT_ROLLBACK_NEEDS_RECOVERY)?;
                self.transaction(|repo| {
                    repo.clear_checkout()?;
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
        settings: Option<(mchprs_save_data::plot_data::WorldSendRate, mchprs_save_data::plot_data::PistonAnimation)>,
    ) -> Result<Snapshot> {
        let (branch, id, before) = self.checkout_record()?;
        let (send_rate, piston_animation) = match settings {
            Some(settings) => settings,
            None => {
                let previous = self.load_object(&before)?;
                (previous.data.world_send_rate, previous.data.piston_animation)
            }
        };
        let mut target = self.load(&id)?;
        target.data.tps = mchprs_save_data::plot_data::Tps::Limited(0);
        target.data.world_send_rate = send_rate;
        target.data.piston_animation = piston_animation;
        target.data.save_to_file(save)?;
        self.transaction(|repo| {
            repo.set_active(&branch)?;
            repo.clear_checkout()?;
            Ok(())
        })?;
        Ok(target)
    }
}

fn head_label(active: &str) -> &str {
    if active.starts_with('@') { messages::GIT_DETACHED_LABEL } else { active }
}

pub(super) fn button(label: &str, command: &str) -> Value {
    serde_json::json!({"text":format!(" [{label}]"),"color":"aqua","click_event":{"action":"run_command","command":command}})
}
