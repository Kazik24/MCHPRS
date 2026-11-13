use super::PlotWorld;
use crate::player::{PacketSender, Player};
use crate::utils::HyphenatedUUID;
use mchprs_blocks::{BlockPos, blocks::Block};
use mchprs_network::packets::{PacketEncoder, PacketEncoderExt};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_millis(500); // Ten player ticks, independent of redstone TPS.

#[derive(Clone, Serialize, Deserialize)]
struct Flag {
    username: String,
    modified_at: String,
    position: [i32; 3],
    #[serde(skip)]
    next_kill: Option<Instant>,
    #[serde(skip)]
    dead: bool,
}

struct Watch {
    path: PathBuf,
    flags: HashMap<u128, Flag>,
}

static WATCH: Lazy<Mutex<Watch>> = Lazy::new(|| {
    #[cfg(not(test))]
    let path = PathBuf::from("./world/sign-watch.json");
    #[cfg(test)]
    let path = std::env::temp_dir().join(format!("mroww-sign-watch-{}-{}.json", std::process::id(), rand::random::<u64>()));
    Mutex::new(Watch::load(path).expect("Cannot load persistent watched-sign flags"))
});

fn watched(plot: (i32, i32), pos: BlockPos, block: Block) -> bool {
    plot == (0, 0) && matches!((pos.x, pos.y, pos.z), (255, 79, 260) | (258, 79, 298)) && matches!(block.get_name(), "oak_sign" | "oak_wall_sign")
}

pub(super) fn text_changed(rows: &[String; 4], lines: &[String; 4]) -> bool {
    rows.iter()
        .zip(lines)
        .any(|(row, line)| match serde_json::from_str::<serde_json::Value>(row) {
            Ok(serde_json::Value::String(text)) => text != *line,
            Ok(component) => component != serde_json::json!({ "text": line }),
            Err(_) => row != line,
        })
}

impl Watch {
    fn load(path: PathBuf) -> anyhow::Result<Self> {
        let flags = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, flags })
    }

    fn flag(&mut self, player: &Player, pos: BlockPos) -> anyhow::Result<()> {
        let modified_at = chrono::Utc::now().to_rfc3339();
        let mut flags = self.flags.clone();
        let flag = flags.entry(player.uuid).or_insert_with(|| Flag {
            username: player.username.clone(),
            modified_at: modified_at.clone(),
            position: [pos.x, pos.y, pos.z],
            next_kill: Some(Instant::now() + INTERVAL),
            dead: false,
        });
        flag.username = player.username.clone();
        flag.modified_at = modified_at.clone();
        flag.position = [pos.x, pos.y, pos.z];
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        mchprs_save_data::atomic::write(&self.path, &serde_json::to_vec_pretty(&flags)?)?;
        self.flags = flags;
        tracing::warn!(username = %player.username, uuid = %HyphenatedUUID(player.uuid), %modified_at, %pos,
            "Watched oak sign modified on plot 0,0; persistent kill flag set");
        Ok(())
    }

    fn tick(&mut self, player: &Player, now: Instant) {
        let Some(flag) = self.flags.get_mut(&player.uuid) else { return };
        if flag.next_kill.is_some_and(|next| now < next) {
            return;
        }
        flag.next_kill = Some(now + INTERVAL);
        flag.dead = true;
        health(player, 0.0);
        let mut data = Vec::new();
        data.write_varint(player.entity_id as i32);
        data.write_text(r#"{"text":"You modified a watched oak sign."}"#);
        player.send_packet(&PacketEncoder::new(data, 0x3d));
    }

    fn respawn(&mut self, player: &mut Player) {
        let Some(flag) = self.flags.get_mut(&player.uuid) else { return };
        if !flag.dead {
            return;
        }
        flag.dead = false;
        // Same dimension and world as CJoinGame; retain attributes and entity metadata.
        let mut data = Vec::new();
        data.write_varint(0);
        data.write_string(32767, "mchprs:world");
        data.write_long(0);
        data.write_unsigned_byte(player.gamemode.get_id() as u8);
        data.write_unsigned_byte(255);
        data.write_bool(false);
        data.write_bool(true);
        data.write_bool(false);
        data.write_varint(0);
        data.write_varint(63);
        data.write_unsigned_byte(3);
        player.send_packet(&PacketEncoder::new(data, 0x4b));
        health(player, 20.0);
        player.teleport(player.pos);
    }
}

fn health(player: &Player, value: f32) {
    let mut data = Vec::new();
    data.write_float(value);
    data.write_varint(20);
    data.write_float(5.0);
    player.send_packet(&PacketEncoder::new(data, 0x61));
}

pub(super) fn modified(world: &PlotWorld, player: &Player, pos: BlockPos, block: Block) -> bool {
    if !watched((world.x, world.z), pos, block) {
        return true;
    }
    match WATCH.lock().unwrap().flag(player, pos) {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(username = %player.username, %error, "Cannot persist watched-sign flag; rejecting edit");
            player.send_error_message("Cannot save watched-sign flag; edit rejected.");
            false
        }
    }
}

pub(super) fn tick(players: &[Player]) {
    if players.is_empty() {
        return;
    }
    let now = Instant::now();
    let mut watch = WATCH.lock().unwrap();
    for player in players {
        watch.tick(player, now);
    }
}

pub(super) fn respawn(player: &mut Player) {
    WATCH.lock().unwrap().respawn(player);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::PlayerPos;
    use crate::world::World;
    use mchprs_blocks::block_entities::{BlockEntity, SignBlockEntity};
    use mchprs_network::packets::{
        PacketDecoderExt,
        clientbound::{CKeepAlive, ClientBoundPacket},
        serverbound::*,
    };
    use mchprs_network::test_support::{connection, read_frame};

    #[test]
    fn sign_edits_flag_only_real_changes_at_the_two_watched_oak_signs() {
        for scenario in [
            "front",
            "back",
            "unchanged",
            "unchanged_literal",
            "waxed",
            "reach",
            "permission",
            "spruce",
            "elsewhere",
            "other_plot",
            "break",
        ] {
            let (mut plot, _peer) = super::super::client_sync_tests::fixture(false);
            let uuid = rand::random();
            plot.players[0].uuid = uuid;
            plot.owner = Some(uuid);
            plot.players[0].inventory[36] = None;
            let pos = if scenario == "back" {
                BlockPos::new(258, 79, 298)
            } else {
                BlockPos::new(255, 79, 260)
            };
            let pos = if scenario == "elsewhere" { pos + BlockPos::new(1, 0, 0) } else { pos };
            plot.players[0].pos = PlayerPos::new(pos.x as f64, pos.y as f64, pos.z as f64 + 1.0);
            let name = if scenario == "spruce" { "spruce_sign" } else { "oak_sign" };
            plot.world.set_block(pos, Block::from_name(name).unwrap());
            plot.world.set_block_entity(
                pos,
                BlockEntity::Sign(Box::new(SignBlockEntity {
                    waxed: scenario == "waxed",
                    rows: std::array::from_fn(|_| {
                        if scenario == "unchanged_literal" {
                            r#""unchanged""#
                        } else {
                            r#"{"text":""}"#
                        }
                        .into()
                    }),
                    ..Default::default()
                })),
            );
            if scenario == "reach" {
                plot.players[0].pos.x += 64.0;
            }
            if scenario == "permission" {
                plot.owner = None;
                plot.players[0].deny_test_permissions();
            }
            if scenario == "other_plot" {
                plot.world.x = 1;
            }
            if scenario == "break" {
                plot.handle_player_digging(
                    SPlayerDigging {
                        status: 0,
                        pos: pos.packed(),
                        face: 1,
                        sequence: 1,
                    },
                    0,
                );
                assert_eq!(plot.world.get_block(pos), Block::Air);
            } else {
                plot.handle_update_sign(
                    SUpdateSign {
                        pos: pos.packed(),
                        front: scenario != "back",
                        lines: std::array::from_fn(|_| {
                            match scenario {
                                "unchanged" => "",
                                "unchanged_literal" => "unchanged",
                                _ => "changed",
                            }
                            .into()
                        }),
                    },
                    0,
                );
            }
            let flagged = WATCH.lock().unwrap().flags.remove(&uuid).is_some();
            assert_eq!(flagged, matches!(scenario, "front" | "back" | "break"), "{scenario}");
        }
        assert!(watched((0, 0), BlockPos::new(258, 79, 298), Block::from_name("oak_wall_sign").unwrap()));
    }

    #[test]
    fn persistent_flag_survives_reconnect_and_respawn_without_resetting_ten_tick_cadence() {
        let path = std::env::temp_dir().join(format!("mroww-sign-watch-reload-{}.json", rand::random::<u64>()));
        let conn = connection(false).unwrap();
        let mut player = Player::test_player(conn.player);
        player.uuid = u128::MAX;
        let mut watch = Watch::load(path.clone()).unwrap();
        watch.flag(&player, BlockPos::new(255, 79, 260)).unwrap();
        assert!(watch.flags[&player.uuid].next_kill.is_some());
        drop(player);
        drop(watch);

        let (mut plot, mut peer) = super::super::client_sync_tests::fixture(false);
        plot.players[0].uuid = u128::MAX;
        plot.players[0].username = "RenamedPlayer".into();
        let mut watch = Watch::load(path.clone()).unwrap();
        let flag = &watch.flags[&u128::MAX];
        assert_eq!(flag.username, "SyncTest");
        assert_eq!(flag.position, [255, 79, 260]);
        chrono::DateTime::parse_from_rfc3339(&flag.modified_at).unwrap();
        let now = Instant::now();
        watch.tick(&plot.players[0], now);
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x61);
        assert_eq!(frame.read_float().unwrap(), 0.0);
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x3d);

        // Decode a real respawn request through the network dispatcher and plot handler.
        let flag = watch.flags.remove(&u128::MAX).unwrap();
        WATCH.lock().unwrap().flags.insert(u128::MAX, flag);
        let packet = mchprs_network::packets::read_packet(
            &mut std::io::Cursor::new(vec![2, 0x0a, 0]),
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            &mut mchprs_network::NetworkState::Play,
        )
        .unwrap();
        packet.handle(&mut plot, 0);
        watch.flags.insert(u128::MAX, WATCH.lock().unwrap().flags.remove(&u128::MAX).unwrap());
        assert!(!watch.flags[&u128::MAX].dead);
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x4b);
        let (id, mut frame) = read_frame(&mut peer, false).unwrap();
        assert_eq!(id, 0x61);
        assert_eq!(frame.read_float().unwrap(), 20.0);
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x41);

        watch.tick(&plot.players[0], now + INTERVAL - Duration::from_millis(1));
        plot.players[0].send_packet(&CKeepAlive { id: 10 }.encode());
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x26, "must not kill before ten ticks");
        watch.tick(&plot.players[0], now + INTERVAL);
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x61);
        assert_eq!(read_frame(&mut peer, false).unwrap().0, 0x3d);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_persistence_does_not_set_a_flag() {
        let path = std::env::temp_dir().join(format!("mroww-sign-watch-blocked-{}.json", rand::random::<u64>()));
        fs::write(&path, b"unchanged").unwrap();
        let conn = connection(false).unwrap();
        let player = Player::test_player(conn.player);
        let mut watch = Watch {
            path: path.join("flags.json"),
            flags: HashMap::new(),
        };
        assert!(watch.flag(&player, BlockPos::new(255, 79, 260)).is_err());
        assert!(watch.flags.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"unchanged");
        assert!(Watch::load(path.clone()).is_err(), "corruption must not silently reset flags");
        fs::remove_file(path).unwrap();
    }
}
