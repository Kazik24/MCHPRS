use super::*;
use packets::serverbound::*;
use packets::PacketEncoderExt;
use std::io::{Read, Write};
use std::time::Duration;

struct Reader {
    peer: TcpStream,
    packets: mpsc::Receiver<Box<dyn ServerBoundPacket>>,
    finished: mpsc::Receiver<()>,
    worker: thread::JoinHandle<()>,
}

fn saturated_reader() -> Reader {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    peer.set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (stream, _) = listener.accept().unwrap();
    let (sender, packets) = mpsc::sync_channel(64);
    // Stand in for a plot that has not drained its existing packets yet.
    for id in -64..0 {
        sender.send(Box::new(SKeepAlive { id }) as _).unwrap();
    }
    let (done, finished) = mpsc::channel();
    let worker = thread::spawn(move || {
        NetworkClient::listen(stream, sender, Arc::new(AtomicBool::new(false)));
        done.send(()).unwrap();
    });
    Reader {
        peer,
        packets,
        finished,
        worker,
    }
}

fn play_preamble(bytes: &mut Vec<u8>) {
    let mut handshake = Vec::new();
    handshake.write_varint(770);
    handshake.write_string(255, "localhost");
    handshake.write_unsigned_short(25565);
    handshake.write_varint(2);
    PacketEncoder::new(handshake, 0)
        .write_uncompressed(&mut *bytes)
        .unwrap();
    let mut login = Vec::new();
    login.write_string(16, "queue_test");
    login.write_uuid(1);
    PacketEncoder::new(login, 0)
        .write_uncompressed(&mut *bytes)
        .unwrap();
    // Login acknowledgement, known-packs response, configuration finish.
    for (id, payload) in [(3, vec![]), (7, vec![0]), (3, vec![])] {
        PacketEncoder::new(payload, id)
            .write_uncompressed(&mut *bytes)
            .unwrap();
    }
}

#[test]
fn full_incoming_queue_waits_then_delivers_play_actions_in_order() {
    #[derive(Debug, PartialEq)]
    enum Event {
        KeepAlive(i64),
        Movement(bool),
        HeldItem(i16),
        Digging(i32),
    }
    #[derive(Default)]
    struct Handler(Vec<Event>);
    impl ServerBoundPacketHandler for Handler {
        fn handle_keep_alive(&mut self, packet: SKeepAlive, _: usize) {
            self.0.push(Event::KeepAlive(packet.id));
        }
        fn handle_player_movement(&mut self, packet: SPlayerMovement, _: usize) {
            self.0.push(Event::Movement(packet.on_ground));
        }
        fn handle_held_item_change(&mut self, packet: SHeldItemChange, _: usize) {
            self.0.push(Event::HeldItem(packet.slot));
        }
        fn handle_player_digging(&mut self, packet: SPlayerDigging, _: usize) {
            self.0.push(Event::Digging(packet.sequence));
        }
    }

    let mut reader = saturated_reader();
    let mut bytes = Vec::new();
    play_preamble(&mut bytes);
    let mut expected: Vec<_> = (-64..0).map(Event::KeepAlive).collect();
    // Several queue capacities, mixing movement with reliable ordered actions.
    for id in 0..256 {
        let mut payload = Vec::new();
        let packet_id = match id % 4 {
            0 => {
                payload.write_long(id);
                expected.push(Event::KeepAlive(id));
                0x1a
            }
            1 => {
                payload.write_bool(id % 8 == 1);
                expected.push(Event::Movement(id % 8 == 1));
                0x1f
            }
            2 => {
                payload.write_short((id % 9) as i16);
                expected.push(Event::HeldItem((id % 9) as i16));
                0x33
            }
            _ => {
                payload.write_varint(0); // Start digging.
                payload.write_long(0); // Packed block position.
                payload.write_byte(1); // Top face.
                payload.write_varint(id as i32); // Prediction sequence.
                expected.push(Event::Digging(id as i32));
                0x27
            }
        };
        PacketEncoder::new(payload, packet_id)
            .write_uncompressed(&mut bytes)
            .unwrap();
    }
    reader.peer.write_all(&bytes).unwrap();
    reader.peer.shutdown(Shutdown::Write).unwrap();
    // EOF must not close the reader before the already-decoded packet can queue.
    assert!(matches!(
        reader.finished.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    let mut handler = Handler::default();
    for _ in 0..(64 + 5 + 256) {
        reader
            .packets
            .recv_timeout(Duration::from_secs(3))
            .expect("a queued or backpressured packet was lost")
            .handle(&mut handler, 0);
    }
    assert_eq!(handler.0, expected);
    reader
        .finished
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    reader.worker.join().unwrap();
    assert!(matches!(
        reader.packets.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn dropping_a_full_queue_wakes_its_reader_and_closes_the_socket() {
    let mut reader = saturated_reader();
    let mut bytes = Vec::new();
    play_preamble(&mut bytes);
    reader.peer.write_all(&bytes).unwrap();
    assert!(matches!(
        reader.finished.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(reader.packets);
    reader
        .finished
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    reader.worker.join().unwrap();
    // The TCP peer may report reset because unread preamble bytes were discarded.
    match reader.peer.read(&mut [0]) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
        result => panic!("reader socket was not closed: {result:?}"),
    }
}
