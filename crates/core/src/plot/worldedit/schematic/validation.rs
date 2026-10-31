use super::*;

fn skip_nbt_bytes(reader: &mut Cursor<&[u8]>, len: u64) -> Result<()> {
    anyhow::ensure!(
        reader.position().checked_add(len).is_some_and(|end| end <= reader.get_ref().len() as u64),
        "truncated NBT payload"
    );
    reader.set_position(reader.position() + len);
    Ok(())
}

fn skip_nbt_string(reader: &mut Cursor<&[u8]>) -> Result<()> {
    let mut len = [0; 2];
    reader.read_exact(&mut len)?;
    skip_nbt_bytes(reader, u16::from_be_bytes(len) as u64)
}

fn preflight_nbt_payload(reader: &mut Cursor<&[u8]>, tag: u8, depth: usize, tags: &mut usize) -> Result<()> {
    use anyhow::{bail, ensure};
    ensure!(depth <= MAX_NBT_DEPTH, "NBT nesting limit exceeded");
    match tag {
        1 => skip_nbt_bytes(reader, 1)?,
        2 => skip_nbt_bytes(reader, 2)?,
        3 | 5 => skip_nbt_bytes(reader, 4)?,
        4 | 6 => skip_nbt_bytes(reader, 8)?,
        7 | 11 | 12 => {
            let mut len = [0; 4];
            reader.read_exact(&mut len)?;
            let count = i32::from_be_bytes(len);
            let width = match tag {
                7 => 1u64,
                11 => 4,
                _ => 8,
            };
            ensure!(count >= 0 && count as u64 <= MAX_NBT_BYTES as u64 / width, "invalid NBT array length");
            let bytes = (count as u64).checked_mul(width).context("NBT array length overflow")?;
            skip_nbt_bytes(reader, bytes)?;
        }
        8 => skip_nbt_string(reader)?,
        9 => {
            let mut head = [0; 5];
            reader.read_exact(&mut head)?;
            let child = head[0];
            let count = i32::from_be_bytes(head[1..].try_into().unwrap());
            ensure!(count >= 0 && count as usize <= MAX_NBT_TAGS, "invalid NBT list length");
            ensure!(child <= 12 && (child != 0 || count == 0), "invalid NBT list type");
            for _ in 0..count {
                *tags += 1;
                ensure!(*tags <= MAX_NBT_TAGS, "NBT tag count limit exceeded");
                preflight_nbt_payload(reader, child, depth + 1, tags)?;
            }
        }
        10 => loop {
            let mut child = [0];
            reader.read_exact(&mut child)?;
            if child[0] == 0 {
                break;
            }
            ensure!(child[0] <= 12, "invalid NBT tag type");
            skip_nbt_string(reader)?;
            *tags += 1;
            ensure!(*tags <= MAX_NBT_TAGS, "NBT tag count limit exceeded");
            preflight_nbt_payload(reader, child[0], depth + 1, tags)?;
        },
        _ => bail!("invalid NBT tag type"),
    }
    Ok(())
}

pub(super) fn preflight_nbt(bytes: &[u8]) -> Result<()> {
    use anyhow::ensure;
    let mut input = Cursor::new(bytes);
    let mut tag = [0];
    input.read_exact(&mut tag)?;
    ensure!(tag[0] == 10, "NBT root must be a compound");
    skip_nbt_string(&mut input)?;
    let mut tags = 0;
    preflight_nbt_payload(&mut input, tag[0], 0, &mut tags)?;
    ensure!(input.position() as usize == bytes.len(), "trailing NBT data");
    Ok(())
}

pub(super) fn read_gzip_nbt(mut file: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(&mut file)
        .take((limit + 1) as u64)
        .read_to_end(&mut decoded)?;
    anyhow::ensure!(decoded.len() <= limit, "schematic NBT byte limit exceeded");
    Ok(decoded)
}
