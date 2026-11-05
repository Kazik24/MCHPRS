//! Pointer-free local connectivity tables; row offsets count records, not bytes.
use super::{AUX_TABLES, Program, ScratchError, overflow};
use crate::redpilator::sampled::routing::{Axis, Local};

fn id(n: usize) -> Result<u32, ScratchError> {
    u32::try_from(n).map_err(|_| overflow())
}

fn local<T>(
    local: &Local<T>,
    tables: &mut [Vec<u32>; AUX_TABLES],
    ports: impl Fn(&T) -> Result<(Vec<(usize, bool)>, Vec<(u32, bool)>), ScratchError>,
) -> Result<(), ScratchError> {
    let axes = id(tables[34].len() / 2)?;
    let rows = id(tables[35].len() / 4)?;
    for &Axis::Token(token, cells) in &local.axes {
        tables[34].extend([id(token)?, id(cells + 1)?]);
    }
    tables[33].extend([axes, id(local.axes.len())?, rows, id(local.ports.len())?]);
    for row in &local.ports {
        let Some(row) = row else {
            tables[35].extend([u32::MAX, 0, 0, 0]);
            continue;
        };
        let (edges, sources) = ports(row)?;
        tables[35].extend([id(tables[36].len() / 2)?, id(edges.len())?, id(tables[37].len() / 2)?, id(sources.len())?]);
        for (wire, side) in edges {
            tables[36].extend([id(wire)?, u32::from(side)]);
        }
        for (source, side) in sources {
            tables[37].extend([source, u32::from(side)]);
        }
    }
    Ok(())
}

pub(super) fn encode(program: &Program, tables: &mut [Vec<u32>; AUX_TABLES]) -> Result<(), ScratchError> {
    let Some(graph) = &program.routing else {
        return Ok(());
    };
    tables[33].push(id(graph.nodes.len())?);
    for node in &graph.nodes {
        local(node, tables, |ports| {
            Ok((
                ports.wires.iter().map(|&wire| (wire, false)).collect(),
                ports
                    .sources
                    .iter()
                    .map(|pos| super::movement::source(graph.source_ids[pos]).map(|source| (source, false)))
                    .collect::<Result<_, _>>()?,
            ))
        })?;
    }
    for (index, reader) in graph.readers.iter().enumerate() {
        local(&reader.table, tables, |ports| {
            Ok((
                ports.roots.clone(),
                ports
                    .direct
                    .iter()
                    .map(|&(source, side)| super::movement::source(source).map(|source| (source, side)))
                    .collect::<Result<_, _>>()?,
            ))
        })?;
        tables[38].extend([id(graph.nodes.len() + index)?, id(tables[39].len())?, id(reader.links.len())?]);
        for &link in &reader.links {
            tables[39].push(id(link)?);
        }
    }
    for readers in &graph.by_token {
        tables[40].extend([id(tables[41].len())?, id(readers.len())?]);
        for &reader in readers {
            tables[41].push(id(reader)?);
        }
    }
    Ok(())
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::super::{ABI_VERSION, AUX_TABLES, HEADER_BYTES, Library, MAGIC, NativeProgramDescriptor};

    #[test]
    fn llvm_rejects_malformed_routing_tables() {
        if std::env::var_os("MCHPRS_LLVM_DLL").is_none()
            && !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/native_engine.dll")
                .exists()
        {
            return;
        }
        let library = Library::load().unwrap();
        let mut tables: [Vec<u32>; AUX_TABLES] = std::array::from_fn(|_| Vec::new());
        // One token, one cell, and one wire with two local states.
        tables[11] = vec![u32::MAX, 0, u32::MAX];
        tables[12] = vec![0, 1];
        tables[13] = vec![0];
        tables[14] = vec![0, 0];
        tables[16] = vec![u32::MAX, 0, 0, 0, 0, 0, 0];
        tables[18] = vec![0, 1];
        tables[19] = vec![0; 6];
        tables[33] = vec![1, 0, 1, 0, 2];
        tables[34] = vec![0, 2];
        tables[35] = vec![0, 0, 0, 0, u32::MAX, 0, 0, 0];
        tables[40] = vec![0, 0];
        tables[50] = vec![1];
        tables[51] = vec![0];
        tables[53] = vec![0, 0];

        let accepts = |tables: &[Vec<u32>; AUX_TABLES], offset: usize, bad_magic: bool| {
            let mut blob = vec![0u8; HEADER_BYTES + AUX_TABLES * 8];
            blob[0..4].copy_from_slice(&MAGIC);
            blob[4..8].copy_from_slice(&ABI_VERSION.to_le_bytes());
            blob[8..12].copy_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
            for (index, table) in tables.iter().enumerate() {
                let directory = HEADER_BYTES + index * 8;
                let offset = blob.len() as u32;
                blob[directory..directory + 4].copy_from_slice(&offset.to_le_bytes());
                blob[directory + 4..directory + 8].copy_from_slice(&(table.len() as u32).to_le_bytes());
                for word in table {
                    blob.extend_from_slice(&word.to_le_bytes());
                }
            }
            let size = blob.len() as u32;
            blob[12..16].copy_from_slice(&size.to_le_bytes());
            if bad_magic {
                blob[0] = 0;
            }
            let descriptor = NativeProgramDescriptor {
                abi_version: ABI_VERSION,
                header_bytes: HEADER_BYTES as u32,
                total_bytes: size,
                operation_offset: HEADER_BYTES as u32,
                operation_count: 0,
                link_offset: HEADER_BYTES as u32,
                link_count: 0,
                component_offset: HEADER_BYTES as u32,
                component_count: 0,
                source_offset: HEADER_BYTES as u32,
                source_count: 0,
                arrangement_link_count: 0,
                watched_wire_count: 0,
            };
            let mut aligned = vec![0u64; (blob.len() + offset).div_ceil(8)];
            unsafe {
                let data = aligned.as_mut_ptr().cast::<u8>().add(offset);
                std::ptr::copy_nonoverlapping(blob.as_ptr(), data, blob.len());
                let state_bytes = (library.state_size)(&descriptor, data, size, 0);
                if state_bytes != 0 {
                    let mut memory = vec![0u64; (state_bytes + 8).div_ceil(8)];
                    let unaligned = memory.as_mut_ptr().cast::<u8>().add(1);
                    assert_ne!((library.create)(&descriptor, data, size, unaligned.cast(), state_bytes, 0), 0);
                }
                state_bytes != 0
            }
        };
        assert!(accepts(&tables, 0, false), "valid local routing table");
        assert!(!accepts(&tables, 1, false), "unaligned Program must be rejected");
        assert!(!accepts(&tables, 0, true), "invalid Program magic must be rejected");
        for (table, word, value) in [
            (33, 0, 2), // Wire count exceeds local records.
            (33, 4, 3), // State count differs from the axis product.
            (34, 1, 3), // Radix differs from the token's cell count.
            (35, 1, 1), // Edge range exceeds the edge table.
            (35, 7, 1), // Invalid states cannot contain port data.
        ] {
            let mut malformed = tables.clone();
            malformed[table][word] = value;
            assert!(!accepts(&malformed, 0, false), "malformed table {table}, word {word}");
        }
        for (table, data) in [(53, vec![0, 1]), (54, vec![0; 7]), (56, vec![0]), (57, vec![256; 256]), (58, vec![0])] {
            let mut malformed = tables.clone();
            malformed[table] = data;
            assert!(!accepts(&malformed, 0, false), "malformed shape table {table}");
        }
    }
}
