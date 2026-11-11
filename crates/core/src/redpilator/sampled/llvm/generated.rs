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
        let encoded = LlvmProgram::encode(program)?;
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
        let mut header = String::from("/* Generated from validated circuit data; never edit. */\n#define NE_EMBEDDED_PROGRAM 1\n");
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
        for (name, words) in [
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
