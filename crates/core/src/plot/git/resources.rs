//! Bound canonical JSON scratch and reject NBT allocation bombs before parsing.
use crate::messages;
use anyhow::{ensure, Result};
use mchprs_blocks::block_entities::BlockEntity;

pub(super) fn check_entity(entity: &BlockEntity) -> Result<()> {
    ensure!(
        bincode::serialized_size(entity)? <= 256 * 1024,
        messages::GIT_ENTITY_SIZE_LIMIT
    );
    if let BlockEntity::Container { inventory, .. } = entity {
        for entry in inventory {
            if let Some(bytes) = &entry.nbt {
                check_nbt(bytes)?;
            }
        }
    }
    Ok(())
}

pub(super) fn check_nbt(bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= 64 * 1024, messages::GIT_NBT_RESOURCE_LIMIT);
    let mut reader = NbtReader { bytes, tags: 4096 };
    ensure!(reader.take(1)?[0] == 10, messages::GIT_NBT_RESOURCE_LIMIT);
    reader.string()?;
    reader.payload(10, 0)?;
    ensure!(reader.bytes.is_empty(), messages::GIT_NBT_RESOURCE_LIMIT);
    Ok(())
}

struct NbtReader<'a> {
    bytes: &'a [u8],
    tags: usize,
}

impl<'a> NbtReader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        ensure!(count <= self.bytes.len(), messages::GIT_NBT_RESOURCE_LIMIT);
        let (data, remaining) = self.bytes.split_at(count);
        self.bytes = remaining;
        Ok(data)
    }

    fn string(&mut self) -> Result<()> {
        let length = u16::from_be_bytes(self.take(2)?.try_into()?) as usize;
        self.take(length)?;
        Ok(())
    }

    fn count(&mut self) -> Result<usize> {
        let count = i32::from_be_bytes(self.take(4)?.try_into()?);
        ensure!(count >= 0, messages::GIT_NBT_RESOURCE_LIMIT);
        Ok(count as usize)
    }

    fn payload(&mut self, tag: u8, depth: usize) -> Result<()> {
        ensure!(
            depth < 64 && self.tags > 0,
            messages::GIT_NBT_RESOURCE_LIMIT
        );
        self.tags -= 1;
        match tag {
            1..=6 => {
                self.take([1, 2, 4, 8, 4, 8][tag as usize - 1])?;
            }
            7 | 11 | 12 => {
                let count = self.count()?;
                let width = match tag {
                    7 => 1,
                    11 => 4,
                    _ => 8,
                };
                ensure!(
                    count <= self.bytes.len() / width,
                    messages::GIT_NBT_RESOURCE_LIMIT
                );
                self.take(count * width)?;
            }
            8 => self.string()?,
            9 => {
                let element = self.take(1)?[0];
                let count = self.count()?;
                ensure!(
                    element <= 12
                        && (element != 0 || count == 0)
                        && count <= self.tags
                        && count <= self.bytes.len(),
                    messages::GIT_NBT_RESOURCE_LIMIT
                );
                for _ in 0..count {
                    self.payload(element, depth + 1)?;
                }
            }
            10 => loop {
                let child = self.take(1)?[0];
                if child == 0 {
                    break;
                }
                self.string()?;
                self.payload(child, depth + 1)?;
            },
            _ => anyhow::bail!(messages::GIT_NBT_RESOURCE_LIMIT),
        }
        Ok(())
    }
}
