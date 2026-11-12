//! Immutable circuit data compiled in the same translation unit as the native runtime.
use super::{AUX_TABLES, LlvmProgram, NativeProgramDescriptor, Program, ScratchError, command, command_notifications};
use crate::redpilator::TaskMonitor;
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, fs, path::Path};

pub(super) struct GeneratedProgram {
    pub(super) encoded: LlvmProgram,
    pub(super) identity: [u8; 32],
    commands: command::CommandTables,
    notifications: command_notifications::CommandNotifications,
    initial_tick_capacity: u32,
    has_reactive: bool,
    lazy_wire_power: bool,
    command_components: Vec<u32>,
    command_neighbors: Vec<u32>,
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use crate::redpilator::sampled::{Operation, llvm::LlvmRuntime, program::ElectricalLink};
    use mchprs_blocks::{BlockPos, blocks::Block};

    fn program(attenuation: u8) -> Program {
        let operations = vec![Operation::Contribution(0), Operation::UpdateLamp(1)];
        let mut program = Program::new(
            vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10)],
            vec![Block::from_name("lever").unwrap(), Block::RedstoneLamp { lit: false }],
            vec![0],
            vec![[operations.clone(), operations], [vec![], vec![]]],
            vec![ElectricalLink {
                source: 0,
                target: 1,
                attenuation,
            }],
            vec![],
            vec![],
        )
        .unwrap();
        program.compile_movement_flips().unwrap();
        program.prepare_projection();
        program
    }

    fn states(runtime: &LlvmRuntime) -> [u8; 2] {
        let mut out = [0; 2];
        // SAFETY: Both runtimes retain their modules/state; the output has exactly two component bytes.
        assert_eq!(
            unsafe { (runtime.library.read_state)(runtime.state.as_ptr().cast(), out.as_mut_ptr(), 2) },
            0
        );
        out
    }

    #[test]
    fn generated_identity_is_deterministic_and_includes_equal_sized_program_changes() {
        let a = GeneratedProgram::new(&program(0)).unwrap();
        let same = GeneratedProgram::new(&program(0)).unwrap();
        let changed = GeneratedProgram::new(&program(15)).unwrap();
        assert_eq!(a.identity, same.identity);
        assert_eq!(a.encoded.blob.len(), changed.encoded.blob.len());
        assert_ne!(a.identity, changed.identity);
        assert_ne!(a.identity, GeneratedProgram::with_tick_capacity(&program(0), 1).unwrap().identity);
        assert!(GeneratedProgram::with_tick_capacity(&program(0), u32::MAX).is_err());
        let mut relaxed = program(0);
        relaxed.enable_relaxed_wire_shapes().unwrap();
        assert_ne!(a.identity, GeneratedProgram::new(&relaxed).unwrap().identity);
    }

    #[test]
    fn movement_steps_inline_fixed_cases_and_preserve_conditional_metadata() {
        let steps = bytes(&[2, 1, 0, 2, 2, 1, 3, 2, 0, 2, 2, u32::MAX]);
        let cases = bytes(&[7, 1, 9, 0, 13, 4, 17, 2]);
        assert_eq!(
            inline_movement_cases(&steps, &cases),
            [2, 1, 0, 2, 2, 1, 9, 0, 17, 2, 3, 2, 0, 2, 2, u32::MAX, 0, 0, 0, 0]
        );
        assert!(inline_movement_cases(&[], &[]).is_empty());
    }

    #[test]
    fn movement_masks_preserve_order_and_fall_back_for_shared_or_large_plans() {
        let steps = bytes(&[7, 0, 1, 0, 0, 9, 0, 0, 0, 1, 10, 0, 0, 1, 1, 8, 0, 1, 0, 0, 11, 0, 0, 2, 2]);
        let dependencies = bytes(&[7, 8, 7, 8]);
        let plan = [0, 0, 0, 5, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            movement_dependency_masks(&bytes(&plan), &steps, &dependencies),
            [0, 0, 1, 0, 0, 0, 0, 0, 9, 0]
        );
        let shared = [plan, [0, 0, 1, 4, 0, 0, 0, 0, 0, 0]].concat();
        assert_eq!(&movement_dependency_masks(&bytes(&shared), &steps, &dependencies)[2..4], &[u32::MAX; 2]);
        assert_eq!(
            movement_dependency_masks(&bytes(&[0, 0, 0, 65, 0, 0, 0, 0, 0, 0]), &bytes(&[0; 325]), &[]),
            vec![u32::MAX; 130]
        );
        assert!(movement_dependency_masks(&[], &[], &[]).is_empty());
    }

    #[test]
    fn wire_graph_uses_only_immutable_unwatched_inputs() {
        use crate::redpilator::sampled::program::WatchedWire;
        for dynamic in [false, true] {
            for watched in [false, true] {
                let source = if dynamic {
                    Block::from_name("lever").unwrap()
                } else {
                    Block::RedstoneBlock
                };
                let mut p = Program::new(
                    vec![BlockPos::new(10, 10, 10), BlockPos::new(11, 10, 10)],
                    vec![source, Block::from_name("redstone_wire").unwrap()],
                    if dynamic { vec![0] } else { vec![] },
                    vec![[vec![], vec![]], [vec![], vec![]]],
                    vec![ElectricalLink {
                        source: 0,
                        target: 1,
                        attenuation: 2,
                    }],
                    if watched {
                        vec![WatchedWire { wire: 1, watchers: vec![] }]
                    } else {
                        vec![]
                    },
                    vec![],
                )
                .unwrap();
                p.compile_movement_flips().unwrap();
                p.prepare_projection();
                let encoded = LlvmProgram::encode(&p).unwrap();
                assert!(wire_replay_graph(&encoded, false).iter().all(Vec::is_empty));
                let graph = wire_replay_graph(&encoded, true);
                if dynamic || watched {
                    assert!(graph.iter().all(Vec::is_empty));
                } else {
                    assert_eq!(graph[0], [u32::MAX, u32::MAX, u32::MAX, 0]);
                    assert_eq!(graph[1], [13 | 1 << 8]);
                    assert!(graph[2].is_empty());
                    assert_eq!(graph[3], [0]);
                }
            }
        }
    }

    #[test]
    fn movement_input_classification_preserves_token_levels_and_main_side_semantics() {
        let initial = [
            Block::RedstoneLamp { lit: false },
            Block::from_name("redstone_wire").unwrap(),
            Block::from_name("comparator").unwrap(),
        ];
        let mut flips = Vec::new();
        for target in 0..3 {
            for side in 0..2 {
                flips.extend(bytes(&[target, 0x8000_0000, 7, 0x000f_0107 | side << 9]));
            }
        }
        let original = flips.clone();
        mark_analog_flips(&mut flips, &initial);
        for (index, (before, after)) in original.chunks_exact(16).zip(flips.chunks_exact(16)).enumerate() {
            assert_eq!(&before[..12], &after[..12]);
            let flags = u32::from_le_bytes(after[12..].try_into().unwrap());
            assert_eq!(flags & !0x400, u32::from_le_bytes(before[12..].try_into().unwrap()));
            assert_eq!(flags & 0x400 != 0, [false, false, true, false, true, true][index]);
        }
        mark_analog_flips(&mut [], &[]);
    }

    #[test]
    fn generated_empty_tables_and_cancelled_initialization_are_supported() {
        let mut empty = Program::new(vec![], vec![], vec![], vec![], vec![], vec![], vec![]).unwrap();
        empty.compile_movement_flips().unwrap();
        empty.prepare_projection();
        let native = LlvmRuntime::new(&empty, &[]).unwrap();
        // SAFETY: The native instance and its module are live; the counter query is read-only.
        assert_eq!(unsafe { (native.library.work_count)(native.state.as_ptr().cast()) }, 0);
        let monitor = TaskMonitor::default();
        monitor.cancel();
        assert!(
            matches!(LlvmRuntime::new_with_monitor(&program(0), &[], Some(&monitor)), Err(error) if error.reason == "LLVM compilation cancelled")
        );
    }

    #[test]
    fn generated_modules_own_program_data_and_instances_have_independent_state() {
        let a = program(0);
        let mut left = LlvmRuntime::new(&a, &[]).unwrap();
        let right = LlvmRuntime::new(&a, &[]).unwrap();
        assert!(left.library.create_embedded_state(1).is_err());
        left.use_control(a.positions[0]).unwrap();
        assert_eq!(states(&left), [1, 1]);
        assert_eq!(states(&right), [0, 0]);
        let b = program(15);
        let mut other = LlvmRuntime::new(&b, &[]).unwrap();
        other.use_control(b.positions[0]).unwrap();
        assert_eq!(states(&other), [1, 0]);
        assert_eq!(states(&left), [1, 1]);
        let wrong_identity = GeneratedProgram::new(&b).unwrap().identity;
        assert!(left.library.validate_program_identity(&wrong_identity).is_err());
        drop(left);
        assert_eq!(states(&right), [0, 0]);
    }
}

fn descriptor_words(p: &NativeProgramDescriptor) -> [u32; 13] {
    [
        p.abi_version,
        p.header_bytes,
        p.total_bytes,
        p.operation_offset,
        p.operation_count,
        p.link_offset,
        p.link_count,
        p.component_offset,
        p.component_count,
        p.source_offset,
        p.source_count,
        p.arrangement_link_count,
        p.watched_wire_count,
    ]
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

fn inline_movement_cases(steps: &[u8], cases: &[u8]) -> Vec<u32> {
    let mut words = Vec::with_capacity(steps.len() / 24 * 10);
    for step in steps.chunks_exact(24) {
        let row: [u32; 6] = std::array::from_fn(|i| u32::from_le_bytes(step[i * 4..i * 4 + 4].try_into().unwrap()));
        words.extend_from_slice(&row);
        for base in [row[2], row[3]] {
            if row[5] == u32::MAX {
                words.extend_from_slice(&[0, 0]);
            } else {
                let at = (base as usize + row[5] as usize) * 8;
                words.extend(cases[at..at + 8].chunks_exact(4).map(|word| u32::from_le_bytes(word.try_into().unwrap())));
            }
        }
    }
    words
}

fn movement_dependency_masks(plans: &[u8], steps: &[u8], dependencies: &[u8]) -> Vec<u32> {
    let read = |data: &[u8], at: usize| u32::from_le_bytes(data[at * 4..at * 4 + 4].try_into().unwrap());
    let mut masks = vec![None; steps.len() / 20];
    for plan in plans.chunks_exact(40) {
        for side in [2, 4] {
            let start = read(plan, side) as usize;
            let count = read(plan, side + 1) as usize;
            if count > 64 {
                continue;
            }
            let mut previous = std::collections::HashMap::<u32, u64>::new();
            for i in 0..count {
                let index = start + i;
                let row = &steps[index * 20..index * 20 + 20];
                let first = read(row, 3) as usize;
                let count = read(row, 4) as usize;
                let mask = (first..first + count).fold(0, |mask, j| mask | previous.get(&read(dependencies, j)).copied().unwrap_or(0));
                masks[index] = Some(match masks[index] {
                    Some(old) if old != mask => u64::MAX,
                    _ => mask,
                });
                if read(row, 2) != 0 {
                    *previous.entry(read(row, 0)).or_default() |= 1u64 << i;
                }
            }
        }
    }
    masks
        .into_iter()
        .flat_map(|mask| {
            let mask = mask.unwrap_or(u64::MAX);
            [mask as u32, (mask >> 32) as u32]
        })
        .collect()
}

// Component group/offset rows, packed level/count values, retained flips, and canonical prefix offsets.
fn wire_replay_graph(encoded: &LlvmProgram, lazy_wire_power: bool) -> [Vec<u32>; 4] {
    if !lazy_wire_power {
        return Default::default();
    }
    let p = &encoded.descriptor;
    let blob = &encoded.blob;
    let read = |data: &[u8], at: usize| u32::from_le_bytes(data[at * 4..at * 4 + 4].try_into().unwrap());
    let directory = p.source_offset as usize + p.source_count as usize * 4;
    let tables: [&[u8]; AUX_TABLES] = std::array::from_fn(|index| {
        let at = directory + index * 8;
        let offset = u32::from_le_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
        let count = u32::from_le_bytes(blob[at + 4..at + 8].try_into().unwrap()) as usize;
        &blob[offset..offset + count * 4]
    });
    let word = |table: usize, at: usize| read(tables[table], at);
    let kind = |node: usize| blob[p.component_offset as usize + node * 32 + 12];
    let immutable = |source: u32| {
        if source & 0x8000_0000 != 0 {
            Some(word(11, (source & 0x7fff_ffff) as usize * 3 + 1))
        } else if kind(source as usize) == 3 {
            Some(if word(30, source as usize) == 0 { 15 } else { 0 })
        } else {
            None
        }
    };
    let nodes = p.component_count as usize;
    let mut eligible: Vec<_> = (0..nodes).map(|node| kind(node) == 5).collect();
    for node in 0..nodes {
        if [0, 5, 28].into_iter().any(|table| word(table, node * 2 + 1) != 0) || [0, 2, 4].into_iter().any(|field| word(43, node * 5 + field) != 0) {
            eligible[node] = false;
        }
    }
    for (table, stride) in [(1, 2), (3, 3), (45, 5), (46, 2), (47, 1), (48, 1)] {
        for at in (0..tables[table].len() / 4).step_by(stride) {
            eligible[word(table, at) as usize] = false;
        }
    }
    let mut fixed_count = vec![0u32; nodes];
    let mut fixed_level = vec![0u32; nodes];
    for link in blob[p.link_offset as usize..p.link_offset as usize + p.link_count as usize * 12].chunks_exact(12) {
        let node = read(link, 1) as usize;
        if !eligible[node] || link[9] == 0 {
            continue;
        }
        if let Some(strength) = immutable(read(link, 0)) {
            let level = strength.saturating_sub(u32::from(link[8]));
            fixed_count[node] += u32::from(level != 0);
            fixed_level[node] = fixed_level[node].max(level);
        } else {
            eligible[node] = false;
        }
    }
    let mut routed = vec![false; p.arrangement_link_count as usize];
    for index in 0..tables[39].len() / 4 {
        routed[word(39, index) as usize] = true;
    }
    let mut arranged = vec![Vec::<[u32; 4]>::new(); nodes];
    for (index, row) in tables[25].chunks_exact(32).enumerate() {
        let node = read(row, 0) as usize;
        if !eligible[node] {
            continue;
        }
        if let Some(strength) = immutable(read(row, 3)).filter(|_| read(row, 1) == 0 && !routed[index]) {
            arranged[node].push([read(row, 5), read(row, 6), read(row, 7), strength.saturating_sub(read(row, 4))]);
        } else {
            eligible[node] = false;
        }
    }
    let mut rows = vec![u32::MAX; nodes * 2];
    let mut values = Vec::new();
    for node in 0..nodes {
        if !eligible[node] {
            continue;
        }
        let (group, states) = arranged[node].first().map_or((u32::MAX, 1), |row| (row[0], row[2]));
        if states > 4096 || arranged[node].iter().any(|row| row[0] != group || row[2] != states) {
            continue;
        }
        // shortcut: cap derived lookups at 4 MiB; raise only when larger circuits show a measured gain.
        if values.len() + states as usize > 1024 * 1024 {
            continue;
        }
        rows[node * 2] = group;
        rows[node * 2 + 1] = values.len() as u32;
        for state in 0..states {
            let mut count = fixed_count[node];
            let mut level = fixed_level[node];
            for &[_, start, _, strength] in &arranged[node] {
                if word(26, (start + state) as usize) != 0 && strength != 0 {
                    count += 1;
                    level = level.max(strength);
                }
            }
            // Preserve full 32-bit counts through scalar replay if a packed value cannot represent them.
            if count >= 1 << 24 {
                rows[node * 2 + 1] = u32::MAX;
                break;
            }
            values.push(level | count << 8);
        }
    }
    if values.is_empty() {
        return Default::default();
    }
    let mut flips = Vec::new();
    let mut prefix = vec![0];
    for row in tables[22].chunks_exact(16) {
        if rows[read(row, 0) as usize * 2 + 1] == u32::MAX {
            flips.extend((0..4).map(|index| read(row, index)));
        }
        prefix.push((flips.len() / 4) as u32);
    }
    [rows, values, flips, prefix]
}

fn mark_analog_flips(flips: &mut [u8], initial: &[mchprs_blocks::blocks::Block]) {
    use mchprs_blocks::blocks::Block;
    for row in flips.chunks_exact_mut(16) {
        let target = u32::from_le_bytes(row[..4].try_into().unwrap()) as usize;
        let flags = u32::from_le_bytes(row[12..].try_into().unwrap());
        let analog = matches!(initial[target], Block::RedstoneComparator { .. })
            || (flags & 0x200 == 0 && matches!(initial[target], Block::RedstoneWire { .. }));
        row[12..].copy_from_slice(&(flags | (u32::from(analog) << 10)).to_le_bytes());
    }
}

impl GeneratedProgram {
    #[cfg(test)]
    pub(super) fn new(program: &Program) -> Result<Self, ScratchError> {
        Self::with_tick_capacity(program, 0)
    }

    pub(super) fn with_tick_capacity(program: &Program, initial_tick_capacity: u32) -> Result<Self, ScratchError> {
        let components = u32::try_from(program.initial.len()).map_err(|_| super::overflow())?;
        let event_capacity = components
            .checked_mul(2)
            .and_then(|n| n.checked_add(initial_tick_capacity))
            .ok_or_else(super::overflow)?
            .max(64);
        if u64::from(event_capacity) * 8 + u64::from(components) * 128 > 256 * 1024 * 1024 {
            return Err(ScratchError {
                position: None,
                reason: "LLVM state size rejected",
            });
        }
        let wires: Vec<_> = program
            .initial
            .iter()
            .enumerate()
            .filter_map(|(index, block)| matches!(block, mchprs_blocks::blocks::Block::RedstoneWire { .. }).then_some(index))
            .collect();
        let lazy_wire_power = program.relaxed_mode
            && !wires.is_empty()
            && wires.iter().all(|&wire| {
                program.outgoing[wire].is_empty() && program.outgoing_arrangements[wire].is_empty() && program.side_outputs[wire].is_empty()
            });
        let mut encoded = LlvmProgram::encode(program)?;
        let directory = encoded.descriptor.source_offset as usize + encoded.descriptor.source_count as usize * 4;
        let at = directory + 22 * 8;
        let offset = u32::from_le_bytes(encoded.blob[at..at + 4].try_into().unwrap()) as usize;
        let words = u32::from_le_bytes(encoded.blob[at + 4..at + 8].try_into().unwrap()) as usize;
        mark_analog_flips(&mut encoded.blob[offset..offset + words * 4], &program.initial);
        let commands = command::encode(program)?.serialize_at(std::time::Instant::now())?;
        let notifications = command_notifications::encode(program)?;
        let mut command_components = vec![u32::MAX; program.initial.len()];
        let positions: std::collections::HashMap<_, _> = commands
            .blocks
            .chunks_exact(10)
            .enumerate()
            .map(|(index, row)| {
                let pos = program.positions[row[0] as usize];
                command_components[row[0] as usize] = index as u32;
                ((i64::from(pos.x), i64::from(pos.y), i64::from(pos.z)), index as u32)
            })
            .collect();
        let mut command_neighbors = Vec::with_capacity(commands.blocks.len() / 5);
        for row in commands.blocks.chunks_exact(10) {
            let pos = program.positions[row[0] as usize];
            let (dx, dy, dz) = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][row[3] as usize];
            for direction in [-1, 1] {
                let neighbor = (
                    i64::from(pos.x) + direction * dx,
                    i64::from(pos.y) + direction * dy,
                    i64::from(pos.z) + direction * dz,
                );
                command_neighbors.push(positions.get(&neighbor).copied().unwrap_or(u32::MAX));
            }
        }

        let mut hash = Sha256::new();
        hash.update(b"MROWW embedded program v2");
        hash.update(initial_tick_capacity.to_le_bytes());
        hash.update([u8::from(program.relaxed_mode)]);
        hash.update(bytes(&descriptor_words(&encoded.descriptor)));
        hash.update(&encoded.blob);
        for words in [&commands.blocks, &commands.specs, &notifications.rows, &notifications.targets] {
            hash.update((words.len() as u64).to_le_bytes());
            for word in words {
                hash.update(word.to_le_bytes());
            }
        }
        hash.update((commands.strings.len() as u64).to_le_bytes());
        hash.update(&commands.strings);
        Ok(Self {
            encoded,
            identity: hash.finalize().into(),
            commands,
            notifications,
            initial_tick_capacity,
            has_reactive: program.has_reactive,
            lazy_wire_power,
            command_components,
            command_neighbors,
        })
    }

    pub(super) fn write(&self, directory: &Path, monitor: Option<&TaskMonitor>) -> Result<(), String> {
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err("LLVM compilation cancelled".into());
        }
        let p = &self.encoded.descriptor;
        let blob = &self.encoded.blob;
        let mut header = String::from(
            "/* Generated from validated circuit data; never edit. */\n#define NE_EMBEDDED_PROGRAM 1\n#define NE_STATIC_ANALOG_FLIPS 1\n",
        );
        writeln!(header, "#define NE_STATIC_IMPORTED_TICK_CAPACITY {}u\n#define NE_STATIC_EVENT_CAPACITY {}u\n#define NE_STATIC_HAS_REACTIVE {}u\n#define NE_STATIC_LAZY_WIRE_POWER {}u", self.initial_tick_capacity, (p.component_count*2+self.initial_tick_capacity).max(64), u8::from(self.has_reactive), u8::from(self.lazy_wire_power)).unwrap();
        writeln!(
            header,
            "static const MchprsNativeProgramDescriptor ne_static_program = {{{}}};",
            descriptor_words(p).map(|w| format!("{w}u")).join(",")
        )
        .unwrap();
        writeln!(
            header,
            "static const uint8_t ne_static_identity[32] = {{{}}};",
            self.identity.map(|b| b.to_string()).join(",")
        )
        .unwrap();
        header.push_str("static const uint32_t ne_empty_words[1] = {0};\ntypedef struct {\n uint8_t header[16];\n");
        let mut fields = Vec::new();
        for (name, ty, count, offset) in [
            ("operations", "uint8_t", p.operation_count as usize * 8, p.operation_offset as usize),
            ("links", "uint8_t", p.link_count as usize * 12, p.link_offset as usize),
            (
                "components",
                "MchprsNativeComponent",
                p.component_count as usize,
                p.component_offset as usize,
            ),
            ("sources", "uint32_t", p.source_count as usize, p.source_offset as usize),
        ] {
            if count > 0 {
                writeln!(header, " {ty} {name}[{count}];").unwrap();
                fields.push((name.to_owned(), offset));
            }
            writeln!(
                header,
                "#define NE_STATIC_{} {}",
                name.to_uppercase(),
                if count > 0 {
                    format!("ne_static_data.data.{name}")
                } else {
                    format!("((const {ty} *)ne_empty_words)")
                }
            )
            .unwrap();
        }
        let directory_offset = p.source_offset as usize + p.source_count as usize * 4;
        writeln!(header, " uint32_t directory[{}];", AUX_TABLES * 2).unwrap();
        let mut sizes = Vec::new();
        let mut pointers = Vec::new();
        for index in 0..AUX_TABLES {
            let at = directory_offset + index * 8;
            let offset = u32::from_le_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(blob[at + 4..at + 8].try_into().unwrap()) as usize;
            sizes.push(format!("{count}u"));
            if count > 0 {
                writeln!(header, " uint32_t table_{index}[{count}];").unwrap();
                fields.push((format!("table_{index}"), offset));
                pointers.push(format!("ne_static_data.data.table_{index}"));
            } else {
                pointers.push("ne_empty_words".to_owned());
            }
        }
        header.push_str("} NEStaticProgramData;\n");
        for (name, offset) in fields {
            writeln!(
                header,
                "_Static_assert(offsetof(NEStaticProgramData,{name}) == {offset}u, \"program layout\");"
            )
            .unwrap();
        }
        writeln!(
            header,
            "_Static_assert(sizeof(NEStaticProgramData) == {}u, \"program size\");",
            blob.len()
        )
        .unwrap();
        writeln!(
            header,
            "static const union {{ uint8_t bytes[{}]; NEStaticProgramData data; }} ne_static_data = {{.bytes = {{\n#embed \"program.bin\"\n}}}};",
            blob.len()
        )
        .unwrap();
        writeln!(
            header,
            "static const uint32_t *const ne_static_tables[{AUX_TABLES}] = {{{}}};",
            pointers.join(",")
        )
        .unwrap();
        writeln!(header, "static const uint32_t ne_static_sizes[{AUX_TABLES}] = {{{}}};", sizes.join(",")).unwrap();
        fs::write(directory.join("program.bin"), blob).map_err(|e| e.to_string())?;
        let section = |index: usize| {
            let at = directory_offset + index * 8;
            let offset = u32::from_le_bytes(blob[at..at + 4].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(blob[at + 4..at + 8].try_into().unwrap()) as usize;
            &blob[offset..offset + count * 4]
        };
        // Keep proven fixed plans beside their rules to avoid a dependent lookup during movement.
        let movement_steps = inline_movement_cases(section(20), section(21));
        let movement_masks = if self.lazy_wire_power {
            movement_dependency_masks(section(19), section(23), section(24))
        } else {
            Vec::new()
        };
        header.push_str("#define NE_OPTIMIZED_MOVEMENT_STEPS 1\n");
        // Paired measurements regressed with eager strict-mode wire refreshes.
        if self.lazy_wire_power {
            header.push_str("#define NE_STATIC_MOVEMENT_MASKS 1\n");
        }
        let mut graph = wire_replay_graph(&self.encoded, self.lazy_wire_power);
        if graph[2].len() >= section(22).len() / 4 {
            graph = Default::default();
        }
        if !graph[1].is_empty() {
            header.push_str("#define NE_STATIC_WIRE_GRAPH 1\n");
        }
        for (name, words) in [
            ("graph_rows", &graph[0]),
            ("graph_values", &graph[1]),
            ("graph_flips", &graph[2]),
            ("graph_prefix", &graph[3]),
            ("movement_steps", &movement_steps),
            ("movement_masks", &movement_masks),
            ("command_blocks", &self.commands.blocks),
            ("command_components", &self.command_components),
            ("command_neighbors", &self.command_neighbors),
            ("command_specs", &self.commands.specs),
            ("command_notify_rows", &self.notifications.rows),
            ("command_notify_targets", &self.notifications.targets),
        ] {
            if words.is_empty() {
                writeln!(header, "#define ne_static_{name} ne_empty_words").unwrap();
            } else {
                fs::write(directory.join(format!("{name}.bin")), bytes(words)).map_err(|e| e.to_string())?;
                writeln!(header, "static const union {{ uint8_t bytes[{}]; uint32_t words[{}]; }} ne_data_{name} = {{.bytes = {{\n#embed \"{name}.bin\"\n}}}};\n#define ne_static_{name} ne_data_{name}.words", words.len()*4, words.len()).unwrap();
            }
        }
        if self.commands.strings.is_empty() {
            header.push_str("static const uint8_t ne_static_command_strings[1] = {0};\n");
        } else {
            fs::write(directory.join("command_strings.bin"), &self.commands.strings).map_err(|e| e.to_string())?;
            writeln!(
                header,
                "static const uint8_t ne_static_command_strings[{}] = {{\n#embed \"command_strings.bin\"\n}};",
                self.commands.strings.len()
            )
            .unwrap();
        }
        writeln!(
            header,
            "#define NE_STATIC_COMMAND_COUNT {}u\n#define NE_STATIC_STRING_BYTES {}u\n#define NE_STATIC_COMMAND_TARGET_COUNT {}u",
            self.commands.blocks.len() / 10,
            self.commands.strings.len(),
            self.notifications.targets.len() / 3
        )
        .unwrap();
        fs::write(directory.join("program.h"), header).map_err(|e| e.to_string())?;
        if monitor.is_some_and(TaskMonitor::cancelled) {
            return Err("LLVM compilation cancelled".into());
        }
        Ok(())
    }
}
