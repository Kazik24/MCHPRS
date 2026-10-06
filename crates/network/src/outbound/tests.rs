use super::*;
use std::io::Read;
use std::net::TcpListener;

fn queued() -> Outbound {
    Outbound {
        handle: Arc::new(Handle {
            shared: Arc::new(Shared::default()),
        }),
    }
}

fn block(id: u32) -> CMultiBlockChange {
    CMultiBlockChange {
        chunk_x: -3,
        chunk_y: 15,
        chunk_z: 7,
        records: vec![C3BMultiBlockChangeRecord {
            x: 15,
            y: 14,
            z: 13,
            block_id: id,
        }],
    }
}

#[test]
fn backlog_keeps_latest_state_without_crossing_packet_barriers() {
    let sender = queued();
    let barrier = PacketEncoder::new(vec![1, 2, 3], 7);
    sender.packet(&barrier, true); // chunk load or entity/interaction packet
    for id in 0..10_000 {
        sender.blocks(&block(id), true);
    }
    sender.packet(&barrier, true);
    sender.blocks(&block(20_000), true);
    let pending = sender.handle.shared.pending.lock().unwrap();
    assert_eq!(pending.items.len(), 4);
    let Payload::Blocks(blocks) = &pending.items[1].payload else {
        panic!()
    };
    assert_eq!(blocks[&(-3, 15, 7)][&0x0fde], 9_999);
    assert_eq!(pending.items[1].bytes, 96);
    let Payload::Blocks(blocks) = &pending.items[3].payload else {
        panic!()
    };
    assert_eq!(blocks[&(-3, 15, 7)][&0x0fde], 20_000);
    assert_eq!(
        sender
            .handle
            .shared
            .counters
            .coalesced_blocks
            .load(Ordering::Relaxed),
        9_999
    );
}

#[test]
fn chunk_snapshots_and_prediction_acks_preserve_their_place_in_the_wire_stream() {
    use crate::packets::clientbound::{CChunkData, CUnloadChunk};
    use crate::packets::{PacketDecoderExt, PacketEncoderExt};
    use std::io::Cursor;

    for compressed in [false, true] {
        let sender = queued(); // Populate a backlog before starting the writer.
        let unload = CUnloadChunk {
            chunk_x: -3,
            chunk_z: 7,
        }
        .encode();
        let load = CChunkData {
            chunk_x: -3,
            chunk_z: 7,
            heightmaps: nbt::Blob::new(),
            chunk_sections: Vec::new(),
            block_entities: Vec::new(),
        }
        .encode();
        let mut ack_data = Vec::new();
        ack_data.write_varint(42);
        let ack = PacketEncoder::new(ack_data, 0x04);
        sender.packet(&unload, compressed);
        sender.packet(&load, compressed);
        for id in 1..=100 {
            sender.blocks(&block(id), compressed);
        }
        sender.packet(&ack, compressed);
        for id in 101..=200 {
            sender.blocks(&block(id), compressed);
        }
        sender.close();
        let mut actual = Vec::new();
        Outbound::run(&sender.handle.shared, &mut actual);

        let mut expected = Vec::new();
        for packet in [unload, load, block(100).encode(), ack, block(200).encode()] {
            if compressed {
                packet.write_compressed(&mut expected).unwrap();
            } else {
                packet.write_uncompressed(&mut expected).unwrap();
            }
        }
        assert_eq!(actual, expected);
        let mut reader = Cursor::new(actual);
        let mut ids = Vec::new();
        while reader.position() < reader.get_ref().len() as u64 {
            let length = reader.read_varint().unwrap();
            let mut frame = Cursor::new(reader.read_bytes(length as usize).unwrap());
            if compressed {
                let size = frame.read_varint().unwrap();
                if size != 0 {
                    let mut data = Vec::new();
                    flate2::read::ZlibDecoder::new(frame)
                        .read_to_end(&mut data)
                        .unwrap();
                    assert_eq!(data.len(), size as usize);
                    frame = Cursor::new(data);
                }
            }
            ids.push(frame.read_varint().unwrap());
        }
        assert_eq!(ids, [0x21, 0x27, 0x4d, 0x04, 0x4d]);
        assert_eq!(sender.stats().coalesced_blocks, 198);
        assert_eq!(sender.stats().failures, 0);
    }
}

#[test]
fn compression_transition_is_an_ordering_boundary() {
    let sender = queued();
    sender.blocks(&block(1), false);
    sender.blocks(&block(2), true);
    assert_eq!(sender.handle.shared.pending.lock().unwrap().items.len(), 2);
}

#[test]
fn failed_writes_close_the_connection_instead_of_skipping_reliable_packets() {
    struct FailingSink;
    impl Write for FailingSink {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let sender = queued();
    sender.packet(&PacketEncoder::new(vec![0; 400], 0x27), true);
    sender.blocks(&block(1), true);
    sender.packet(&PacketEncoder::new(vec![42], 0x04), true);
    Outbound::run(&sender.handle.shared, FailingSink);
    let stats = sender.stats();
    assert_eq!(
        (
            stats.packets,
            stats.bytes,
            stats.failures,
            stats.queued_bytes
        ),
        (0, 0, 1, 0)
    );
    assert!(sender.handle.shared.pending.lock().unwrap().closed);
    sender.packet(&PacketEncoder::new(vec![43], 0x04), true);
    assert!(sender
        .handle
        .shared
        .pending
        .lock()
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn ordered_writer_matches_protocol_bytes_and_drains_on_close() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (server, _) = listener.accept().unwrap();
    let sender = queued(); // hold worker until all updates have been coalesced
    let login = PacketEncoder::new(vec![1, 2, 3], 7);
    let load = PacketEncoder::new(vec![42; 400], 8);
    sender.packet(&login, false);
    sender.packet(&load, true);
    sender.blocks(&block(1), true);
    sender.blocks(&block(2), true);
    sender.packet(&login, true);
    sender.close();
    let shared = sender.handle.shared.clone();
    let worker = std::thread::spawn(move || {
        Outbound::run(&shared, &server);
        server.shutdown(Shutdown::Both).unwrap();
    });
    let mut actual = Vec::new();
    client.read_to_end(&mut actual).unwrap();
    worker.join().unwrap();
    let mut expected = Vec::new();
    login.write_uncompressed(&mut expected).unwrap();
    load.write_compressed(&mut expected).unwrap();
    block(2).encode().write_compressed(&mut expected).unwrap();
    login.write_compressed(&mut expected).unwrap();
    assert_eq!(actual, expected);
    let stats = sender.stats();
    assert_eq!(stats.packets, 4);
    assert_eq!(stats.bytes, expected.len() as u64);
    assert_eq!(stats.queued_bytes, 0);
    assert_eq!(stats.failures, 0);
}

#[test]
fn dropping_last_handle_closes_shared_writer_and_overflow_closes_connection() {
    let sender = queued();
    let shared = sender.handle.shared.clone();
    let clone = sender.clone();
    drop(sender);
    assert!(!shared.pending.lock().unwrap().closed);
    drop(clone);
    assert!(shared.pending.lock().unwrap().closed);

    let sender = queued();
    let packet = PacketEncoder::new(vec![], 0);
    for _ in 0..=MAX_QUEUED_ITEMS {
        sender.packet(&packet, true);
    }
    let pending = sender.handle.shared.pending.lock().unwrap();
    assert!(pending.closed);
    assert!(pending.items.is_empty());
    assert_eq!(pending.bytes, 0);
    assert_eq!(
        sender
            .handle
            .shared
            .counters
            .failures
            .load(Ordering::Relaxed),
        1
    );
}

#[test]
fn background_writer_accepts_packets_without_waiting_for_client_reads() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let (server, _) = listener.accept().unwrap();
    let sender = Outbound::new(server);
    let packet = PacketEncoder::new(vec![123; 4096], 1);
    for _ in 0..100 {
        sender.packet(&packet, false);
    }
    sender.close();
    let mut bytes = Vec::new();
    client.read_to_end(&mut bytes).unwrap();
    let mut expected = Vec::new();
    for _ in 0..100 {
        packet.write_uncompressed(&mut expected).unwrap();
    }
    assert_eq!(bytes, expected);
}

#[test]
#[ignore = "synthetic sender benchmark; run with --release -- --ignored --nocapture"]
fn compare_no_client_synchronous_and_background_sending() {
    struct SlowSink(Arc<Mutex<Vec<Vec<u8>>>>);
    impl Write for SlowSink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(1));
            self.0.lock().unwrap().push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let frame = |tick: u32| CMultiBlockChange {
        chunk_x: -3,
        chunk_y: 15,
        chunk_z: 7,
        records: (0..256u32)
            .map(|i| C3BMultiBlockChangeRecord {
                x: (i >> 4) as u8,
                y: (i & 15) as u8,
                z: 0,
                block_id: (tick + i) & 15,
            })
            .collect(),
    };
    for mode in ["no_client", "synchronous", "background"] {
        let output = Arc::new(Mutex::new(Vec::new()));
        let sender = queued();
        let worker = if mode == "background" {
            let shared = sender.handle.shared.clone();
            let output = output.clone();
            Some(std::thread::spawn(move || {
                Outbound::run(&shared, SlowSink(output))
            }))
        } else {
            None
        };
        let mut sink = SlowSink(output.clone());
        let mut state = [123u64; 256];
        let started = Instant::now();
        for tick in 1..=10_000u32 {
            // The same small synthetic simulation kernel in all three modes.
            for value in &mut state {
                *value = std::hint::black_box(
                    value
                        .rotate_left(7)
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(u64::from(tick)),
                );
            }
            if tick % 20 != 0 || mode == "no_client" {
                continue;
            }
            let frame = frame(tick);
            if mode == "synchronous" {
                let mut bytes = Vec::new();
                frame.encode().write_compressed(&mut bytes).unwrap();
                sink.write_all(&bytes).unwrap();
            } else {
                sender.blocks(&frame, true);
            }
        }
        let producer = started.elapsed();
        sender.close();
        if let Some(worker) = worker {
            worker.join().unwrap();
        }
        let total = started.elapsed();
        if mode != "no_client" {
            let mut last_frame = Vec::new();
            frame(10_000)
                .encode()
                .write_compressed(&mut last_frame)
                .unwrap();
            assert_eq!(
                output.lock().unwrap().last().unwrap(),
                &last_frame,
                "even with skipped frames, the final visual state must arrive"
            );
        }
        let stats = sender.stats();
        let frames = output.lock().unwrap().len();
        println!("sender mode={mode}: producer={:.6}s drain_total={:.6}s synthetic_tps={:.0} delivered_frames={frames} coalesced_blocks={} encode={:.6}s compression={:.6}s writes={:.6}s",
            producer.as_secs_f64(), total.as_secs_f64(), 10_000.0/producer.as_secs_f64(), stats.coalesced_blocks,
            stats.encode_ns as f64/1e9, stats.compress_ns as f64/1e9, stats.write_ns as f64/1e9);
        std::hint::black_box(state);
    }
}
