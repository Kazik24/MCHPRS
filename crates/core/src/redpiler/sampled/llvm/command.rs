//! Pointer-free command-block bytecode inputs for the native executor.
use super::{Program, ScratchError};
use crate::chat_commands::{ChatCommand, Recipient};
use mchprs_blocks::{BlockFace, BlockPos, block_entities::CommandBlockEntity, blocks::Block};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StringRef {
    pub(super) offset: u32,
    pub(super) len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecipientKind {
    All,
    Name(StringRef),
    Nobody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandResult {
    Empty,
    Success { recipient: RecipientKind, message: StringRef },
    Error(StringRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandSpec {
    /// UTF-8 byte lengths are retained for CommandBlockOutputState's limits.
    pub(super) command_len: u32,
    pub(super) source_len: u32,
    pub(super) result: CommandResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandBlockMode {
    Impulse,
    Repeating,
    Chain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandBlockRecord {
    /// Component index; records follow Program component order.
    pub(super) component: u32,
    pub(super) spec: u32,
    pub(super) mode: CommandBlockMode,
    pub(super) facing: BlockFace,
    pub(super) conditional: bool,
    pub(super) has_entity: bool,
    pub(super) powered: bool,
    pub(super) automatic: bool,
    pub(super) condition_met: bool,
    pub(super) track_output: bool,
    pub(super) update_last_execution: bool,
    pub(super) success_count: i32,
    pub(super) last_execution: i64,
    pub(super) last_output: Option<StringRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct EncodedCommands {
    pub(super) blocks: Vec<CommandBlockRecord>,
    pub(super) specs: Vec<CommandSpec>,
    /// Packed UTF-8 payloads; references are byte offsets, never process pointers.
    pub(super) strings: Vec<u8>,
    pub(super) output_state: EncodedOutputState,
    /// Rust-only clock anchor; it is sampled into the scalar table on serialize.
    window_started: Option<std::time::Instant>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EncodedOutputState {
    pub(super) available: bool,
    pub(super) limited: bool,
    pub(super) count: u32,
    pub(super) bytes: u32,
    pub(super) queued: u32,
}

/// Serialized table rows use only u32 words and byte offsets. ABI v1 schema:
///
/// - `blocks`: 10 words per row: component, spec, mode (impulse/repeating/chain
///   = 0/1/2), facing (`BlockFace` ID), flags (conditional/entity/powered/auto/
///   condition-met/track-output/update-last-execution = bits 0..6), success
///   count (i32 bits), last-execution low/high, last-output byte offset/length.
/// - `specs`: 10 words per row: command/source UTF-8 byte lengths, result
///   (empty/success/error = 0/1/2), recipient (none/all/name/nobody = 0/1/2/3),
///   recipient-name offset/length, message offset/length, error offset/length.
///   Unused offset fields are `u32::MAX` and their lengths are zero.
/// - `strings`: concatenated UTF-8 bytes referenced by byte offsets above.
/// - `output_state`: available, limited, count, bytes, queued, window age ns
///   measured by `serialize_at` at native state creation, after compilation and
///   DLL loading.
///   C must apply precedence in this order: unavailable output state, empty
///   command no-op, window reset, message-count/command/source limits, static
///   parse result, parsed-message byte limit. Parser errors bypass the output-
///   byte limit as in `CommandBlockOutputState`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct CommandTables {
    pub(super) blocks: Vec<u32>,
    pub(super) specs: Vec<u32>,
    pub(super) strings: Vec<u8>,
    pub(super) output_state: [u32; 6],
}

impl EncodedCommands {
    /// Call with `Instant::now()` immediately before native state creation so
    /// compilation and DLL loading time count against the existing output window.
    pub(super) fn serialize_at(&self, now: std::time::Instant) -> Result<CommandTables, ScratchError> {
        const BLOCK_WORDS: usize = 10;
        const SPEC_WORDS: usize = 10;
        const NO_STRING: u32 = u32::MAX;
        let check_ref = |reference: StringRef| -> Result<(), ScratchError> {
            let end = (reference.offset as usize).checked_add(reference.len as usize).ok_or_else(invalid)?;
            if end > self.strings.len() {
                return Err(invalid());
            }
            Ok(())
        };
        let optional_ref = |reference: Option<StringRef>| -> Result<(u32, u32), ScratchError> {
            match reference {
                Some(reference) => {
                    check_ref(reference)?;
                    Ok((reference.offset, reference.len))
                }
                None => Ok((NO_STRING, 0)),
            }
        };

        let block_words = self.blocks.len().checked_mul(BLOCK_WORDS).ok_or_else(invalid)?;
        let spec_words = self.specs.len().checked_mul(SPEC_WORDS).ok_or_else(invalid)?;
        id(block_words)?;
        id(spec_words)?;
        id(self.blocks.len())?;
        id(self.specs.len())?;
        id(self.strings.len())?;
        let mut tables = CommandTables {
            blocks: Vec::with_capacity(block_words),
            specs: Vec::with_capacity(spec_words),
            strings: self.strings.clone(),
            output_state: [
                u32::from(self.output_state.available),
                u32::from(self.output_state.limited),
                self.output_state.count,
                self.output_state.bytes,
                self.output_state.queued,
                self.window_started
                    .map_or(0, |started| now.saturating_duration_since(started).as_nanos().min(1_000_000_000) as u32),
            ],
        };

        for block in &self.blocks {
            let (last_output_offset, last_output_len) = optional_ref(block.last_output)?;
            let flags = u32::from(block.conditional)
                | (u32::from(block.has_entity) << 1)
                | (u32::from(block.powered) << 2)
                | (u32::from(block.automatic) << 3)
                | (u32::from(block.condition_met) << 4)
                | (u32::from(block.track_output) << 5)
                | (u32::from(block.update_last_execution) << 6);
            tables.blocks.extend([
                block.component,
                block.spec,
                match block.mode {
                    CommandBlockMode::Impulse => 0,
                    CommandBlockMode::Repeating => 1,
                    CommandBlockMode::Chain => 2,
                },
                block.facing.get_id(),
                flags,
                block.success_count as u32,
                block.last_execution as u64 as u32,
                (block.last_execution as u64 >> 32) as u32,
                last_output_offset,
                last_output_len,
            ]);
        }

        for spec in &self.specs {
            let (result, recipient, recipient_offset, recipient_len, message_offset, message_len, error_offset, error_len) = match spec.result {
                CommandResult::Empty => (0, 0, NO_STRING, 0, NO_STRING, 0, NO_STRING, 0),
                CommandResult::Success { recipient, message } => {
                    check_ref(message)?;
                    let (recipient, recipient_offset, recipient_len) = match recipient {
                        RecipientKind::All => (1, NO_STRING, 0),
                        RecipientKind::Name(name) => {
                            check_ref(name)?;
                            (2, name.offset, name.len)
                        }
                        RecipientKind::Nobody => (3, NO_STRING, 0),
                    };
                    (1, recipient, recipient_offset, recipient_len, message.offset, message.len, NO_STRING, 0)
                }
                CommandResult::Error(error) => {
                    check_ref(error)?;
                    (2, 0, NO_STRING, 0, NO_STRING, 0, error.offset, error.len)
                }
            };
            tables.specs.extend([
                spec.command_len,
                spec.source_len,
                result,
                recipient,
                recipient_offset,
                recipient_len,
                message_offset,
                message_len,
                error_offset,
                error_len,
            ]);
        }
        Ok(tables)
    }
}

fn invalid() -> ScratchError {
    ScratchError {
        position: None,
        reason: "native command data exceeds the ABI limit",
    }
}

fn id(value: usize) -> Result<u32, ScratchError> {
    u32::try_from(value).map_err(|_| invalid())
}

fn add_string(strings: &mut Vec<u8>, value: &str) -> Result<StringRef, ScratchError> {
    let offset = id(strings.len())?;
    let len = id(value.len())?;
    let end = strings.len().checked_add(value.len()).ok_or_else(invalid)?;
    id(end)?;
    strings.extend_from_slice(value.as_bytes());
    Ok(StringRef { offset, len })
}

fn recipient(command: ChatCommand, strings: &mut Vec<u8>) -> Result<(RecipientKind, StringRef), ScratchError> {
    let recipient = match command.recipient {
        Recipient::All => RecipientKind::All,
        Recipient::Name(name) => RecipientKind::Name(add_string(strings, &name)?),
        Recipient::Nobody => RecipientKind::Nobody,
    };
    let message = add_string(strings, &command.message)?;
    Ok((recipient, message))
}

fn spec(entity: &CommandBlockEntity, strings: &mut Vec<u8>) -> Result<CommandSpec, ScratchError> {
    let command_len = id(entity.command.len())?;
    let source = crate::redstone::command_block::command_source(&entity.custom_name);
    let source_len = id(source.len())?;
    let result = if entity.command.trim().is_empty() {
        CommandResult::Empty
    } else if command_len > 131_068 || source_len > 256 {
        CommandResult::Error(add_string(strings, crate::messages::COMMAND_BLOCK_OUTPUT_LIMIT)?)
    } else {
        match crate::chat_commands::parse(&entity.command, &source, None) {
            Ok(command) => {
                let (recipient, message) = recipient(command, strings)?;
                CommandResult::Success { recipient, message }
            }
            Err(error) => CommandResult::Error(add_string(strings, &error)?),
        }
    };
    Ok(CommandSpec {
        command_len,
        source_len,
        result,
    })
}

fn mode(block: Block) -> CommandBlockMode {
    match block.get_name() {
        "repeating_command_block" => CommandBlockMode::Repeating,
        "chain_command_block" => CommandBlockMode::Chain,
        _ => CommandBlockMode::Impulse,
    }
}

/// Encode every command component in canonical Program order, including blocks
/// without a saved entity. Parse outcomes become native bytecode rather than
/// causing unsupported commands to disappear or reject the Program.
pub(super) fn encode(program: &Program) -> Result<EncodedCommands, ScratchError> {
    let mut encoded = EncodedCommands::default();
    let mut entity_count = 0usize;
    for pos in program.command_blocks.keys() {
        let Some(&component) = program.component_indices.get(pos) else {
            return Err(ScratchError {
                position: Some(*pos),
                reason: "native command entity has no command component",
            });
        };
        if !program.initial[component].is_command_block() {
            return Err(ScratchError {
                position: Some(*pos),
                reason: "native command entity has no command component",
            });
        }
    }
    for (component, (&pos, &block)) in program.positions.iter().zip(&program.initial).enumerate() {
        if !block.is_command_block() {
            continue;
        }
        let component = id(component)?;
        let spec_index = id(encoded.specs.len())?;
        let has_entity = program.command_blocks.get(&pos);
        entity_count += usize::from(has_entity.is_some());
        let entity = has_entity.map_or_else(CommandBlockEntity::default, |entity| (**entity).clone());
        let record = CommandBlockRecord {
            component,
            spec: spec_index,
            mode: mode(block),
            facing: block.facing().unwrap_or(BlockFace::North),
            conditional: block.conditional() == Some(true),
            has_entity: has_entity.is_some(),
            powered: entity.powered,
            automatic: entity.automatic,
            condition_met: entity.condition_met,
            track_output: entity.track_output,
            update_last_execution: entity.update_last_execution,
            success_count: entity.success_count,
            last_execution: entity.last_execution,
            last_output: entity
                .last_output
                .as_deref()
                .map(|value| add_string(&mut encoded.strings, value))
                .transpose()?,
        };
        encoded.specs.push(spec(&entity, &mut encoded.strings)?);
        encoded.blocks.push(record);
    }

    if entity_count != program.command_blocks.len() {
        return Err(ScratchError {
            position: None,
            reason: "native command entity has no command component",
        });
    }
    id(encoded.blocks.len())?;
    id(encoded.specs.len())?;
    id(encoded.strings.len())?;
    encoded.output_state = match &program.command_output_state {
        Some(state) => EncodedOutputState {
            available: true,
            limited: state.limited,
            count: id(state.count)?,
            bytes: id(state.bytes)?,
            queued: id(state.queued)?,
        },
        None => EncodedOutputState::default(),
    };
    encoded.window_started = program.command_output_state.as_ref().map(|state| state.window);
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(blocks: &[(&str, &str, &str)]) -> Program {
        let positions: Vec<_> = (0..blocks.len()).map(|index| BlockPos::new(index as i32, 20, 30)).collect();
        let initial: Vec<_> = blocks.iter().map(|(name, _, _)| Block::from_name(name).unwrap()).collect();
        let mut program = Program::new(
            positions.clone(),
            initial,
            vec![],
            vec![[vec![], vec![]]; blocks.len()],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        for index in (0..blocks.len()).rev() {
            let (_, command, custom_name) = blocks[index];
            let mut entity = CommandBlockEntity::default();
            entity.command = (*command).to_owned();
            entity.custom_name = (*custom_name).to_owned();
            program.command_blocks.insert(positions[index], Box::new(entity));
        }
        program
    }

    fn text(encoded: &EncodedCommands, value: StringRef) -> &str {
        std::str::from_utf8(&encoded.strings[value.offset as usize..(value.offset + value.len) as usize]).unwrap()
    }

    #[test]
    fn encodes_supported_say_and_tellraw_as_owned_bytecode() {
        let program = program(&[
            ("command_block", "say hello", "\"Nora\""),
            ("repeating_command_block", "/tellraw @s {\"text\":\"ready\"}", "\"@\""),
            ("chain_command_block", "tellraw Nora {\"text\":\"hi\"}", "\"@\""),
        ]);
        let encoded = encode(&program).unwrap();
        assert_eq!(encoded.blocks.len(), 3);
        assert_eq!(encoded.blocks[0].component, 0);
        assert_eq!(encoded.blocks[1].mode, CommandBlockMode::Repeating);
        assert_eq!(encoded.blocks[2].mode, CommandBlockMode::Chain);
        assert_eq!(encoded.specs[0].command_len, "say hello".len() as u32);
        assert_eq!(encoded.specs[0].source_len, "Nora".len() as u32);
        let CommandResult::Success { recipient, message } = encoded.specs[0].result else {
            panic!()
        };
        assert_eq!(recipient, RecipientKind::All);
        assert_eq!(text(&encoded, message), r#"{"text":"[Nora] hello"}"#);
        let CommandResult::Success { recipient, message } = encoded.specs[1].result else {
            panic!()
        };
        assert_eq!(recipient, RecipientKind::Nobody);
        assert_eq!(text(&encoded, message), r#"{"text":"ready"}"#);
        let CommandResult::Success { recipient, message } = encoded.specs[2].result else {
            panic!()
        };
        let RecipientKind::Name(name) = recipient else { panic!() };
        assert_eq!(text(&encoded, name), "Nora");
        assert_eq!(text(&encoded, message), r#"{"text":"hi"}"#);
    }

    #[test]
    fn keeps_parser_errors_and_empty_commands_instead_of_dropping_them() {
        let program = program(&[("command_block", "stop", "\"@\""), ("chain_command_block", "   ", "\"@\"")]);
        let encoded = encode(&program).unwrap();
        let CommandResult::Error(error) = encoded.specs[0].result else {
            panic!()
        };
        assert_eq!(text(&encoded, error), crate::messages::ONLY_TELLRAW_SAY_SUPPORTED_COMMAND_BLOCKS);
        assert_eq!(encoded.specs[1].result, CommandResult::Empty);
    }

    #[test]
    fn encodes_default_entity_for_command_block_without_saved_entity() {
        let block = Block::from_name("command_block").unwrap();
        let program = Program::new(
            vec![BlockPos::new(1, 2, 3)],
            vec![block],
            vec![],
            vec![[vec![], vec![]]],
            vec![],
            vec![],
            vec![],
        )
        .unwrap();
        let encoded = encode(&program).unwrap();
        let record = &encoded.blocks[0];
        assert!(!record.has_entity);
        assert!(record.track_output);
        assert!(record.update_last_execution);
        assert_eq!(record.last_execution, -1);
        assert_eq!(encoded.specs[0].result, CommandResult::Empty);
    }

    #[test]
    fn serializes_fixed_width_rows_offsets_and_initial_limit_state() {
        let mut program = program(&[("command_block", "tellraw @a {\"text\":\"ok\"}", "\"@\"")]);
        let window = std::time::Instant::now() - std::time::Duration::from_secs(2);
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window,
            count: 63,
            bytes: 1234,
            queued: 7,
            limited: true,
        });
        let encoded = encode(&program).unwrap();
        let tables = encoded.serialize_at(window + std::time::Duration::from_secs(3)).unwrap();
        assert_eq!(tables.blocks.len(), 10);
        assert_eq!(tables.specs.len(), 10);
        assert_eq!(tables.blocks[0], 0);
        assert_eq!(tables.specs[2], 1);
        assert_eq!(tables.specs[3], 1);
        let message_offset = tables.specs[6] as usize;
        let message_len = tables.specs[7] as usize;
        assert_eq!(&tables.strings[message_offset..message_offset + message_len], br#"{"text":"ok"}"#);
        assert_eq!(tables.output_state, [1, 1, 63, 1234, 7, 1_000_000_000]);
    }

    #[test]
    fn preserves_static_parse_error_when_output_execution_is_unavailable() {
        let program = program(&[("command_block", "stop", "\"@\"")]);
        let encoded = encode(&program).unwrap();
        assert!(!encoded.output_state.available);
        let CommandResult::Error(error) = encoded.specs[0].result else {
            panic!()
        };
        let tables = encoded.serialize_at(std::time::Instant::now()).unwrap();
        assert_eq!(tables.output_state[0], 0);
        let offset = tables.specs[8] as usize;
        let len = tables.specs[9] as usize;
        assert_eq!(
            &tables.strings[offset..offset + len],
            crate::messages::ONLY_TELLRAW_SAY_SUPPORTED_COMMAND_BLOCKS.as_bytes()
        );
        assert_eq!(text(&encoded, error), crate::messages::ONLY_TELLRAW_SAY_SUPPORTED_COMMAND_BLOCKS);
    }

    #[test]
    fn stores_output_limit_error_without_parsing_an_oversized_command() {
        let oversized = "x".repeat(131_069);
        let program = program(&[("command_block", &oversized, "\"@\"")]);
        let encoded = encode(&program).unwrap();
        let CommandResult::Error(error) = encoded.specs[0].result else {
            panic!()
        };
        assert_eq!(text(&encoded, error), crate::messages::COMMAND_BLOCK_OUTPUT_LIMIT);
    }

    #[test]
    fn encodes_all_three_fpu_tellraw_specs_and_orders_by_program_component() {
        let commands = [
            "/tellraw @a[distance=..300] [\"\",{\"text\":\"Inf\",\"bold\":true,\"color\":\"red\"}]",
            "/tellraw @a[distance=..300] [\"\",{\"text\":\"div by zero\",\"bold\":true,\"color\":\"red\"}]",
            "/tellraw @a[distance=..300] [\"\",{\"text\":\"NaN\",\"bold\":true,\"color\":\"red\"}]",
        ];
        let program = program(&[
            ("command_block", commands[0], "@"),
            ("command_block", commands[1], "@"),
            ("command_block", commands[2], "@"),
        ]);
        let encoded = encode(&program).unwrap();
        assert_eq!(encoded.blocks.iter().map(|block| block.component).collect::<Vec<_>>(), [0, 1, 2]);
        let expected = ["Inf", "div by zero", "NaN"];
        for (spec, text_value) in encoded.specs.iter().zip(expected) {
            let CommandResult::Success {
                recipient: RecipientKind::All,
                message,
            } = spec.result
            else {
                panic!()
            };
            let normalized: serde_json::Value = serde_json::from_str(text(&encoded, message)).unwrap();
            assert_eq!(normalized["text"], "");
            assert_eq!(normalized["extra"][0]["text"], text_value);
            assert_eq!(normalized["extra"][0]["bold"], true);
            assert_eq!(normalized["extra"][0]["color"], "red");
        }
    }
}
