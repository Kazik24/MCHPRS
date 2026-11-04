use super::*;
use crate::redbox::{Incoming, SpeedMode};
use mchprs_network::packets::serverbound::SConfigurationFinished;

#[test]
fn redbox_gamemode_and_speed_during_login_reach_the_first_player_state() {
    for (mode, expected_flying) in [(0, false), (1, false), (2, false), (3, true)] {
        let (network, _peer) = mchprs_network::test_support::handshaking_connection(false).unwrap();
        let (plot_sender, receiver) = mpsc::channel();
        let uuid = rand::random();
        let mut server = MinecraftServer {
            waveform_api: None,
            network,
            broadcaster: Bus::new(4),
            receiver,
            plot_sender,
            online_players: Default::default(),
            running_plots: Vec::new(),
            whitelist: None,
            pending_redbox: FxHashMap::from_iter([(
                uuid,
                vec![
                    Incoming::Gamemode { uuid, mode },
                    Incoming::Speed {
                        uuid,
                        mode: SpeedMode::Auto,
                        value: 3.0,
                    },
                ],
            )]),
        };
        let client = &mut server.network.handshaking_clients[0];
        client.username = Some("RedBoxTest".into());
        client.uuid = Some(uuid);
        client.protocol_phase = 3;
        server.handle_configuration_finished(SConfigurationFinished, 0);
        let Message::PlayerJoined(player) = server.receiver.recv_timeout(Duration::from_secs(1)).unwrap() else {
            panic!("configuration did not produce a player");
        };
        assert_eq!(player.gamemode.get_id(), mode);
        assert_eq!(player.flying, expected_flying);
        assert_eq!(player.fly_speed, if expected_flying { 3.0 } else { 1.0 });
        assert_eq!(player.walk_speed, if expected_flying { 1.0 } else { 3.0 });
        assert!(server.pending_redbox.is_empty());
    }
}
