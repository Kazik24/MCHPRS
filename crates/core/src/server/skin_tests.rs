use super::*;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::packets::serverbound::{
    SClientSettings, SConfigurationFinished, ServerBoundPacket,
};
use std::io::Cursor;

#[test]
fn configuration_skin_settings_reach_the_player_before_the_first_spawn() {
    for mask in [0, 0x40, 0x55, 0x7f, 0xff] {
        let (network, _peer) = mchprs_network::test_support::handshaking_connection(false).unwrap();
        let (plot_sender, receiver) = mpsc::channel();
        let mut server = MinecraftServer {
            network,
            broadcaster: Bus::new(4),
            receiver,
            plot_sender,
            online_players: Default::default(),
            running_plots: Vec::new(),
            whitelist: None,
        };
        let client = &mut server.network.handshaking_clients[0];
        client.username = Some("SkinTest".into());
        client.uuid = Some(rand::random());
        client.protocol_phase = 3;

        let mut settings = Vec::new();
        settings.write_string(16, "en_us");
        settings.write_byte(8);
        settings.write_varint(0);
        settings.write_bool(true);
        settings.write_unsigned_byte(mask);
        settings.write_varint(1);
        settings.write_bool(false);
        settings.write_bool(true);
        let settings = SClientSettings::decode(&mut Cursor::new(settings)).unwrap();
        Box::new(settings).handle(&mut server, 0);
        server.handle_configuration_finished(SConfigurationFinished, 0);
        let Message::PlayerJoined(player) = server
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
        else {
            panic!("configuration did not produce a player");
        };
        assert_eq!(player.skin_parts.bits(), u32::from(mask & 0x7f));
        for for_self in [false, true] {
            let mut metadata = Cursor::new(player.entity_metadata_packet(for_self).buffer);
            assert_eq!(metadata.read_varint().unwrap(), player.entity_id as i32);
            for index in [0, 6, 17] {
                assert_eq!(metadata.read_unsigned_byte().unwrap(), index);
                assert_eq!(
                    metadata.read_varint().unwrap(),
                    if index == 6 { 21 } else { 0 }
                );
                assert_eq!(
                    metadata.read_unsigned_byte().unwrap(),
                    if index == 17 { mask & 0x7f } else { 0 }
                );
            }
            assert_eq!(metadata.read_unsigned_byte().unwrap(), 0xff);
        }
    }
}
