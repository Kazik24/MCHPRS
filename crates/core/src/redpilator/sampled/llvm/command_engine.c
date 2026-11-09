/*
 * Native command-block state machine. All Program data is read through
 * component/spec indices and byte offsets; no serialized pointers cross the
 * ABI. The engine supplies C-to-C hooks for redstone power, scheduling and
 * notifications. Nothing in this file calls back into Rust.
 *
 * Command row schema: see command.rs. Read-state rows are 11 u32 words and
 * effect rows are 8 u32 words; offsets in both refer to the Program byte pool.
 */
#include "abi.h"

#include <limits.h>

#define NE_CMD_OK 0u
#define NE_CMD_INVALID 1u
#define NE_CMD_CAPACITY 3u
#define NE_CMD_BLOCK_WORDS 10u
#define NE_CMD_SPEC_WORDS 10u
#define NE_CMD_STATE_WORDS 11u
#define NE_CMD_EFFECT_WORDS 8u
#define NE_CMD_NONE UINT32_MAX
#define NE_CMD_MAX_COUNT 64u
#define NE_CMD_MAX_COMMAND_BYTES 131068u
#define NE_CMD_MAX_SOURCE_BYTES 256u
#define NE_CMD_MAX_OUTPUT_BYTES 65536u
#define NE_CMD_WINDOW_NS UINT64_C(1000000000)

#define NE_CMD_CONDITIONAL (1u << 0)
#define NE_CMD_HAS_ENTITY (1u << 1)
#define NE_CMD_POWERED (1u << 2)
#define NE_CMD_AUTOMATIC (1u << 3)
#define NE_CMD_CONDITION_MET (1u << 4)
#define NE_CMD_TRACK_OUTPUT (1u << 5)
#define NE_CMD_UPDATE_LAST_EXECUTION (1u << 6)

enum { NE_CMD_MODE_IMPULSE = 0u, NE_CMD_MODE_REPEATING = 1u, NE_CMD_MODE_CHAIN = 2u };

enum { NE_CMD_RESULT_EMPTY = 0u, NE_CMD_RESULT_SUCCESS = 1u, NE_CMD_RESULT_ERROR = 2u };

enum {
    NE_CMD_RECIPIENT_NONE = 0u,
    NE_CMD_RECIPIENT_ALL = 1u,
    NE_CMD_RECIPIENT_NAME = 2u,
    NE_CMD_RECIPIENT_NOBODY = 3u
};

enum { NE_CMD_LAST_OUTPUT_INITIAL = 0u, NE_CMD_LAST_OUTPUT_EXECUTED = 1u, NE_CMD_LAST_OUTPUT_ERROR = 2u };

enum {
    NE_CMD_ERROR_NONE = 0u,
    NE_CMD_ERROR_EXECUTION_UNAVAILABLE = 1u,
    NE_CMD_ERROR_OUTPUT_LIMIT = 2u,
    NE_CMD_ERROR_OUTPUT_BYTE_LIMIT = 3u,
    NE_CMD_ERROR_PARSE = 4u
};

typedef struct {
    uint32_t flags;
    int32_t success_count;
    int64_t last_execution;
    uint32_t last_output_mode;
    uint32_t last_output_offset;
    uint32_t last_output_len;
    uint32_t last_output_error;
    uint32_t error_offset;
    uint32_t error_len;
} NECommandMutable;

typedef struct {
    const uint32_t *block_rows;
    const uint32_t *spec_rows;
    const uint8_t *strings;
    const MchprsNativeComponent *components;
    uint32_t component_count;
    uint32_t block_count;
    uint32_t spec_count;
    uint32_t string_bytes;
    uint32_t effect_count;
    uint32_t effect_capacity;
    uint8_t output_available;
    uint8_t limited;
    uint32_t output_count;
    uint32_t output_bytes;
    uint32_t output_queued;
    uint64_t window_started_ns;
    uint64_t window_age_at_start_ns;
    NECommandMutable *mutable_blocks;
    uint32_t *effects;
    uint32_t *component_commands;
    uint32_t *previous_next;
} NECommandEngine;

typedef struct {
    void *context;
    uint8_t (*powered)(void *context, uint32_t component);
    uint8_t (*pending)(void *context, uint32_t component);
    uint32_t (*schedule)(void *context, uint32_t component, uint32_t delay, uint8_t priority);
    uint32_t (*notify)(void *context, uint32_t component);
} NECommandHooks;

static uint32_t checked_mul_size(size_t left, size_t right, size_t *out) {
    if (right && left > SIZE_MAX / right)
        return 0u;
    *out = left * right;
    return 1u;
}

static size_t command_align8(size_t value) {
    if (value > SIZE_MAX - 7u)
        return 0u;
    return (value + 7u) & ~(size_t)7u;
}

static void zero_bytes(void *target, size_t length) {
    uint8_t *bytes = (uint8_t *)target;
    for (size_t i = 0; i < length; ++i)
        bytes[i] = 0u;
}

static void copy_bytes(void *target, const void *source, size_t length) {
    uint8_t *destination = (uint8_t *)target;
    const uint8_t *bytes = (const uint8_t *)source;
    for (size_t i = 0; i < length; ++i)
        destination[i] = bytes[i];
}

static void read_i32(uint32_t value, int32_t *out) {
    *out = value <= INT32_MAX ? (int32_t)value : -1 - (int32_t)(UINT32_MAX - value);
}

static void read_i64(uint32_t low, uint32_t high, int64_t *out) {
    uint64_t bits = ((uint64_t)high << 32u) | low;
    *out = bits <= INT64_MAX ? (int64_t)bits : -1 - (int64_t)(UINT64_MAX - bits);
}

static void words_i64(int64_t value, uint32_t *low, uint32_t *high) {
    const uint64_t bits = (uint64_t)value;
    *low = (uint32_t)bits;
    *high = (uint32_t)(bits >> 32u);
}

static uint32_t valid_ref(uint32_t offset, uint32_t length, uint32_t bytes) {
    return offset != NE_CMD_NONE && (uint64_t)offset + length <= bytes;
}

static uint32_t valid_optional_ref(uint32_t offset, uint32_t length, uint32_t bytes) {
    return offset == NE_CMD_NONE ? length == 0u : valid_ref(offset, length, bytes);
}

/**
 * Check serialized command records before native initialization.
 *
 * Inputs: Component count, block/specification tables, strings and output-state metadata.
 *
 * Validate command kinds, component references, string slices, flags and queue/time fields, rejecting
 * any table combination outside the native command contract.
 */
static uint32_t valid_tables(uint32_t component_count, const uint32_t *blocks, uint32_t block_count,
                             const uint32_t *specs, uint32_t spec_count, const uint8_t *strings, uint32_t string_bytes,
                             const uint32_t output_state[6]) {
    if (block_count > UINT32_MAX / NE_CMD_STATE_WORDS || block_count > UINT32_MAX / NE_CMD_BLOCK_WORDS ||
        spec_count > UINT32_MAX / NE_CMD_SPEC_WORDS || (!blocks && block_count) || (!specs && spec_count) ||
        (!strings && string_bytes) || !output_state || block_count != spec_count || output_state[0] > 1u ||
        output_state[1] > 1u || output_state[5] > NE_CMD_WINDOW_NS)
        return 0u;
    if (!output_state[0] &&
        (output_state[1] || output_state[2] || output_state[3] || output_state[4] || output_state[5]))
        return 0u;

    uint32_t previous_component = 0u;
    for (uint32_t i = 0; i < block_count; ++i) {
        const uint32_t *row = blocks + (size_t)i * NE_CMD_BLOCK_WORDS;
        if (row[0] >= component_count || (i && row[0] <= previous_component) || row[1] != i ||
            row[2] > NE_CMD_MODE_CHAIN || row[3] > 5u || (row[4] & ~127u) ||
            !valid_optional_ref(row[8], row[9], string_bytes))
            return 0u;
        previous_component = row[0];
    }

    for (uint32_t i = 0; i < spec_count; ++i) {
        const uint32_t *row = specs + (size_t)i * NE_CMD_SPEC_WORDS;
        const uint32_t result = row[2], recipient = row[3];
        if (result == NE_CMD_RESULT_EMPTY) {
            if (recipient || row[4] != NE_CMD_NONE || row[5] || row[6] != NE_CMD_NONE || row[7] ||
                row[8] != NE_CMD_NONE || row[9])
                return 0u;
        } else if (result == NE_CMD_RESULT_SUCCESS) {
            if (recipient < NE_CMD_RECIPIENT_ALL || recipient > NE_CMD_RECIPIENT_NOBODY ||
                !valid_ref(row[6], row[7], string_bytes) || row[8] != NE_CMD_NONE || row[9])
                return 0u;
            if (recipient == NE_CMD_RECIPIENT_NAME) {
                if (!valid_ref(row[4], row[5], string_bytes))
                    return 0u;
            } else if (row[4] != NE_CMD_NONE || row[5])
                return 0u;
        } else if (result == NE_CMD_RESULT_ERROR) {
            if (recipient || row[4] != NE_CMD_NONE || row[5] || row[6] != NE_CMD_NONE || row[7] ||
                !valid_ref(row[8], row[9], string_bytes))
                return 0u;
        } else
            return 0u;
    }
    return 1u;
}

/**
 * Compute bounded storage for native command state.
 *
 * Inputs: The number of command-block records and requested effect capacity.
 *
 * Use checked size arithmetic for the engine header, mutable entity rows and effect records, returning
 * zero when counts or total size exceed supported bounds.
 */
MCHPRS_NATIVE_EXPORT size_t ne_command_state_size(uint32_t block_count, uint32_t effect_capacity) {
    size_t block_bytes, effect_bytes;
    if (block_count > UINT32_MAX / NE_CMD_STATE_WORDS || effect_capacity > UINT32_MAX / NE_CMD_EFFECT_WORDS ||
        !checked_mul_size(block_count, sizeof(NECommandMutable), &block_bytes) ||
        !checked_mul_size(effect_capacity, NE_CMD_EFFECT_WORDS * sizeof(uint32_t), &effect_bytes))
        return 0u;
    size_t bytes = command_align8(sizeof(NECommandEngine));
    if (!bytes || block_bytes > SIZE_MAX - bytes)
        return 0u;
    bytes += block_bytes;
    bytes = command_align8(bytes);
    if (!bytes || effect_bytes > SIZE_MAX - bytes)
        return 0u;
    return bytes + effect_bytes;
}

/**
 * Initialize native command execution in caller-owned memory.
 *
 * Inputs: The memory region, component/block/specification tables, strings and captured output state.
 *
 * Validate all tables and capacity, partition and clear mutable storage, retain immutable pointers and
 * import each entity's flags, counters and output-window state.
 */
MCHPRS_NATIVE_EXPORT uint32_t ne_command_init(void *memory, size_t memory_bytes, uint32_t component_count,
                                              const MchprsNativeComponent *components, const uint32_t *block_rows,
                                              uint32_t block_count, const uint32_t *spec_rows, uint32_t spec_count,
                                              const uint8_t *strings, uint32_t string_bytes,
                                              const uint32_t output_state[6], uint64_t now_ns,
                                              uint32_t effect_capacity) {
    const size_t needed = ne_command_state_size(block_count, effect_capacity);
    if (!needed || !memory || (uintptr_t)memory % _Alignof(NECommandEngine) || memory_bytes < needed ||
        (!components && component_count) ||
        !valid_tables(component_count, block_rows, block_count, spec_rows, spec_count, strings, string_bytes,
                      output_state))
        return NE_CMD_INVALID;

    uint8_t *base = (uint8_t *)memory;
    NECommandEngine *engine = (NECommandEngine *)base;
    zero_bytes(engine, sizeof(*engine));
    engine->block_rows = block_rows;
    engine->spec_rows = spec_rows;
    engine->strings = strings;
    engine->components = components;
    engine->component_count = component_count;
    engine->block_count = block_count;
    engine->spec_count = spec_count;
    engine->string_bytes = string_bytes;
    engine->output_available = (uint8_t)output_state[0];
    engine->limited = (uint8_t)output_state[1];
    engine->output_count = output_state[2];
    engine->output_bytes = output_state[3];
    engine->output_queued = output_state[4];
    engine->window_started_ns = now_ns;
    engine->window_age_at_start_ns = output_state[5];
    engine->effect_count = 0u;
    engine->effect_capacity = effect_capacity;

    size_t offset = command_align8(sizeof(NECommandEngine));
    engine->mutable_blocks = (NECommandMutable *)(base + offset);
    offset = command_align8(offset + (size_t)block_count * sizeof(NECommandMutable));
    engine->effects = (uint32_t *)(base + offset);
    for (uint32_t i = 0; i < block_count; ++i) {
        const uint32_t *row = block_rows + (size_t)i * NE_CMD_BLOCK_WORDS;
        NECommandMutable *state = &engine->mutable_blocks[i];
        state->flags = row[4];
        read_i32(row[5], &state->success_count);
        read_i64(row[6], row[7], &state->last_execution);
        state->last_output_mode = NE_CMD_LAST_OUTPUT_INITIAL;
        state->last_output_offset = row[8];
        state->last_output_len = row[9];
        state->last_output_error = NE_CMD_ERROR_NONE;
        state->error_offset = NE_CMD_NONE;
        state->error_len = 0u;
    }
    return NE_CMD_OK;
}

static uint32_t command_index(const NECommandEngine *engine, uint32_t component) {
    return engine->component_commands && component < engine->component_count ? engine->component_commands[component]
                                                                             : NE_CMD_NONE;
}

static uint32_t command_at(const NECommandEngine *engine, int64_t x, int64_t y, int64_t z) {
    for (uint32_t i = 0; i < engine->block_count; ++i) {
        uint32_t component = engine->block_rows[(size_t)i * NE_CMD_BLOCK_WORDS];
        const MchprsNativeComponent *pos = &engine->components[component];
        if ((int64_t)pos->x == x && (int64_t)pos->y == y && (int64_t)pos->z == z)
            return i;
    }
    return NE_CMD_NONE;
}

/**
 * Convert an encoded facing into a coordinate offset.
 *
 * Inputs: The facing value and mutable x/y/z output pointers.
 */
static void face_delta(uint32_t face, int32_t *x, int32_t *y, int32_t *z) {
    *x = *y = *z = 0;
    if (face == 0u)
        *y = -1;
    else if (face == 1u)
        *y = 1;
    else if (face == 2u)
        *z = -1;
    else if (face == 3u)
        *z = 1;
    else if (face == 4u)
        *x = -1;
    else if (face == 5u)
        *x = 1;
}

/**
 * Build command component and previous/next chain lookups.
 *
 * Inputs: The initialized command engine and caller-provided index buffers with capacities.
 *
 * Validate buffer sizes, map command component IDs to their rows, then use positions and facing offsets
 * to find each block's predecessor and successor.
 */
MCHPRS_NATIVE_EXPORT uint32_t ne_command_prepare_indices(void *memory, uint32_t *components, uint32_t component_count,
                                                         uint32_t *previous_next, uint32_t words) {
    NECommandEngine *engine = (NECommandEngine *)memory;
    if (!engine || component_count != engine->component_count || words != engine->block_count * 2u ||
        (!components && component_count) || (!previous_next && words))
        return NE_CMD_INVALID;
    for (uint32_t component = 0; component < component_count; ++component)
        components[component] = NE_CMD_NONE;
    for (uint32_t index = 0; index < engine->block_count; ++index) {
        const uint32_t *row = engine->block_rows + (size_t)index * NE_CMD_BLOCK_WORDS;
        components[row[0]] = index;
        const MchprsNativeComponent *pos = &engine->components[row[0]];
        int32_t dx, dy, dz;
        face_delta(row[3], &dx, &dy, &dz);
        previous_next[index * 2u] =
            command_at(engine, (int64_t)pos->x - dx, (int64_t)pos->y - dy, (int64_t)pos->z - dz);
        previous_next[index * 2u + 1u] =
            command_at(engine, (int64_t)pos->x + dx, (int64_t)pos->y + dy, (int64_t)pos->z + dz);
    }
    engine->component_commands = components;
    engine->previous_next = previous_next;
    return NE_CMD_OK;
}

static uint32_t conditional_met(const NECommandEngine *engine, uint32_t index) {
    const uint32_t *row = engine->block_rows + (size_t)index * NE_CMD_BLOCK_WORDS;
    if (!(row[4] & NE_CMD_CONDITIONAL))
        return 1u;
    const uint32_t previous = engine->previous_next[index * 2u];
    if (previous == NE_CMD_NONE || !(engine->mutable_blocks[previous].flags & NE_CMD_HAS_ENTITY))
        return 0u;
    return engine->mutable_blocks[previous].success_count > 0;
}

static void set_error(NECommandMutable *state, uint32_t error, uint32_t offset, uint32_t length) {
    state->success_count = 0;
    if (state->flags & NE_CMD_TRACK_OUTPUT) {
        state->last_output_mode = NE_CMD_LAST_OUTPUT_ERROR;
        state->last_output_error = error;
        state->error_offset = offset;
        state->error_len = length;
    }
}

static uint64_t current_window_age(const NECommandEngine *engine, uint64_t now_ns) {
    uint64_t elapsed = now_ns >= engine->window_started_ns ? now_ns - engine->window_started_ns : 0u;
    if (UINT64_MAX - engine->window_age_at_start_ns < elapsed)
        return UINT64_MAX;
    return engine->window_age_at_start_ns + elapsed;
}

static void reset_window(NECommandEngine *engine, uint64_t now_ns) {
    engine->output_count = 0u;
    engine->output_bytes = 0u;
    engine->window_started_ns = now_ns;
    engine->window_age_at_start_ns = 0u;
}

static uint32_t emit(NECommandEngine *engine, uint32_t index, uint64_t logical_tick, uint32_t recipient,
                     uint32_t name_offset, uint32_t name_len, uint32_t message_offset, uint32_t message_len) {
    if (engine->effect_count >= engine->effect_capacity)
        return NE_CMD_CAPACITY;
    uint32_t *row = engine->effects + (size_t)engine->effect_count * NE_CMD_EFFECT_WORDS;
    row[0] = engine->block_rows[(size_t)index * NE_CMD_BLOCK_WORDS];
    row[1] = (uint32_t)logical_tick;
    row[2] = (uint32_t)(logical_tick >> 32u);
    row[3] = recipient;
    row[4] = name_offset;
    row[5] = name_len;
    row[6] = message_offset;
    row[7] = message_len;
    engine->effect_count++;
    return NE_CMD_OK;
}

/**
 * Execute one supported native command-block specification.
 *
 * Inputs: The command engine, block index, logical/wall clocks and executed output flag.
 *
 * Apply condition, once-per-tick and output-window rules, run the encoded command form, update
 * success/last-output state and append any permitted message effect.
 */
static uint32_t execute_command(NECommandEngine *engine, uint32_t index, uint64_t logical_tick, uint64_t now_ns,
                                uint8_t *ran) {
    const uint32_t *block = engine->block_rows + (size_t)index * NE_CMD_BLOCK_WORDS;
    const uint32_t *spec = engine->spec_rows + (size_t)block[1] * NE_CMD_SPEC_WORDS;
    NECommandMutable *state = &engine->mutable_blocks[index];
    const uint32_t condition =
        block[2] == NE_CMD_MODE_CHAIN ? conditional_met(engine, index) : ((state->flags & NE_CMD_CONDITION_MET) != 0u);

    *ran = 0u;
    const int64_t execution_tick = logical_tick > (uint64_t)INT64_MAX ? INT64_MAX : (int64_t)logical_tick;
    if ((state->flags & NE_CMD_UPDATE_LAST_EXECUTION) && state->last_execution == execution_tick)
        return NE_CMD_OK;
    if (block[2] == NE_CMD_MODE_CHAIN) {
        state->flags = (state->flags & ~NE_CMD_CONDITION_MET) | (condition ? NE_CMD_CONDITION_MET : 0u);
    }
    state->success_count = 0;

    if (condition && spec[2] != NE_CMD_RESULT_EMPTY) {
        if (!engine->output_available) {
            set_error(state, NE_CMD_ERROR_EXECUTION_UNAVAILABLE, NE_CMD_NONE, 0u);
        } else {
            if (current_window_age(engine, now_ns) >= NE_CMD_WINDOW_NS)
                reset_window(engine, now_ns);
            if ((engine->limited &&
                 (engine->output_queued >= NE_CMD_MAX_COUNT || engine->output_count >= NE_CMD_MAX_COUNT)) ||
                spec[0] > NE_CMD_MAX_COMMAND_BYTES || spec[1] > NE_CMD_MAX_SOURCE_BYTES) {
                set_error(state, NE_CMD_ERROR_OUTPUT_LIMIT, NE_CMD_NONE, 0u);
            } else if (spec[2] == NE_CMD_RESULT_ERROR) {
                set_error(state, NE_CMD_ERROR_PARSE, spec[8], spec[9]);
            } else if (engine->limited && (engine->output_bytes > NE_CMD_MAX_OUTPUT_BYTES ||
                                           spec[7] > NE_CMD_MAX_OUTPUT_BYTES - engine->output_bytes)) {
                set_error(state, NE_CMD_ERROR_OUTPUT_BYTE_LIMIT, NE_CMD_NONE, 0u);
            } else {
                if (engine->output_count == UINT32_MAX || engine->output_bytes > UINT32_MAX - spec[7] ||
                    engine->output_queued == UINT32_MAX)
                    return NE_CMD_CAPACITY;
                uint32_t status = emit(engine, index, logical_tick, spec[3], spec[4], spec[5], spec[6], spec[7]);
                if (status != NE_CMD_OK)
                    return status;
                engine->output_count++;
                engine->output_bytes += spec[7];
                engine->output_queued++;
                state->success_count = 1;
                if (state->flags & NE_CMD_TRACK_OUTPUT) {
                    state->last_output_mode = NE_CMD_LAST_OUTPUT_EXECUTED;
                    state->last_output_error = NE_CMD_ERROR_NONE;
                    state->error_offset = NE_CMD_NONE;
                    state->error_len = 0u;
                }
            }
        }
    }
    if (state->flags & NE_CMD_UPDATE_LAST_EXECUTION)
        state->last_execution = execution_tick;
    *ran = 1u;
    return NE_CMD_OK;
}

static uint32_t notify_command(NECommandEngine *engine, uint32_t index, const NECommandHooks *hooks) {
    if (!hooks->notify)
        return NE_CMD_INVALID;
    return hooks->notify(hooks->context, engine->block_rows[(size_t)index * NE_CMD_BLOCK_WORDS]);
}

/**
 * React to a command block's current redstone power.
 *
 * Inputs: The command engine, component ID and host power/scheduling hooks.
 *
 * Resolve the command record, update its powered/condition flags and schedule supported impulse or
 * repeating execution according to entity mode and automatic state.
 */
MCHPRS_NATIVE_EXPORT uint32_t ne_command_update(void *memory, uint32_t component, const NECommandHooks *hooks) {
    NECommandEngine *engine = (NECommandEngine *)memory;
    if (!engine || !hooks || !hooks->powered || !hooks->pending || !hooks->schedule)
        return NE_CMD_INVALID;
    uint32_t index = command_index(engine, component);
    if (index == NE_CMD_NONE)
        return NE_CMD_INVALID;
    const uint32_t *block = engine->block_rows + (size_t)index * NE_CMD_BLOCK_WORDS;
    NECommandMutable *state = &engine->mutable_blocks[index];
    if (!(state->flags & NE_CMD_HAS_ENTITY)) {
        state->flags |= NE_CMD_HAS_ENTITY | NE_CMD_TRACK_OUTPUT | NE_CMD_UPDATE_LAST_EXECUTION;
        state->success_count = 0;
        state->last_execution = -1;
        state->last_output_mode = NE_CMD_LAST_OUTPUT_INITIAL;
        state->last_output_offset = NE_CMD_NONE;
        state->last_output_len = 0u;
    }
    const uint8_t powered = hooks->powered(hooks->context, component) != 0u;
    const uint8_t was_powered = (state->flags & NE_CMD_POWERED) != 0u;
    if (powered)
        state->flags |= NE_CMD_POWERED;
    else
        state->flags &= ~NE_CMD_POWERED;
    const uint8_t rising = powered && !was_powered;
    const uint8_t automatic = (state->flags & NE_CMD_AUTOMATIC) != 0u;
    const uint8_t start = rising || (automatic && state->last_execution < 0);
    const uint8_t repeating = block[2] == NE_CMD_MODE_REPEATING && (powered || automatic);
    if (block[2] != NE_CMD_MODE_CHAIN && (start || repeating) && !hooks->pending(hooks->context, component)) {
        const uint8_t condition = conditional_met(engine, index) != 0u;
        state->flags = (state->flags & ~NE_CMD_CONDITION_MET) | (condition ? NE_CMD_CONDITION_MET : 0u);
        return hooks->schedule(hooks->context, component, 1u, 3u);
    }
    return NE_CMD_OK;
}

/**
 * Execute a command-block tick and its enabled chain.
 *
 * Inputs: The command engine, component ID, logical/wall clocks and host callbacks.
 *
 * Execute the root, notify its changes, follow at most 256 eligible chain blocks in facing order and
 * reschedule a repeating root when required.
 */
MCHPRS_NATIVE_EXPORT uint32_t ne_command_tick(void *memory, uint32_t component, uint64_t logical_tick, uint64_t now_ns,
                                              const NECommandHooks *hooks) {
    NECommandEngine *engine = (NECommandEngine *)memory;
    if (!engine || !hooks)
        return NE_CMD_INVALID;
    uint32_t index = command_index(engine, component);
    if (index == NE_CMD_NONE)
        return NE_CMD_INVALID;
    const uint32_t *root_block = engine->block_rows + (size_t)index * NE_CMD_BLOCK_WORDS;
    NECommandMutable *root_state = &engine->mutable_blocks[index];
    if (root_block[2] == NE_CMD_MODE_CHAIN || !(root_state->flags & NE_CMD_HAS_ENTITY))
        return NE_CMD_OK;

    uint8_t executed = 0u;
    uint32_t status = execute_command(engine, index, logical_tick, now_ns, &executed);
    if (status)
        return status;
    if (!executed)
        return NE_CMD_OK;
    status = notify_command(engine, index, hooks);
    if (status)
        return status;
    if (root_state->flags & NE_CMD_CONDITION_MET) {
        uint32_t current = index;
        for (uint32_t steps = 0; steps < 256u; ++steps) {
            uint32_t next = engine->previous_next[current * 2u + 1u];
            if (next == NE_CMD_NONE)
                break;
            const uint32_t *next_block = engine->block_rows + (size_t)next * NE_CMD_BLOCK_WORDS;
            NECommandMutable *next_state = &engine->mutable_blocks[next];
            if (next_block[2] != NE_CMD_MODE_CHAIN)
                break;
            if (!(next_state->flags & NE_CMD_HAS_ENTITY))
                break;
            if ((next_state->flags & NE_CMD_POWERED) || (next_state->flags & NE_CMD_AUTOMATIC)) {
                uint8_t ran = 0u;
                status = execute_command(engine, next, logical_tick, now_ns, &ran);
                if (status)
                    return status;
                if (!ran)
                    break;
                status = notify_command(engine, next, hooks);
                if (status)
                    return status;
            }
            current = next;
        }
    }
    if (root_block[2] == NE_CMD_MODE_REPEATING)
        return ne_command_update(engine, component, hooks);
    return NE_CMD_OK;
}

/**
 * Read a command block's comparator-visible success count.
 *
 * Inputs: The command engine, component ID and optional found output flag.
 *
 * Resolve an existing command entity, report whether it was found and clamp its stored success count to
 * the redstone range 0 through 15.
 */
MCHPRS_NATIVE_EXPORT int32_t ne_command_success_count(const void *memory, uint32_t component, uint32_t *found) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    if (found)
        *found = 0u;
    if (!engine)
        return 0;
    uint32_t index = command_index(engine, component);
    if (index == NE_CMD_NONE || !(engine->mutable_blocks[index].flags & NE_CMD_HAS_ENTITY))
        return 0;
    if (found)
        *found = 1u;
    int32_t value = engine->mutable_blocks[index].success_count;
    if (value < 0)
        return 0;
    return value > 15 ? 15 : value;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_state_words(const void *memory) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    return engine ? engine->block_count * NE_CMD_STATE_WORDS : 0u;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_read_state(const void *memory, uint32_t *out, uint32_t words) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    if (!engine || (!out && words) || words != engine->block_count * NE_CMD_STATE_WORDS)
        return NE_CMD_INVALID;
    for (uint32_t i = 0; i < engine->block_count; ++i) {
        const uint32_t *block = engine->block_rows + (size_t)i * NE_CMD_BLOCK_WORDS;
        const NECommandMutable *state = &engine->mutable_blocks[i];
        uint32_t *row = out + (size_t)i * NE_CMD_STATE_WORDS;
        row[0] = block[0];
        row[1] = state->flags;
        row[2] = (uint32_t)state->success_count;
        words_i64(state->last_execution, &row[3], &row[4]);
        row[5] = state->last_output_mode;
        row[6] = state->last_output_offset;
        row[7] = state->last_output_len;
        row[8] = state->last_output_error;
        row[9] = state->error_offset;
        row[10] = state->error_len;
    }
    return NE_CMD_OK;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_effect_words(const void *memory) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    return engine ? engine->effect_count * NE_CMD_EFFECT_WORDS : 0u;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_set_effect_buffer(void *memory, uint32_t *effects, uint32_t capacity) {
    NECommandEngine *engine = (NECommandEngine *)memory;
    if (!engine || (!effects && capacity) || capacity < engine->effect_count ||
        capacity > UINT32_MAX / NE_CMD_EFFECT_WORDS)
        return NE_CMD_INVALID;
    engine->effects = effects;
    engine->effect_capacity = capacity;
    return NE_CMD_OK;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_read_effects(const void *memory, uint32_t *out, uint32_t words) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    if (!engine || (!out && words) || words != engine->effect_count * NE_CMD_EFFECT_WORDS)
        return NE_CMD_INVALID;
    if (words)
        copy_bytes(out, engine->effects, (size_t)words * sizeof(uint32_t));
    return NE_CMD_OK;
}

MCHPRS_NATIVE_EXPORT void ne_command_ack_effects(void *memory, uint32_t host_queued, uint64_t now_ns) {
    NECommandEngine *engine = (NECommandEngine *)memory;
    if (!engine)
        return;
    engine->effect_count = 0u;
    engine->output_queued = host_queued;
    (void)now_ns;
}

MCHPRS_NATIVE_EXPORT uint32_t ne_command_read_output_state(const void *memory, uint64_t now_ns, uint32_t out[6]) {
    const NECommandEngine *engine = (const NECommandEngine *)memory;
    if (!engine || !out)
        return NE_CMD_INVALID;
    uint64_t age = current_window_age(engine, now_ns);
    out[0] = engine->output_available;
    out[1] = engine->limited;
    out[2] = engine->output_count;
    out[3] = engine->output_bytes;
    out[4] = engine->output_queued;
    out[5] = age >= NE_CMD_WINDOW_NS ? (uint32_t)NE_CMD_WINDOW_NS : (uint32_t)age;
    return NE_CMD_OK;
}
