use super::*;

#[test]
fn native_chat_controls_public_output_without_losing_player_list_removal() {
    let (network, _peer) = mchprs_network::test_support::handshaking_connection(false).unwrap();
    let (plot_sender, receiver) = mpsc::channel();
    let mut broadcaster = Bus::new(8);
    let mut messages = broadcaster.add_rx();
    let mut server = MinecraftServer {
        network,
        broadcaster,
        receiver,
        plot_sender,
        online_players: Default::default(),
        running_plots: Vec::new(),
        whitelist: None,
    };
    server.online_players.insert(
        1,
        PlayerListEntry {
            plot_x: 0,
            plot_z: 0,
            username: "ChatTest".into(),
            gamemode: Gamemode::Creative,
            properties: Vec::new(),
            chat_prefix: Some("Rank".into()),
        },
    );
    server.handle_message(Message::ChatInfo(1, "ChatTest".into(), "hello".into()));
    if CONFIG.native_chat {
        assert!(matches!(
            messages.try_recv().unwrap(),
            BroadcastMessage::Chat(1, _)
        ));
    } else {
        assert!(messages.try_recv().is_err());
    }
    server.handle_message(Message::PlayerLeft(1));
    if CONFIG.native_chat {
        assert!(matches!(
            messages.try_recv().unwrap(),
            BroadcastMessage::Chat(0, _)
        ));
    }
    assert!(matches!(
        messages.try_recv().unwrap(),
        BroadcastMessage::PlayerLeft(1)
    ));
    assert!(messages.try_recv().is_err());
    assert!(server.online_players.is_empty());
}
