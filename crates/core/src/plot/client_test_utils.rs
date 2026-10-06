//! Decode actual client traffic rather than asserting packet-collection internals.
use mchprs_blocks::BlockPos;
use mchprs_network::packets::PacketDecoderExt;
use mchprs_network::test_support::read_frame;
use std::io::Cursor;
use std::net::TcpStream;

pub(super) fn read_blocks(peer: &mut TcpStream, compressed: bool) -> Vec<(BlockPos, u32)> {
    let (id, mut frame) = read_frame(peer, compressed).unwrap();
    decode_blocks(id, &mut frame)
}

pub(super) fn decode_blocks(id: i32, frame: &mut Cursor<Vec<u8>>) -> Vec<(BlockPos, u32)> {
    let blocks = match id {
        0x08 => vec![(
            BlockPos::from_packed(frame.read_position().unwrap()),
            frame.read_varint().unwrap() as u32,
        )],
        0x4d => {
            let section = frame.read_long().unwrap();
            let origin = BlockPos::new(
                (section >> 42) as i32 * 16,
                (section << 44 >> 44) as i32 * 16,
                (section << 22 >> 42) as i32 * 16,
            );
            let count = frame.read_varint().unwrap();
            (0..count)
                .map(|_| {
                    let record = frame.read_varlong().unwrap() as u64;
                    (
                        origin
                            + BlockPos::new(
                                ((record >> 8) & 15) as i32,
                                (record & 15) as i32,
                                ((record >> 4) & 15) as i32,
                            ),
                        (record >> 12) as u32,
                    )
                })
                .collect()
        }
        _ => panic!("expected block update, got packet {id:#x}"),
    };
    assert_eq!(frame.position() as usize, frame.get_ref().len());
    blocks
}

pub(super) fn read_ack(peer: &mut TcpStream, compressed: bool, sequence: i32) {
    let (id, mut frame) = read_frame(peer, compressed).unwrap();
    assert_eq!(id, 0x04);
    assert_eq!(frame.read_varint().unwrap(), sequence);
    assert_eq!(frame.position() as usize, frame.get_ref().len());
}
