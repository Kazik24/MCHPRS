pub mod generated;
#[cfg(test)]
mod inbound_tests;
mod outbound;
pub mod packets;
pub mod text;
pub use outbound::SendStats;

use packets::serverbound::ServerBoundPacket;
use packets::{read_packet, PacketDecodeError, PacketEncoder};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use tracing::{debug, warn};

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

/// Finish client prediction only after all authoritative updates for the action
/// have entered the same ordered writer. Drop also covers rejected early returns.
pub struct BlockActionAcknowledgement {
    sender: PlayerPacketSender,
    sequence: i32,
}
impl BlockActionAcknowledgement {
    pub fn new(conn: &PlayerConn, sequence: i32) -> Self {
        Self {
            sender: PlayerPacketSender::new(conn),
            sequence,
        }
    }
}
impl Drop for BlockActionAcknowledgement {
    fn drop(&mut self) {
        use packets::PacketEncoderExt;
        let mut payload = Vec::new();
        payload.write_varint(self.sequence);
        self.sender.send_packet(&PacketEncoder::new(payload, 0x04));
    }
}

#[cfg(test)]
mod acknowledgement_tests {
    use super::*;
    use packets::clientbound::{C3BMultiBlockChangeRecord, CMultiBlockChange, ClientBoundPacket};
    use packets::PacketDecoderExt;
    use std::io::Cursor;
    #[test]
    fn authoritative_blocks_reach_the_wire_before_prediction_acknowledgement() {
        for compressed in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut incoming = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            incoming
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let (outgoing, _) = listener.accept().unwrap();
            let sender = PlayerPacketSender {
                outbound: outbound::Outbound::new(outgoing),
                compressed: Arc::new(AtomicBool::new(compressed)),
            };
            {
                let _ack = BlockActionAcknowledgement {
                    sender: PlayerPacketSender {
                        outbound: sender.outbound.clone(),
                        compressed: sender.compressed.clone(),
                    },
                    sequence: 42,
                };
                sender.send_block_changes(&CMultiBlockChange {
                    chunk_x: 0,
                    chunk_y: 4,
                    chunk_z: 0,
                    records: vec![C3BMultiBlockChangeRecord {
                        x: 1,
                        y: 1,
                        z: 1,
                        block_id: 1,
                    }],
                });
            }
            let mut ids = Vec::new();
            for _ in 0..2 {
                let length = incoming.read_varint().unwrap();
                let mut frame = Cursor::new(incoming.read_bytes(length as usize).unwrap());
                if compressed {
                    assert_eq!(frame.read_varint().unwrap(), 0);
                }
                ids.push(frame.read_varint().unwrap());
                if ids.len() == 2 {
                    assert_eq!(frame.read_varint().unwrap(), 42);
                }
            }
            let expected = CMultiBlockChange {
                chunk_x: 0,
                chunk_y: 0,
                chunk_z: 0,
                records: vec![],
            }
            .encode();
            // Use the registered block-update ID from its encoder, then the ACK.
            let mut bytes = Vec::new();
            expected.write_uncompressed(&mut bytes).unwrap();
            let mut encoded = Cursor::new(bytes);
            encoded.read_varint().unwrap();
            assert_eq!(ids, [encoded.read_varint().unwrap(), 0x04]);
        }
    }
}

/// The minecraft protocol has these 4 different states.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NetworkState {
    Handshake,
    Status,
    Login,
    LoginAcknowledgement,
    Configuration,
    ConfigurationFinish,
    Play,
}

/// Real socket connections for cross-crate packet regression tests.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub mod test_support {
    use super::*;
    use packets::{DecodeResult, PacketDecoderExt};
    use std::io::{self, Cursor, Read};

    pub struct Connection {
        pub player: PlayerConn,
        pub peer: TcpStream,
        pub incoming: mpsc::SyncSender<Box<dyn ServerBoundPacket>>,
    }

    pub fn connection(compressed: bool) -> io::Result<Connection> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let peer = TcpStream::connect(listener.local_addr()?)?;
        peer.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
        let (stream, _) = listener.accept()?;
        let (incoming, packets) = mpsc::sync_channel(64);
        Ok(Connection {
            player: PlayerConn {
                client: NetworkClient {
                    id: 0,
                    outbound: outbound::Outbound::new(stream),
                    packets,
                    compressed: Arc::new(AtomicBool::new(compressed)),
                },
                alive: true,
            },
            peer,
            incoming,
        })
    }

    pub fn read_frame(
        peer: &mut TcpStream,
        compressed: bool,
    ) -> DecodeResult<(i32, Cursor<Vec<u8>>)> {
        let length = peer.read_varint()?;
        if !(1..=2_097_152).contains(&length) {
            return Err(io::Error::from(io::ErrorKind::InvalidData).into());
        }
        let mut frame = Cursor::new(peer.read_bytes(length as usize)?);
        if compressed {
            let size = frame.read_varint()?;
            if size != 0 {
                if !(256..=2_097_152).contains(&size) {
                    return Err(io::Error::from(io::ErrorKind::InvalidData).into());
                }
                let mut data = Vec::new();
                flate2::read::ZlibDecoder::new(frame)
                    .take(size as u64 + 1)
                    .read_to_end(&mut data)?;
                if data.len() != size as usize {
                    return Err(io::Error::from(io::ErrorKind::InvalidData).into());
                }
                frame = Cursor::new(data);
            }
        }
        let id = frame.read_varint()?;
        Ok((id, frame))
    }

    pub fn handshaking_connection(compressed: bool) -> io::Result<(NetworkServer, TcpStream)> {
        let conn = connection(compressed)?;
        let (_, client_receiver) = mpsc::channel();
        Ok((
            NetworkServer {
                client_receiver,
                handshaking_clients: vec![HandshakingConn {
                    client: conn.player.client,
                    alive: true,
                    username: None,
                    uuid: None,
                    profile_properties: Vec::new(),
                    displayed_skin_parts: 0,
                    forwarding_pending: false,
                    protocol_phase: 0,
                }],
            },
            conn.peer,
        ))
    }
}

#[cfg(test)]
mod connection_cleanup_tests {
    use super::*;
    use std::io::Read;

    fn client(
        id: u32,
    ) -> (
        HandshakingConn,
        mpsc::SyncSender<Box<dyn ServerBoundPacket>>,
        TcpStream,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let (stream, _) = listener.accept().unwrap();
        let (sender, packets) = mpsc::sync_channel(64);
        (
            HandshakingConn {
                client: NetworkClient {
                    id,
                    outbound: outbound::Outbound::new(stream),
                    packets,
                    compressed: Arc::new(AtomicBool::new(false)),
                },
                alive: true,
                username: None,
                uuid: None,
                profile_properties: Vec::new(),
                displayed_skin_parts: 0,
                forwarding_pending: false,
                protocol_phase: 0,
            },
            sender,
            peer,
        )
    }

    #[test]
    fn closed_status_clients_are_pruned_and_idle_writer_sockets_are_released() {
        let (mut closed, closed_sender, mut closed_peer) = client(1);
        let (mut live, live_sender, mut live_peer) = client(2);
        drop(closed_sender); // The reader thread ended after a status connection closed.
        assert!(closed.receive_packets().is_empty());
        assert!(!closed.alive);
        assert!(live.receive_packets().is_empty());
        assert!(live.alive);
        let (_sender, receiver) = mpsc::channel();
        let mut server = NetworkServer {
            client_receiver: receiver,
            handshaking_clients: vec![closed, live],
        };
        server.update();
        assert_eq!(server.handshaking_clients.len(), 1);
        assert_eq!(server.handshaking_clients[0].client.id, 2);
        assert_eq!(closed_peer.read(&mut [0]).unwrap(), 0);
        server.handshaking_clients[0].close_connection();
        server.update();
        assert!(server.handshaking_clients.is_empty());
        assert_eq!(live_peer.read(&mut [0]).unwrap(), 0);
        drop(live_sender);
    }

    #[test]
    fn login_transition_does_not_revive_a_closed_connection() {
        let (mut connection, _sender, _peer) = client(1);
        connection.close_connection();
        let player = PlayerConn::from(connection);
        assert!(!player.alive());
    }
}

pub struct HandshakingConn {
    client: NetworkClient,
    alive: bool,
    pub username: Option<String>,
    pub uuid: Option<u128>,
    pub profile_properties: Vec<packets::clientbound::CPlayerInfoAddPlayerProperty>,
    pub displayed_skin_parts: u8,
    pub forwarding_pending: bool,
    pub protocol_phase: u8,
}

impl HandshakingConn {
    pub fn send_packet(&self, data: &PacketEncoder) {
        self.client.send_packet(data);
    }

    pub fn receive_packets(&mut self) -> Vec<Box<dyn ServerBoundPacket>> {
        self.client.receive_packets(&mut self.alive)
    }

    pub fn set_compressed(&self, compressed: bool) {
        self.client.compressed.store(compressed, Ordering::Relaxed)
    }

    pub fn close_connection(&mut self) {
        self.alive = false;
        self.client.close_connection();
    }
}

impl From<HandshakingConn> for PlayerConn {
    fn from(conn: HandshakingConn) -> Self {
        PlayerConn {
            client: conn.client,
            alive: conn.alive,
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
        let peer = stream.peer_addr().ok();
        loop {
            let packet = match read_packet(&mut stream, &compressed, &mut state) {
                Ok(packet) => packet,
                Err(PacketDecodeError::ConnectionClosed) => {
                    debug!(?peer, ?state, "Client connection closed between packets");
                    let _ = stream.shutdown(Shutdown::Both);
                    return;
                }
                // This will cause the client to disconnect
                Err(error) => {
                    warn!(?peer, ?state, ?error, "Client packet decode failed");
                    let _ = stream.shutdown(Shutdown::Both);
                    return;
                }
            };
            // Only this connection's reader waits when the bounded queue fills.
            // Pausing reads applies TCP backpressure during plot stalls without
            // dropping ordered actions or disconnecting on a temporary burst.
            // Dropping the receiver wakes a reader waiting for queue space.
            if sender.send(packet).is_err() {
                debug!(?peer, ?state, "Client packet receiver closed");
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
        // Dropping closed pre-login/status clients also releases their sender
        // handles, waking idle writer threads so they can exit.
        self.handshaking_clients.retain(|client| client.alive);
        loop {
            match self.client_receiver.try_recv() {
                Ok(client) => self.handshaking_clients.push(HandshakingConn {
                    client,
                    alive: true,
                    username: None,
                    uuid: None,
                    profile_properties: Vec::new(),
                    displayed_skin_parts: 0,
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
