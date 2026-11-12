//! Pointer-free command-block bytecode inputs for the native executor.
use super::{Program, ScratchError};
use crate::chat_commands::{ChatCommand, Recipient};
#[cfg(test)]
use mchprs_blocks::BlockPos;
use mchprs_blocks::{BlockFace, block_entities::CommandBlockEntity, blocks::Block};
#[cfg(any(windows, target_os = "linux"))]
use std::ffi::c_void;

#[cfg(any(windows, target_os = "linux"))]
pub(super) struct Api {
    state_size: unsafe extern "C" fn(u32, u32) -> usize,
    init: unsafe extern "C" fn(*mut c_void, usize, u32, *const c_void, *const u32, u32, *const u32, u32, *const u8, u32, *const u32, u64, u32) -> u32,
    prepare_indices: unsafe extern "C" fn(*mut c_void, *mut u32, u32, *mut u32, u32) -> u32,
    bind: unsafe extern "C" fn(*mut c_void, *mut c_void, *const u32, *const u32, u32) -> u32,
    clock: unsafe extern "C" fn(*mut c_void, u64, u64),
    effect_bound: unsafe extern "C" fn(*const c_void) -> u64,
    effect_buffer: unsafe extern "C" fn(*mut c_void, *mut u32, u32) -> u32,
    read_state: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
    effect_words: unsafe extern "C" fn(*const c_void) -> u32,
    read_effects: unsafe extern "C" fn(*const c_void, *mut u32, u32) -> u32,
    read_output: unsafe extern "C" fn(*const c_void, u64, *mut u32) -> u32,
    ack: unsafe extern "C" fn(*mut c_void, u32, u64),
    refresh_queue: unsafe extern "C" fn(*mut c_void, u32) -> u32,
}

#[cfg(any(windows, target_os = "linux"))]
impl Api {
    /// # Safety
    /// The module must export the generated command API with the signatures declared above;
    /// it must remain loaded until the last call through this API.
    pub(super) unsafe fn load(module: &super::library::Module) -> Result<Self, ScratchError> {
        // SAFETY: The trusted command_engine.c/engine.c exports match these extern C fields; names end in NUL
        // and Library retains module.
        unsafe {
            Ok(Self {
                state_size: module.symbol(b"ne_command_state_size\0")?,
                init: module.symbol(b"ne_command_init\0")?,
                prepare_indices: module.symbol(b"ne_command_prepare_indices\0")?,
                bind: module.symbol(b"ne_bind_commands\0")?,
                clock: module.symbol(b"ne_set_command_clock\0")?,
                effect_bound: module.symbol(b"ne_command_effect_bound\0")?,
                effect_buffer: module.symbol(b"ne_command_set_effect_buffer\0")?,
                read_state: module.symbol(b"ne_command_read_state\0")?,
                effect_words: module.symbol(b"ne_command_effect_words\0")?,
                read_effects: module.symbol(b"ne_command_read_effects\0")?,
                read_output: module.symbol(b"ne_command_read_output_state\0")?,
                ack: module.symbol(b"ne_command_ack_effects\0")?,
                refresh_queue: module.symbol(b"ne_command_refresh_queue\0")?,
            })
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) struct State {
    memory: Vec<u64>,
    tables: CommandTables,
    blocks: Vec<CommandBlockRecord>,
    effects: Vec<u32>,
    started: std::time::Instant,
    pub(super) logical_base: u64,
    #[cfg(feature = "plot-bench")]
    benchmark_clock: bool,
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) struct ClockContext {
    started: std::time::Instant,
    #[cfg(feature = "plot-bench")]
    frozen: bool,
}

#[cfg(any(windows, target_os = "linux"))]
impl ClockContext {
    pub(super) unsafe extern "C" fn read(context: *const c_void) -> u64 {
        // SAFETY: tick_batch passes a live immutable ClockContext until the synchronous native call returns.
        let context = unsafe { &*context.cast::<Self>() };
        #[cfg(feature = "plot-bench")]
        if context.frozen {
            return 0;
        }
        context.started.elapsed().as_nanos().min(u64::MAX as u128) as u64
    }
}

#[cfg(any(windows, target_os = "linux"))]
impl State {
    pub(super) fn clock_context(&self) -> ClockContext {
        ClockContext {
            started: self.started,
            #[cfg(feature = "plot-bench")]
            frozen: self.benchmark_clock,
        }
    }
    /// Create and bind native command-block state for a program.
    ///
    /// Inputs: The program, command API, native engine pointer and component-table pointer.
    ///
    /// Allocate instance state against embedded command tables and retain only host publication metadata.
    pub(super) fn new(program: &Program, api: &Api, engine: *mut c_void) -> Result<Option<Self>, ScratchError> {
        let encoded = encode(program)?;
        if encoded.blocks.is_empty() {
            return Ok(None);
        }
        let now = std::time::Instant::now();
        let mut tables = encoded.serialize_at(now)?;
        let count = id(encoded.blocks.len())?;
        // SAFETY: ne_command_state_size takes scalar counts only; api belongs to the live runtime library.
        let bytes = unsafe { (api.state_size)(count, 0) };
        if bytes == 0 || bytes > 256 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut memory = crate::redpilator::memory::zeroed::<u64>(bytes.div_ceil(8))?;
        // SAFETY: The generated module owns all command/component tables; output_state has six words,
        // and the instance allocation covers the checked native size.
        let status = unsafe {
            (api.init)(
                memory.as_mut_ptr().cast(),
                memory.len() * 8,
                id(program.initial.len())?,
                std::ptr::null(),
                std::ptr::null(),
                count,
                std::ptr::null(),
                count,
                std::ptr::null(),
                id(tables.strings.len())?,
                tables.output_state.as_ptr(),
                0,
                0,
            )
        };
        if status != 0 {
            return Err(invalid());
        }
        // SAFETY: Generated modules bind their own immutable component/chain indexes.
        let status = unsafe { (api.prepare_indices)(memory.as_mut_ptr().cast(), std::ptr::null_mut(), 0, std::ptr::null_mut(), 0) };
        if status != 0 {
            return Err(invalid());
        }
        // SAFETY: The live generated engine binds its own immutable notification tables.
        let status = unsafe { (api.bind)(engine, memory.as_mut_ptr().cast(), std::ptr::null(), std::ptr::null(), 0) };
        if status != 0 {
            return Err(invalid());
        }
        tables.blocks = Vec::new();
        tables.specs = Vec::new();
        Ok(Some(Self {
            memory,
            tables,
            blocks: encoded.blocks,
            effects: Vec::new(),
            started: now,
            logical_base: 0,
            #[cfg(feature = "plot-bench")]
            benchmark_clock: false,
        }))
    }

    /// Reserve enough effect output space and update the native clock.
    ///
    /// Inputs: The command API, native engine pointer and this command state's effect buffer/clock basis.
    ///
    /// Read the pending effect count and per-tick bound, grow the buffer with checked arithmetic, rebind
    /// it when moved and send the current time/logical base to native code.
    pub(super) fn prepare_tick(&mut self, api: &Api, engine: *mut c_void) -> Result<(), ScratchError> {
        // SAFETY: Self owns initialized command memory and retained tables; the matching library remains
        // loaded.
        let current = unsafe { (api.effect_words)(self.memory.as_ptr().cast()) } as usize;
        // SAFETY: The sole caller passes its initialized engine, with this command state bound and all
        // referenced allocations still live.
        let additional = usize::try_from(unsafe { (api.effect_bound)(engine) }).map_err(|_| invalid())?;
        let needed = current.checked_add(additional.checked_mul(8).ok_or_else(invalid)?).ok_or_else(invalid)?;
        if needed > self.effects.len() {
            let length = needed.checked_next_power_of_two().ok_or_else(invalid)?;
            let capacity = id(length / 8)?;
            self.effects.try_reserve(length - self.effects.len()).map_err(|_| invalid())?;
            self.effects.resize(length, 0);
            // SAFETY: The writable allocation contains capacity * 8 words; C retains it, and each later growth
            // rebinds before native execution.
            if unsafe { (api.effect_buffer)(self.memory.as_mut_ptr().cast(), self.effects.as_mut_ptr(), capacity) } != 0 {
                return Err(invalid());
            }
        }
        // SAFETY: The sole caller supplies the initialized engine under an exclusive runtime borrow; clock only
        // updates scalar time fields.
        unsafe {
            #[cfg(feature = "plot-bench")]
            let now = if self.benchmark_clock {
                0
            } else {
                self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64
            };
            #[cfg(not(feature = "plot-bench"))]
            let now = self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            (api.clock)(engine, now, self.logical_base);
        }
        Ok(())
    }

    pub(super) fn grow_batch_effects(&mut self, api: &Api, engine: *mut c_void) -> Result<(), ScratchError> {
        let previous = self.effects.len();
        self.prepare_tick(api, engine)?;
        if self.effects.len() <= previous {
            return Err(invalid());
        }
        Ok(())
    }

    #[cfg(feature = "plot-bench")]
    pub(super) fn benchmark_prepare(&mut self, api: &Api, engine: *mut c_void) -> Result<(), ScratchError> {
        if self.tables.output_state[1] != 0 {
            return Err(ScratchError {
                position: None,
                reason: "batch experiment requires unlimited command output",
            });
        }
        if !self.benchmark_clock {
            // shortcut: bounded experiment buffer; overflow fails the native tick instead of dropping effects.
            const WORDS: usize = 65_536 * 8;
            if self.effects.len() < WORDS {
                self.effects.try_reserve(WORDS - self.effects.len()).map_err(|_| invalid())?;
                self.effects.resize(WORDS, 0);
                // SAFETY: State owns this writable buffer; it stays bound until State is dropped or resized.
                if unsafe { (api.effect_buffer)(self.memory.as_mut_ptr().cast(), self.effects.as_mut_ptr(), id(self.effects.len() / 8)?) } != 0 {
                    return Err(invalid());
                }
            }
            self.benchmark_clock = true;
        }
        // SAFETY: Same live initialized engine as prepare_tick; benchmark output limits are disabled.
        unsafe { (api.clock)(engine, 0, self.logical_base) };
        Ok(())
    }

    #[cfg(feature = "plot-bench")]
    pub(super) fn benchmark_snapshot(&self, api: &Api) -> Result<Vec<u32>, ScratchError> {
        let mut state = vec![0; self.blocks.len().checked_mul(11).ok_or_else(invalid)?];
        // SAFETY: Sizes match the retained native command records and C verifies them.
        if unsafe { (api.read_state)(self.memory.as_ptr().cast(), state.as_mut_ptr(), id(state.len())?) } != 0 {
            return Err(invalid());
        }
        let words = unsafe { (api.effect_words)(self.memory.as_ptr().cast()) };
        let mut effects = vec![0; words as usize];
        // SAFETY: Read-only calls with exactly sized writable output allocations.
        if unsafe { (api.read_effects)(self.memory.as_ptr().cast(), effects.as_mut_ptr(), words) } != 0 {
            return Err(invalid());
        }
        let mut output = [0u32; 6];
        if unsafe { (api.read_output)(self.memory.as_ptr().cast(), 0, output.as_mut_ptr()) } != 0 {
            return Err(invalid());
        }
        // Unlimited replay ignores window age; its initial value includes untimed setup wall time.
        output[5] = 0;
        state.push(words);
        state.extend(effects);
        state.extend(output);
        Ok(state)
    }

    fn text(&self, offset: u32, len: u32) -> Result<String, ScratchError> {
        let end = (offset as usize).checked_add(len as usize).ok_or_else(invalid)?;
        let bytes = self.tables.strings.get(offset as usize..end).ok_or_else(invalid)?;
        std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| invalid())
    }

    /// Copy checked native command state and effects into the Rust projection.
    ///
    /// Inputs: The command API and mutable Rust runtime.
    ///
    /// Read entity records, validate component identities/flags and string ranges, rebuild command
    /// entities and append decoded emissions and output-window state.
    pub(super) fn sync(&self, api: &Api, runtime: &mut crate::redpilator::sampled::Runtime) -> Result<(), ScratchError> {
        self.sync_scope(api, runtime, true)
    }

    pub(super) fn sync_effects(&self, api: &Api, runtime: &mut crate::redpilator::sampled::Runtime) -> Result<(), ScratchError> {
        self.sync_scope(api, runtime, false)
    }

    fn sync_scope(&self, api: &Api, runtime: &mut crate::redpilator::sampled::Runtime, include_entities: bool) -> Result<(), ScratchError> {
        let mut entities = Vec::new();
        if include_entities {
            let mut words = vec![0; self.blocks.len().checked_mul(11).ok_or_else(invalid)?];
            // SAFETY: The initialized state has blocks.len() records; words holds eleven u32s per record and C
            // checks the exact word count.
            if unsafe { (api.read_state)(self.memory.as_ptr().cast(), words.as_mut_ptr(), id(words.len())?) } != 0 {
                return Err(invalid());
            }
            entities = Vec::with_capacity(self.blocks.len());
            for (record, row) in self.blocks.iter().zip(words.chunks_exact(11)) {
                if row[0] != record.component || row[1] & !127 != 0 {
                    return Err(invalid());
                }
                let pos = *runtime.program.positions.get(row[0] as usize).ok_or_else(invalid)?;
                if row[1] & 2 == 0 {
                    entities.push((pos, None));
                    continue;
                }
                let mut entity = runtime.command_blocks.get(&pos).cloned().unwrap_or_default();
                entity.powered = row[1] & 4 != 0;
                entity.automatic = row[1] & 8 != 0;
                entity.condition_met = row[1] & 16 != 0;
                entity.track_output = row[1] & 32 != 0;
                entity.update_last_execution = row[1] & 64 != 0;
                entity.success_count = row[2] as i32;
                entity.last_execution = (u64::from(row[3]) | (u64::from(row[4]) << 32)) as i64;
                entity.last_output = match row[5] {
                    0 if row[6] == u32::MAX && row[7] == 0 => None,
                    0 => Some(self.text(row[6], row[7])?),
                    1 => Some(serde_json::json!({"text": crate::messages::COMMAND_BLOCK_EXECUTED}).to_string()),
                    2 => {
                        let error = match row[8] {
                            1 => crate::messages::COMMAND_BLOCK_EXECUTION_UNAVAILABLE.to_owned(),
                            2 => crate::messages::COMMAND_BLOCK_OUTPUT_LIMIT.to_owned(),
                            3 => crate::messages::COMMAND_BLOCK_OUTPUT_BYTE_LIMIT.to_owned(),
                            4 => self.text(row[9], row[10])?,
                            _ => return Err(invalid()),
                        };
                        Some(serde_json::json!({"text": error, "color": "red"}).to_string())
                    }
                    _ => return Err(invalid()),
                };
                entities.push((pos, Some(entity)));
            }
        }
        // SAFETY: Self retains initialized command memory; this query only reads its effect count.
        let count = unsafe { (api.effect_words)(self.memory.as_ptr().cast()) } as usize;
        if count % 8 != 0 || count > self.effects.len() {
            return Err(invalid());
        }
        let mut effect_words = vec![0; count];
        if count != 0 {
            // SAFETY: count is checked against the retained effect buffer; the separate output has count u32s
            // and C checks its exact length.
            if unsafe { (api.read_effects)(self.memory.as_ptr().cast(), effect_words.as_mut_ptr(), id(count)?) } != 0 {
                return Err(invalid());
            }
        }
        let mut emissions = Vec::with_capacity(count / 8);
        for row in effect_words.chunks_exact(8) {
            let pos = *runtime.program.positions.get(row[0] as usize).ok_or_else(invalid)?;
            if !runtime.program.initial[row[0] as usize].is_command_block() {
                return Err(invalid());
            }
            let recipient = match row[3] {
                1 => Recipient::All,
                2 => Recipient::Name(self.text(row[4], row[5])?),
                3 => Recipient::Nobody,
                _ => return Err(invalid()),
            };
            emissions.push(crate::world::CommandBlockEmission {
                pos,
                tick: u64::from(row[1]) | (u64::from(row[2]) << 32),
                message: ChatCommand {
                    recipient,
                    message: self.text(row[6], row[7])?,
                },
            });
        }
        // Queue acknowledgements and window age can change even when there are no message effects.
        let mut output = [0; 6];
        // SAFETY: The command state is initialized and retained; output is a separate writable array of the six
        // u32s required by C.
        if unsafe {
            (api.read_output)(
                self.memory.as_ptr().cast(),
                self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                output.as_mut_ptr(),
            )
        } != 0
            || output[0] > 1
            || output[1] > 1
            || output[5] > 1_000_000_000
        {
            return Err(invalid());
        }
        let output = if output[0] == 0 {
            None
        } else {
            let now = std::time::Instant::now();
            Some(crate::chat_commands::CommandBlockOutputState {
                window: now.checked_sub(std::time::Duration::from_nanos(u64::from(output[5]))).unwrap_or(now),
                limited: output[1] != 0,
                count: output[2] as usize,
                bytes: output[3] as usize,
                queued: output[4] as usize,
            })
        };
        for (pos, entity) in entities {
            if let Some(entity) = entity {
                runtime.command_blocks.insert(pos, entity);
            } else {
                runtime.command_blocks.remove(&pos);
            }
        }
        runtime.command_emissions = emissions;
        runtime.command_output_state = output;
        Ok(())
    }

    pub(super) fn refresh_queue(&mut self, api: &Api, queued: usize) -> Result<(), ScratchError> {
        // SAFETY: The exclusive state owns its pending effects; this updates only queue accounting.
        let status = unsafe { (api.refresh_queue)(self.memory.as_mut_ptr().cast(), id(queued)?) };
        if status == 0 { Ok(()) } else { Err(invalid()) }
    }

    pub(super) fn acknowledge(&mut self, api: &Api, queued: usize) {
        // SAFETY: Self exclusively borrows initialized command memory; C changes only counters, and the
        // remaining arguments are scalars.
        unsafe {
            (api.ack)(
                self.memory.as_mut_ptr().cast(),
                queued.min(u32::MAX as usize) as u32,
                self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            );
        }
    }
}

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
    ///
    /// Inputs: The encoded command set and the reference Instant used for cooldown timing.
    ///
    /// Assign checked string offsets, serialize block/specification rows and output state, and translate
    /// host times relative to the supplied clock basis.
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

/// Parse and encode one command block's command result.
///
/// Inputs: The command-block entity and shared string buffer.
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
///
/// Inputs: The program's component positions, command blocks and captured output state.
///
/// Walk command components in program order, supply default entities when missing, parse each command and
/// retain its specification and mutable captured state.
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

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn drained_host_queue_recovers_without_discarding_native_completion_messages() {
        use mchprs_world::{TickEntry, TickPriority};
        let mut program = program(&[("repeating_command_block", "say shut", "\"@\"")]);
        let pos = program.positions[0];
        let entity = program.command_blocks.get_mut(&pos).unwrap();
        entity.automatic = true;
        entity.condition_met = true;
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window: std::time::Instant::now(),
            count: 0,
            bytes: 0,
            queued: crate::chat_commands::COMMAND_OUTPUT_QUEUE_LIMIT,
            limited: true,
        });
        program.prepare_projection();
        let ticks = [TickEntry {
            pos,
            block_type: Some(program.initial[0].registry_id()),
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
        }];
        let mut native = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        native.tick().unwrap();
        native.refresh_command_queue(0).unwrap();
        native.tick().unwrap();
        native.refresh_command_queue(0).unwrap();
        native.tick().unwrap();
        let mut projection = program.into_runtime_prepared();
        native.sync_effects(&mut projection).unwrap();
        assert_eq!(projection.command_emissions.len(), 2, "queue refresh must retain unexported messages");
        assert!(projection.command_emissions.iter().all(|m| m.message.message.contains("shut")));
        assert_eq!(projection.command_output_state.as_ref().unwrap().queued, 2);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_io_and_effects_export_commands_without_entities_and_keep_empty_queue_quota_state() {
        use mchprs_world::{TickEntry, TickPriority};
        unsafe extern "C" fn reject_read(_: *const c_void, _: *mut u32, _: u32) -> u32 {
            1
        }
        let mut program = program(&[("repeating_command_block", "say retained", "\"@\"")]);
        let pos = program.positions[0];
        let entity = program.command_blocks.get_mut(&pos).unwrap();
        entity.automatic = true;
        entity.condition_met = true;
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window: std::time::Instant::now(),
            count: 0,
            bytes: 0,
            queued: 5,
            limited: true,
        });
        program.prepare_projection();
        let initial_entities = program.command_blocks.clone();
        let ticks = [TickEntry {
            pos,
            block_type: Some(program.initial[0].registry_id()),
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
        }];
        let mut projection = program.clone().into_runtime_prepared();
        let mut native = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        native.tick().unwrap();
        let read_entities = native.library.commands.read_state;
        native.library.commands.read_state = reject_read;
        native.sync_io(&mut projection).unwrap();
        let emissions = |runtime: &crate::redpilator::sampled::Runtime| {
            runtime
                .command_emissions
                .iter()
                .map(|event| (event.pos, event.tick, event.message.recipient.clone(), event.message.message.clone()))
                .collect::<Vec<_>>()
        };
        let messages = emissions(&projection);
        assert!(!messages.is_empty());
        assert_eq!(projection.command_blocks, initial_entities);
        let output = projection.command_output_state.as_ref().unwrap();
        let count = output.count;
        let bytes = output.bytes;
        assert_eq!(count, messages.len());
        assert_eq!(
            bytes,
            projection
                .command_emissions
                .iter()
                .map(|event| event.message.message.len())
                .sum::<usize>()
        );
        assert_eq!(output.queued, 5 + messages.len());
        assert!(output.limited);
        native.sync_effects(&mut projection).unwrap();
        assert_eq!(emissions(&projection), messages, "unacknowledged effects must not duplicate");
        assert_eq!(projection.command_blocks, initial_entities);

        native.acknowledge_effects(17);
        native.library.commands.read_effects = reject_read;
        native.sync_effects(&mut projection).unwrap();
        native.sync_io(&mut projection).unwrap();
        assert!(projection.command_emissions.is_empty());
        let output = projection.command_output_state.as_ref().unwrap();
        assert_eq!((output.count, output.bytes, output.queued, output.limited), (count, bytes, 17, true));
        native.library.commands.read_state = read_entities;
        native.sync_redstone(&mut projection).unwrap();
        assert_ne!(
            projection.command_blocks, initial_entities,
            "Redstone must synchronize executed command entities"
        );
        let mut full = program.clone().into_runtime_prepared();
        native.sync_projection(&mut full).unwrap();
        assert_eq!(projection.command_blocks, full.command_blocks);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_tick_batches_preserve_repeating_commands_and_retry_capacity() {
        use mchprs_world::{TickEntry, TickPriority};
        let mut program = program(&[("repeating_command_block", "say retained", "\"@\"")]);
        let pos = program.positions[0];
        let entity = program.command_blocks.get_mut(&pos).unwrap();
        entity.automatic = true;
        entity.condition_met = true;
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window: std::time::Instant::now(),
            count: 0,
            bytes: 0,
            queued: 0,
            limited: false,
        });
        program.prepare_projection();
        let ticks = vec![TickEntry {
            pos,
            block_type: Some(program.initial[0].registry_id()),
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
        }];
        for count in 0..=crate::redpilator::MAX_LLVM_BATCH {
            let mut scalar = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
            let mut batched = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
            for _ in 0..count {
                scalar.tick().unwrap();
            }
            let mut completed = 99;
            batched.tick_batch(count, &mut completed).unwrap();
            assert_eq!(completed, count);
            let mut a = program.clone().into_runtime_prepared();
            let mut b = program.clone().into_runtime_prepared();
            scalar.sync_projection(&mut a).unwrap();
            batched.sync_projection(&mut b).unwrap();
            assert_eq!(a.command_blocks, b.command_blocks, "count {count}");
            let emissions = |r: &crate::redpilator::sampled::Runtime| {
                r.command_emissions
                    .iter()
                    .map(|e| (e.tick, e.message.message.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(emissions(&a), emissions(&b), "count {count}");
            assert_eq!(scalar.pending_entries().unwrap(), batched.pending_entries().unwrap());
            let snapshot = |native: &super::super::LlvmRuntime| {
                let engine = native.state.as_ptr().cast();
                let words = unsafe { (native.library.snapshot_words)(engine) };
                let mut state = vec![0; words as usize];
                assert_eq!(unsafe { (native.library.read_snapshot)(engine, state.as_mut_ptr(), words) }, 0);
                state
            };
            assert_eq!(snapshot(&scalar), snapshot(&batched), "native state count {count}");
        }
        let mut native = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        let engine = native.state.as_mut_ptr().cast();
        let bound = unsafe { (native.library.commands.effect_bound)(engine) } as usize;
        let commands = native.commands.as_mut().unwrap();
        commands.effects.resize(bound * 8, 0);
        assert_eq!(
            unsafe { (native.library.commands.effect_buffer)(commands.memory.as_mut_ptr().cast(), commands.effects.as_mut_ptr(), bound as u32) },
            0
        );
        unsafe extern "C" fn zero(_: *const c_void) -> u64 {
            0
        }
        let mut prefix = 99;
        assert_eq!(
            unsafe { (native.library.tick_batch)(engine, 16, Some(zero), std::ptr::null(), 0, &mut prefix) },
            5
        );
        assert_eq!(prefix, 1, "capacity request must precede the second tick");
        assert_eq!(unsafe { (native.library.tick_count)(engine) }, 1);
        let mut resumed = 99;
        native.tick_batch(15, &mut resumed).unwrap();
        assert_eq!(resumed, 15);
        let mut projection = program.into_runtime_prepared();
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(projection.command_emissions.len(), 16);
        assert_eq!(
            projection.command_emissions.iter().map(|e| e.tick).collect::<Vec<_>>(),
            (1..=16).collect::<Vec<_>>()
        );
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_tick_batch_stops_after_first_runtime_error_and_preserves_completed_prefix() {
        use mchprs_world::{TickEntry, TickPriority};
        let mut program = program(&[("repeating_command_block", "say retained", "\"@\"")]);
        let pos = program.positions[0];
        let entity = program.command_blocks.get_mut(&pos).unwrap();
        entity.automatic = true;
        entity.condition_met = true;
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window: std::time::Instant::now(),
            count: 0,
            bytes: 0,
            queued: u32::MAX as usize - 1,
            limited: false,
        });
        program.prepare_projection();
        let ticks = vec![TickEntry {
            pos,
            block_type: Some(program.initial[0].registry_id()),
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
        }];
        let mut scalar = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        let mut batched = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        scalar.tick().unwrap();
        let scalar_error = scalar.tick().unwrap_err();
        let mut completed = 99;
        let error = batched.tick_batch(16, &mut completed).unwrap_err();
        assert_eq!(completed, 1);
        assert_eq!(error.reason, scalar_error.reason);
        assert!(error.reason.contains("capacity"));
        let snapshot = |native: &super::super::LlvmRuntime| {
            let engine = native.state.as_ptr().cast();
            assert_eq!(
                unsafe { (native.library.tick_count)(engine) },
                2,
                "failing tick started; later ticks did not"
            );
            let words = unsafe { (native.library.snapshot_words)(engine) };
            let mut state = vec![0; words as usize];
            assert_eq!(unsafe { (native.library.read_snapshot)(engine, state.as_mut_ptr(), words) }, 0);
            state
        };
        assert_eq!(snapshot(&scalar), snapshot(&batched));
        assert_eq!(scalar.pending_entries().unwrap(), batched.pending_entries().unwrap());
        let mut a = program.clone().into_runtime_prepared();
        let mut b = program.into_runtime_prepared();
        scalar.sync_projection(&mut a).unwrap();
        batched.sync_projection(&mut b).unwrap();
        assert_eq!(a.command_blocks, b.command_blocks);
        assert_eq!(a.command_emissions.len(), 1);
        assert_eq!(b.command_emissions.len(), 1);
        assert_eq!(a.command_emissions[0].tick, b.command_emissions[0].tick);
        completed = 99;
        assert!(batched.tick_batch(16, &mut completed).is_err());
        assert_eq!(completed, 0, "sticky failure must not restart ticks");
        assert_eq!(unsafe { (batched.library.tick_count)(batched.state.as_ptr().cast()) }, 2);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_tick_batch_refreshes_live_output_window_for_each_internal_tick() {
        use mchprs_world::{TickEntry, TickPriority};
        let mut program = program(&[("repeating_command_block", "say retained", "\"@\"")]);
        let pos = program.positions[0];
        let entity = program.command_blocks.get_mut(&pos).unwrap();
        entity.automatic = true;
        entity.condition_met = true;
        program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
            window: std::time::Instant::now() + std::time::Duration::from_secs(3600),
            count: 64,
            bytes: 0,
            queued: 0,
            limited: true,
        });
        program.prepare_projection();
        let ticks = vec![TickEntry {
            pos,
            block_type: Some(program.initial[0].registry_id()),
            ticks_left: 1,
            tick_priority: TickPriority::Normal,
        }];
        let mut native = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
        let engine = native.state.as_mut_ptr().cast();
        native.commands.as_mut().unwrap().prepare_tick(&native.library.commands, engine).unwrap();
        // Callback records invocations in independent test-only storage; it never touches engine state.
        let calls = std::cell::Cell::new(0u64);
        unsafe extern "C" fn clock(context: *const c_void) -> u64 {
            let calls = unsafe { &*context.cast::<std::cell::Cell<u64>>() };
            let value = calls.get();
            calls.set(value + 1);
            value * 1_000_000_000
        }
        let mut completed = 99;
        assert_eq!(
            unsafe { (native.library.tick_batch)(engine, 2, Some(clock), (&calls as *const std::cell::Cell<u64>).cast(), 0, &mut completed) },
            0
        );
        assert_eq!(completed, 2);
        assert_eq!(calls.get(), 2);
        let mut projection = program.into_runtime_prepared();
        native.sync_projection(&mut projection).unwrap();
        assert_eq!(
            projection.command_emissions.len(),
            2,
            "both ticks enqueue output while delivery owns the wall-clock limit"
        );
        assert_eq!(projection.command_emissions[0].tick, 1);
        assert_eq!(projection.command_emissions[1].tick, 2);
        assert_eq!(projection.command_output_state.as_ref().unwrap().count, 1);
        completed = 99;
        assert!(native.tick_batch(crate::redpilator::MAX_LLVM_BATCH + 1, &mut completed).is_err());
        assert_eq!(completed, 0);
        assert_eq!(unsafe { (native.library.tick_count)(engine) }, 2);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn llvm_commands_preserve_logical_clock_entities_and_unflushed_output() {
        use mchprs_world::{TickEntry, TickPriority};
        for update_last_execution in [true, false] {
            let mut program = program(&[("repeating_command_block", "say retained", "\"@\"")]);
            let pos = program.positions[0];
            let entity = program.command_blocks.get_mut(&pos).unwrap();
            entity.automatic = true;
            entity.condition_met = true;
            entity.update_last_execution = update_last_execution;
            program.command_output_state = Some(crate::chat_commands::CommandBlockOutputState {
                window: std::time::Instant::now(),
                count: 0,
                bytes: 0,
                queued: 0,
                limited: true,
            });
            program.prepare_projection();
            let ticks = vec![TickEntry {
                pos,
                block_type: Some(program.initial[0].registry_id()),
                ticks_left: 1,
                tick_priority: TickPriority::Normal,
            }];
            let mut rust = program.clone().into_runtime_prepared();
            rust.set_pending(ticks.clone()).unwrap();
            rust.piston_state.logical_tick = 1234;
            let mut projection = program.clone().into_runtime_prepared();
            projection.piston_state.logical_tick = 1234;
            let mut native = super::super::LlvmRuntime::new(&program, &ticks).unwrap();
            native.initialize_tick(1234);
            let emissions = |runtime: &crate::redpilator::sampled::Runtime| {
                runtime
                    .command_emissions
                    .iter()
                    .map(|event| (event.pos, event.tick, event.message.recipient.clone(), event.message.message.clone()))
                    .collect::<Vec<_>>()
            };
            let mut initial_buffer = 0;
            for tick in 1..=1200 {
                rust.tick().unwrap();
                native.tick().unwrap();
                if tick == 1 {
                    initial_buffer = native.commands.as_ref().unwrap().effects.len();
                }
                native.sync_projection(&mut projection).unwrap();
                assert_eq!(
                    projection.command_blocks, rust.command_blocks,
                    "last-execution {update_last_execution} tick {tick}"
                );
                assert_eq!(
                    emissions(&projection),
                    emissions(&rust),
                    "last-execution {update_last_execution} tick {tick}"
                );
                assert_eq!(projection.pending_entries(), rust.pending_entries());
            }
            assert_eq!(projection.command_emissions.first().unwrap().tick, 1235);
            if !update_last_execution {
                assert!(projection.command_emissions.len() > 64);
                assert!(native.commands.as_ref().unwrap().effects.len() > initial_buffer);
            }
            let before = emissions(&projection);
            native.sync_publication(&mut projection).unwrap();
            assert_eq!(emissions(&projection), before, "repeated synchronization duplicated effects");
            native.acknowledge_effects(0);
            native.sync_publication(&mut projection).unwrap();
            assert!(projection.command_emissions.is_empty());
            assert_eq!(projection.command_output_state.as_ref().unwrap().queued, 0);
        }
    }

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
