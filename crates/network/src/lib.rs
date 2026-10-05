pub mod generated;
mod outbound;
pub mod packets;
pub mod text;
pub use outbound::SendStats;

use packets::serverbound::ServerBoundPacket;
use packets::{read_packet, PacketEncoder};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use tracing::warn;

#[derive(Debug)]
pub struct PlayerPacketSender {
    // todo add synced player position, so that block_actions, and chunk updates can be sent more locally to
    // the player reducing lag spikes while moving around very large redstone contraptions
    // player_pos: Arc<AtomicI64>, // raw PackedPos converted with as_raw()/from_raw()
    outbound: outbound::Outbound,
    compressed: Arc<AtomicBool>,
}

impl PlayerPacketSender {
    pub fn new(conn: &PlayerConn) -> PlayerPacketSender {
        PlayerPacketSender {
            outbound: conn.client.outbound.clone(),
            compressed: conn.client.compressed.clone(),
        }
    }

    pub fn send_packet(&self, data: &PacketEncoder) {
        self.outbound
            .packet(data, self.compressed.load(Ordering::Relaxed));
    }

    pub fn send_block_changes(&self, data: &packets::clientbound::CMultiBlockChange) {
        self.outbound
            .blocks(data, self.compressed.load(Ordering::Relaxed));
    }

    pub fn send_stats(&self) -> SendStats {
        self.outbound.stats()
    }
}

/// The minecraft protocol has these 4 different states.
#[derive(PartialEq, Eq, Clone)]
pub enum NetworkState {
    Handshake,
    Status,
    Login,
    LoginAcknowledgement,
    Configuration,
    ConfigurationFinish,
    Play,
}

pub struct HandshakingConn {
    client: NetworkClient,
    pub username: Option<String>,
    pub uuid: Option<u128>,
    pub profile_properties: Vec<packets::clientbound::CPlayerInfoAddPlayerProperty>,
    pub forwarding_pending: bool,
    pub protocol_phase: u8,
}

impl HandshakingConn {
    pub fn send_packet(&self, data: &PacketEncoder) {
        self.client.send_packet(data);
    }

    pub fn receive_packets(&self) -> Vec<Box<dyn ServerBoundPacket>> {
        self.client.receive_packets(&mut true)
    }

    pub fn set_compressed(&self, compressed: bool) {
        self.client.compressed.store(compressed, Ordering::Relaxed)
    }

    pub fn close_connection(&self) {
        self.client.close_connection();
    }
}

impl From<HandshakingConn> for PlayerConn {
    fn from(conn: HandshakingConn) -> Self {
        PlayerConn {
            client: conn.client,
            alive: true,
        }
    }
}

pub struct PlayerConn {
    client: NetworkClient,
    alive: bool,
}

impl PlayerConn {
    pub fn send_packet(&self, data: &PacketEncoder) {
        self.client.send_packet(data);
    }

    pub fn receive_packets(&mut self) -> Vec<Box<dyn ServerBoundPacket>> {
        self.client.receive_packets(&mut self.alive)
    }

    pub fn alive(&self) -> bool {
        self.alive
    }

    pub fn close_connection(&mut self) {
        self.alive = false;
        self.client.close_connection();
    }
}

/// This handles the TCP stream.
pub struct NetworkClient {
    /// All NetworkClients are identified by this id.
    /// If the client is a player, the player's entitiy id becomes the same.
    pub id: u32,
    outbound: outbound::Outbound,
    packets: mpsc::Receiver<Box<dyn ServerBoundPacket>>,
    compressed: Arc<AtomicBool>,
}

impl NetworkClient {
    fn listen(
        mut stream: TcpStream,
        sender: mpsc::SyncSender<Box<dyn ServerBoundPacket>>,
        compressed: Arc<AtomicBool>,
    ) {
        let mut state = NetworkState::Handshake;
        loop {
            let packet = match read_packet(&mut stream, &compressed, &mut state) {
                Ok(packet) => packet,
                // This will cause the client to disconnect
                Err(error) => {
                    warn!("Client packet decode failed: {:?}", error);
                    let _ = stream.shutdown(Shutdown::Both);
                    return;
                }
            };
            // Disconnect a flooding client instead of growing an unbounded queue.
            if sender.try_send(packet).is_err() {
                let _ = stream.shutdown(Shutdown::Both);
                return;
            }
        }
    }

    pub fn receive_packets(&self, alive: &mut bool) -> Vec<Box<dyn ServerBoundPacket>> {
        let mut packets = Vec::new();
        for _ in 0..128 {
            let packet = self.packets.try_recv();
            match packet {
                Ok(packet) => packets.push(packet),
                Err(mpsc::TryRecvError::Empty) => break,
                _ => {
                    *alive = false;
                    break;
                }
            }
        }
        packets
    }

    pub fn send_packet(&self, data: &PacketEncoder) {
        self.outbound
            .packet(data, self.compressed.load(Ordering::Relaxed));
    }

    pub fn close_connection(&self) {
        self.outbound.close();
    }
}

/// This represents the network portion of a minecraft server
pub struct NetworkServer {
    client_receiver: mpsc::Receiver<NetworkClient>,
    /// These clients are either in the handshake, login, or ping state, once they shift to play, they will be moved to a plot
    pub handshaking_clients: Vec<HandshakingConn>,
}

impl NetworkServer {
    fn listen(bind_address: &str, sender: mpsc::Sender<NetworkClient>) {
        let listener = TcpListener::bind(bind_address).unwrap();

        for (index, stream) in listener.incoming().enumerate() {
            let stream = stream.unwrap();
            let (packet_sender, packet_receiver) = mpsc::sync_channel(64);
            let compressed = Arc::new(AtomicBool::new(false));
            let client_stream = stream.try_clone().unwrap();
            let client_compressed = compressed.clone();
            thread::spawn(move || {
                NetworkClient::listen(client_stream, packet_sender, client_compressed);
            });
            sender
                .send(NetworkClient {
                    // The index will increment after each client making it unique. We'll just use this as the enitity id.
                    id: index as u32,
                    outbound: outbound::Outbound::new(stream),
                    packets: packet_receiver,
                    compressed,
                })
                .unwrap();
        }
    }

    /// Creates a new `NetworkServer`. The server will then start accepting TCP clients.
    pub fn new(bind_address: String) -> NetworkServer {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || NetworkServer::listen(&bind_address, sender));
        NetworkServer {
            client_receiver: receiver,
            handshaking_clients: Vec::new(),
        }
    }

    pub fn update(&mut self) {
        loop {
            match self.client_receiver.try_recv() {
                Ok(client) => self.handshaking_clients.push(HandshakingConn {
                    client,
                    username: None,
                    uuid: None,
                    profile_properties: Vec::new(),
                    forwarding_pending: false,
                    protocol_phase: 0,
                }),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("Client receiver channel disconnected!");
                }
            }
        }
    }
}
