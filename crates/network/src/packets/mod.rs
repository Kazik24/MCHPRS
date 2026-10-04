pub mod clientbound;
pub mod components;
pub mod serverbound;

use super::NetworkState;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use flate2::bufread::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::Serialize;
use serverbound::*;
use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct SlotData {
    pub item_id: i32,
    pub item_count: i8,
    pub nbt: Option<nbt::Blob>,
}

#[derive(Debug)]
pub struct PalettedContainer {
    pub bits_per_entry: u8,
    pub palette: Option<Vec<i32>>,
    pub data_array: Vec<u64>,
}

pub type DecodeResult<T> = std::result::Result<T, PacketDecodeError>;

#[derive(Debug)]
pub enum PacketDecodeError {
    Io(io::Error),
    FromUtf8(std::string::FromUtf8Error),
    Nbt(nbt::Error),
}

impl From<nbt::Error> for PacketDecodeError {
    fn from(err: nbt::Error) -> PacketDecodeError {
        PacketDecodeError::Nbt(err)
    }
}

impl From<io::Error> for PacketDecodeError {
    fn from(err: io::Error) -> PacketDecodeError {
        PacketDecodeError::Io(err)
    }
}

impl From<std::string::FromUtf8Error> for PacketDecodeError {
    fn from(err: std::string::FromUtf8Error) -> PacketDecodeError {
        PacketDecodeError::FromUtf8(err)
    }
}

#[derive(Debug)]
pub enum PacketEncodeError {}

fn read_compressed<T: PacketDecoderExt>(
    reader: &mut T,
    network_state: &mut NetworkState,
) -> DecodeResult<Box<dyn ServerBoundPacket>> {
    let length = reader.read_varint()?;
    if !(0..=2097152).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid compressed packet length",
        )
        .into());
    }
    let data = PacketDecoderExt::read_to_end(reader)?;
    if length == 0 {
        read_decompressed(&mut Cursor::new(data), network_state)
    } else {
        if length < 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "compressed packet below threshold",
            )
            .into());
        }
        let mut decompressed = Vec::new();
        Read::take(ZlibDecoder::new(data.as_slice()), 2097153).read_to_end(&mut decompressed)?;
        if decompressed.len() != length as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "compressed packet length mismatch",
            )
            .into());
        }
        read_decompressed(&mut Cursor::new(decompressed), network_state)
    }
}

fn read_decompressed<T: PacketDecoderExt>(
    reader: &mut T,
    state: &mut NetworkState,
) -> DecodeResult<Box<dyn ServerBoundPacket>> {
    let packet_id = reader.read_varint()?;
    let unknown = || -> Box<dyn ServerBoundPacket> { Box::new(SUnknown) };
    Ok(match *state {
        NetworkState::Handshake if packet_id == 0 => {
            let p = SHandshake::decode(reader)?;
            *state = match p.next_state {
                1 => NetworkState::Status,
                2 => NetworkState::Login,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid handshake state",
                    )
                    .into())
                }
            };
            Box::new(p)
        }
        NetworkState::Status => match packet_id {
            0 => Box::new(SRequest::decode(reader)?),
            1 => Box::new(SPing::decode(reader)?),
            _ => unknown(),
        },
        NetworkState::Login if packet_id == 0 => {
            let p = SLoginStart::decode(reader)?;
            *state = NetworkState::LoginAcknowledgement;
            Box::new(p)
        }
        NetworkState::LoginAcknowledgement if packet_id == 3 => {
            *state = NetworkState::Configuration;
            Box::new(SLoginAcknowledged)
        }
        NetworkState::Configuration | NetworkState::ConfigurationFinish => match packet_id {
            0 => Box::new(SClientSettings::decode(reader)?),
            2 => Box::new(SPluginMessage::decode(reader)?),
            7 if *state == NetworkState::Configuration => {
                let count = reader.read_varint()?;
                if count != 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unexpected known packs",
                    )
                    .into());
                }
                *state = NetworkState::ConfigurationFinish;
                Box::new(SKnownPacks)
            }
            3 if *state == NetworkState::ConfigurationFinish => {
                *state = NetworkState::Play;
                Box::new(SConfigurationFinished)
            }
            4 => Box::new(SKeepAlive::decode(reader)?),
            _ => unknown(),
        },
        NetworkState::Play => match packet_id {
            0x05 => {
                let message = format!("/{}", reader.read_string()?);
                Box::new(SChatMessage { message })
            }
            0x06 => Box::new(SChatCommand::decode(reader)?),
            0x07 => Box::new(SChatMessage::decode(reader)?),
            0x0c => Box::new(SClientSettings::decode(reader)?),
            0x0d => Box::new(STabComplete::decode(reader)?),
            0x14 => Box::new(SPluginMessage::decode(reader)?),
            0x1a => Box::new(SKeepAlive::decode(reader)?),
            0x1c => Box::new(SPlayerPosition::decode(reader)?),
            0x1d => Box::new(SPlayerPositionAndRotation::decode(reader)?),
            0x1e => Box::new(SPlayerRotation::decode(reader)?),
            0x1f => Box::new(SPlayerMovement::decode(reader)?),
            0x22 => Box::new(SPickItemFromBlock::decode(reader)?),
            0x23 => Box::new(SPickItemFromEntity::decode(reader)?),
            0x26 => Box::new(SPlayerAbilities::decode(reader)?),
            0x27 => Box::new(SPlayerDigging::decode(reader)?),
            0x28 => Box::new(SEntityAction::decode(reader)?),
            0x33 => Box::new(SHeldItemChange::decode(reader)?),
            0x36 => Box::new(SCreativeInventoryAction::decode(reader)?),
            0x3a => Box::new(SUpdateSign::decode(reader)?),
            0x3b => Box::new(SAnimation::decode(reader)?),
            0x3e => Box::new(SPlayerBlockPlacemnt::decode(reader)?),
            _ => unknown(),
        },
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packet in invalid connection state",
            )
            .into())
        }
    })
}

pub fn read_packet<T: PacketDecoderExt>(
    reader: &mut T,
    compressed: &Arc<AtomicBool>,
    network_state: &mut NetworkState,
) -> DecodeResult<Box<dyn ServerBoundPacket>> {
    let length = reader.read_varint()?;
    if !(1..=2097152).contains(&length) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid packet length").into());
    }
    let data = reader.read_bytes(length as usize)?;
    let mut cursor = Cursor::new(data);
    if compressed.load(Ordering::Relaxed) {
        read_compressed(&mut cursor, network_state)
    } else {
        read_decompressed(&mut cursor, network_state)
    }
}

impl<T: std::convert::AsRef<[u8]>> PacketDecoderExt for Cursor<T> {}
impl PacketDecoderExt for TcpStream {}

pub trait PacketDecoderExt: Read + Sized {
    fn read_unsigned_byte(&mut self) -> DecodeResult<u8> {
        Ok(self.read_u8()?)
    }

    fn read_byte(&mut self) -> DecodeResult<i8> {
        Ok(self.read_i8()?)
    }

    fn read_bytes(&mut self, bytes: usize) -> DecodeResult<Vec<u8>> {
        let mut read = vec![0; bytes];
        self.read_exact(&mut read)?;
        Ok(read)
    }

    fn read_uuid(&mut self) -> DecodeResult<u128> {
        Ok(self.read_u128::<BigEndian>()?)
    }

    fn read_long(&mut self) -> DecodeResult<i64> {
        Ok(self.read_i64::<BigEndian>()?)
    }

    fn read_int(&mut self) -> DecodeResult<i32> {
        Ok(self.read_i32::<BigEndian>()?)
    }

    fn read_short(&mut self) -> DecodeResult<i16> {
        Ok(self.read_i16::<BigEndian>()?)
    }

    fn read_unsigned_short(&mut self) -> DecodeResult<u16> {
        Ok(self.read_u16::<BigEndian>()?)
    }

    fn read_double(&mut self) -> DecodeResult<f64> {
        Ok(self.read_f64::<BigEndian>()?)
    }

    fn read_float(&mut self) -> DecodeResult<f32> {
        Ok(self.read_f32::<BigEndian>()?)
    }

    fn read_bool(&mut self) -> DecodeResult<bool> {
        Ok(self.read_u8()? == 1)
    }

    fn read_varint(&mut self) -> DecodeResult<i32> {
        let mut result = 0u32;
        for n in 0..5 {
            let byte = self.read_unsigned_byte()?;
            if n == 4 && byte & 0xf0 != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too big").into());
            }
            result |= ((byte & 127) as u32) << (7 * n);
            if byte & 128 == 0 {
                return Ok(result as i32);
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "VarInt too big").into())
    }
    fn read_varlong(&mut self) -> DecodeResult<i64> {
        let mut result = 0u64;
        for n in 0..10 {
            let byte = self.read_unsigned_byte()?;
            if n == 9 && byte & 0xfe != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "VarLong too big").into());
            }
            result |= ((byte & 127) as u64) << (7 * n);
            if byte & 128 == 0 {
                return Ok(result as i64);
            }
        }
        Err(io::Error::new(io::ErrorKind::InvalidData, "VarLong too big").into())
    }
    fn read_string(&mut self) -> DecodeResult<String> {
        let length = self.read_varint()?;
        if !(0..=131068).contains(&length) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid string length").into());
        }
        Ok(String::from_utf8(self.read_bytes(length as usize)?)?)
    }

    fn read_to_end(&mut self) -> DecodeResult<Vec<u8>> {
        let mut data = Vec::new();
        let _ = Read::read_to_end(self, &mut data);
        Ok(data)
    }

    fn read_position(&mut self) -> DecodeResult<PackedPos> {
        let val: i64 = self.read_long()?;
        Ok(PackedPos::from_raw(val))
    }

    fn read_nbt_blob(&mut self) -> DecodeResult<Option<nbt::Blob>> {
        match nbt::Blob::from_reader(self) {
            Ok(nbt) => Ok(Some(nbt)),
            Err(nbt::Error::NoRootCompound) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct PackedPos {
    val: i64,
}

impl PackedPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        let val =
            ((x as i64 & 0x3FF_FFFF) << 38) | ((z as i64 & 0x3FF_FFFF) << 12) | (y as i64 & 0xFFF);
        Self { val }
    }
    pub const fn coords(self) -> (i32, i32, i32) {
        let val = self.val;
        let x = val >> 38;
        let mut y = val & 0xFFF;
        if y >= 0x800 {
            y -= 0x1000;
        }
        let z = val << 26 >> 38;
        (x as i32, y as i32, z as i32)
    }
    pub const fn as_raw(self) -> i64 {
        self.val
    }
    pub const fn from_raw(val: i64) -> Self {
        Self { val }
    }
}

pub trait PacketEncoderExt: Write + Sized {
    fn write_boolean(&mut self, val: bool) {
        self.write_all(&[val as u8]).unwrap();
    }
    fn write_bytes(&mut self, val: &[u8]) {
        self.write_all(val).unwrap();
    }
    fn write_varint(&mut self, val: i32) {
        let _ = self.write_all(&PacketEncoder::varint(val));
    }

    fn write_varlong(&mut self, val: i64) {
        let mut val = val as u64;
        loop {
            let mut temp = (val & 0b1111_1111) as u8;
            val >>= 7;
            if val != 0 {
                temp |= 0b1000_0000;
            }
            self.write_all(&[temp]).unwrap();
            if val == 0 {
                break;
            }
        }
    }

    fn write_byte(&mut self, val: i8) {
        self.write_all(&[val as u8]).unwrap();
    }

    fn write_unsigned_byte(&mut self, val: u8) {
        self.write_all(&[val]).unwrap();
    }

    fn write_short(&mut self, val: i16) {
        self.write_i16::<BigEndian>(val).unwrap();
    }

    fn write_unsigned_short(&mut self, val: u16) {
        self.write_u16::<BigEndian>(val).unwrap();
    }

    fn write_int(&mut self, val: i32) {
        self.write_i32::<BigEndian>(val).unwrap();
    }

    fn write_double(&mut self, val: f64) {
        self.write_f64::<BigEndian>(val).unwrap();
    }

    fn write_float(&mut self, val: f32) {
        self.write_f32::<BigEndian>(val).unwrap();
    }

    fn write_string(&mut self, n: usize, val: &str) {
        if val.len() > n * 4 + 3 {
            panic!("Tried to write string longer than the max length!");
        }
        self.write_varint(val.len() as i32);
        self.write_all(val.as_bytes()).unwrap();
    }

    fn write_uuid(&mut self, val: u128) {
        self.write_u128::<BigEndian>(val).unwrap();
    }

    fn write_long(&mut self, val: i64) {
        self.write_i64::<BigEndian>(val).unwrap();
    }

    fn write_position(&mut self, pos: PackedPos) {
        self.write_long(pos.as_raw());
    }
    fn write_bool(&mut self, val: bool) {
        self.write_u8(val as u8).unwrap();
    }

    fn write_nbt<T: Serialize>(&mut self, value: &T) {
        let mut named = Vec::new();
        nbt::to_writer(&mut named, value, None).unwrap();
        self.write_bytes(&named[..1]);
        self.write_bytes(&named[3..]);
    }
    fn write_nbt_blob(&mut self, blob: &nbt::Blob) {
        let mut named = Vec::new();
        blob.to_writer(&mut named).unwrap();
        let n = u16::from_be_bytes([named[1], named[2]]) as usize;
        self.write_bytes(&named[..1]);
        self.write_bytes(&named[3 + n..]);
    }
    fn write_text(&mut self, text: &str) {
        let value = crate::text::from_json(text);
        if let nbt::Value::Compound(c) = value {
            self.write_nbt_blob(&nbt::Blob::with_content(c));
        } else if let nbt::Value::String(s) = value {
            self.write_bytes(&[8]);
            self.write_unsigned_short(s.len() as u16);
            self.write_bytes(s.as_bytes());
        }
    }
    fn write_slot_data(&mut self, slot: &Option<SlotData>) {
        components::write_slot(self, slot);
    }
}
impl PacketEncoderExt for Vec<u8> {}
pub struct PacketEncoder {
    pub buffer: Vec<u8>,
    pub packet_id: u32,
}
impl PacketEncoder {
    pub fn new(buffer: Vec<u8>, packet_id: u32) -> PacketEncoder {
        PacketEncoder { buffer, packet_id }
    }

    // This function is separate because it is needed when writing packet headers
    fn varint(val: i32) -> Vec<u8> {
        let mut val = val as u32;
        let mut buf = Vec::new();
        loop {
            let mut temp = (val & 0b1111_1111) as u8;
            val >>= 7;
            if val != 0 {
                temp |= 0b1000_0000;
            }
            buf.push(temp);
            if val == 0 {
                return buf;
            }
        }
    }

    pub fn write_compressed(&self, mut w: impl Write) -> io::Result<()> {
        // TODO: zero allocation
        let packet_id = PacketEncoder::varint(self.packet_id as i32);
        let data = [packet_id.as_slice(), self.buffer.as_slice()].concat();
        if data.len() < 256 {
            // Data Length adds another byte
            let packet_length = PacketEncoder::varint((1 + data.len()) as i32);

            w.write_all(&packet_length)?;
            // Data Length: 0 because uncompressed
            w.write_all(&[0])?;
            w.write_all(&data)?;
        } else {
            let data_length = PacketEncoder::varint(data.len() as i32);
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&data)?;
            let compressed = encoder.finish().unwrap();
            let packet_length =
                PacketEncoder::varint((data_length.len() + compressed.len()) as i32);

            w.write_all(&packet_length)?;
            w.write_all(&data_length)?;
            w.write_all(&compressed)?;
        }

        // self.c_cache = Some(finished);
        // return self.c_cache.as_ref().unwrap();

        Ok(())
    }

    pub fn write_uncompressed(&self, mut w: impl Write) -> io::Result<()> {
        // if let Some(data) = &self.unc_cache {
        //     return &data;
        // }

        let packet_id = PacketEncoder::varint(self.packet_id as i32);
        let length = PacketEncoder::varint((self.buffer.len() + packet_id.len()) as i32);

        // https://github.com/rust-lang/rust/issues/70436
        w.write_all(&length)?;
        w.write_all(&packet_id)?;
        w.write_all(&self.buffer)?;

        // self.unc_cache = Some([&length[..], &packet_id[..], &self.buffer[..]].concat());
        // return self.c_cache.as_ref().unwrap();

        Ok(())
    }
}

#[cfg(test)]
mod tests;
