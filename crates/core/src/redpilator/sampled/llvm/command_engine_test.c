/* Standalone, libc-free checks for command_engine.c. Returns zero on success. */
#include "abi.h"

#define TEST_BLOCK_WORDS 10u
#define TEST_SPEC_WORDS 10u
#define TEST_STATE_WORDS 11u
#define TEST_EFFECT_WORDS 8u
#define TEST_NONE UINT32_MAX
#define TEST_ENTITY (1u << 1)
#define TEST_POWERED (1u << 2)
#define TEST_CONDITION_MET (1u << 4)
#define TEST_TRACK_OUTPUT (1u << 5)
#define TEST_UPDATE_LAST_EXECUTION (1u << 6)

typedef struct {
    void *context;
    uint8_t (*powered)(void *context, uint32_t component);
    uint8_t (*pending)(void *context, uint32_t component);
    uint32_t (*schedule)(void *context, uint32_t component, uint32_t delay, uint8_t priority);
    uint32_t (*notify)(void *context, uint32_t component);
} NECommandHooks;

size_t ne_command_state_size(uint32_t block_count, uint32_t effect_capacity);
uint32_t ne_command_init(void *memory, size_t memory_bytes, uint32_t component_count,
    const MchprsNativeComponent *components, const uint32_t *block_rows, uint32_t block_count,
    const uint32_t *spec_rows, uint32_t spec_count, const uint8_t *strings, uint32_t string_bytes,
    const uint32_t output_state[6], uint64_t now_ns, uint32_t effect_capacity);
uint32_t ne_command_prepare_indices(void *memory, uint32_t *components, uint32_t component_count, uint32_t *previous_next, uint32_t words);
uint32_t ne_command_update(void *memory, uint32_t component, const NECommandHooks *hooks);
uint32_t ne_command_tick(void *memory, uint32_t component, uint64_t logical_tick, uint64_t now_ns,
    const NECommandHooks *hooks);
uint32_t ne_command_state_words(const void *memory);
uint32_t ne_command_read_state(const void *memory, uint32_t *out, uint32_t words);
uint32_t ne_command_effect_words(const void *memory);
uint32_t ne_command_read_effects(const void *memory, uint32_t *out, uint32_t words);
void ne_command_ack_effects(void *memory, uint32_t host_queued, uint64_t now_ns);
uint32_t ne_command_read_output_state(const void *memory, uint64_t now_ns, uint32_t out[6]);

typedef struct {
    uint8_t power[3];
    uint8_t pending[3];
    uint32_t scheduled_component[16];
    uint32_t scheduled_delay[16];
    uint8_t scheduled_priority[16];
    uint32_t schedule_count;
    uint32_t notified[16];
    uint32_t notify_count;
} Mock;

static uint8_t mock_powered(void *context, uint32_t component) {
    Mock *mock = (Mock *)context;
    return component < 3u ? mock->power[component] : 0u;
}

static uint8_t mock_pending(void *context, uint32_t component) {
    Mock *mock = (Mock *)context;
    return component < 3u ? mock->pending[component] : 0u;
}

static uint32_t mock_schedule(void *context, uint32_t component, uint32_t delay, uint8_t priority) {
    Mock *mock = (Mock *)context;
    if (component >= 3u || mock->schedule_count >= 16u) return 3u;
    const uint32_t index = mock->schedule_count++;
    mock->scheduled_component[index] = component;
    mock->scheduled_delay[index] = delay;
    mock->scheduled_priority[index] = priority;
    mock->pending[component] = 1u;
    return 0u;
}

static uint32_t mock_notify(void *context, uint32_t component) {
    Mock *mock = (Mock *)context;
    if (component >= 3u || mock->notify_count >= 16u) return 3u;
    mock->notified[mock->notify_count++] = component;
    return 0u;
}

static void mock_reset(Mock *mock) {
    uint8_t *bytes = (uint8_t *)mock;
    for (size_t i = 0; i < sizeof(*mock); ++i) bytes[i] = 0u;
}

static int initialize(void *memory, size_t memory_bytes, const MchprsNativeComponent *components,
    const uint32_t *blocks, uint32_t block_count, const uint32_t *specs, uint32_t spec_count,
    const uint8_t *strings, uint32_t string_bytes, const uint32_t output[6], uint32_t effects) {
    static uint32_t component_commands[3], previous_next[6];
    return ne_command_init(memory, memory_bytes, 3u, components, blocks, block_count, specs, spec_count,
        strings, string_bytes, output, 0u, effects) == 0u
        && ne_command_prepare_indices(memory, component_commands, 3u, previous_next, block_count * 2u) == 0u;
}

static int lifecycle_test(void) {
    static union { uint64_t align; uint8_t bytes[8192]; } memory;
    MchprsNativeComponent components[3] = {0};
    components[0].x = 0;
    components[1].x = 10;
    components[2].x = 1;
    /* Impulse root points east to the chain block at x=1. */
    uint32_t blocks[3 * TEST_BLOCK_WORDS] = {0};
    blocks[0] = 0u; blocks[1] = 0u; blocks[2] = 0u; blocks[3] = 5u;
    blocks[4] = TEST_ENTITY | TEST_CONDITION_MET | TEST_TRACK_OUTPUT | TEST_UPDATE_LAST_EXECUTION;
    blocks[5] = 4u; blocks[6] = UINT32_MAX; blocks[7] = UINT32_MAX;
    blocks[8] = TEST_NONE;
    blocks[10] = 1u; blocks[11] = 1u; blocks[12] = 1u; blocks[13] = 5u;
    blocks[14] = TEST_ENTITY | TEST_TRACK_OUTPUT | TEST_UPDATE_LAST_EXECUTION;
    blocks[15] = 4u; blocks[16] = UINT32_MAX; blocks[17] = UINT32_MAX;
    blocks[18] = TEST_NONE;
    blocks[20] = 2u; blocks[21] = 2u; blocks[22] = 2u; blocks[23] = 5u;
    blocks[24] = TEST_ENTITY | TEST_POWERED | TEST_TRACK_OUTPUT | TEST_UPDATE_LAST_EXECUTION;
    blocks[25] = 4u; blocks[26] = UINT32_MAX; blocks[27] = UINT32_MAX;
    blocks[28] = TEST_NONE;
    const uint8_t strings[] = {'o','k'};
    uint32_t specs[3 * TEST_SPEC_WORDS] = {0};
    for (uint32_t i = 0; i < 3u; ++i) {
        uint32_t *row = specs + i * TEST_SPEC_WORDS;
        row[0] = 4u; row[1] = 1u; row[2] = 1u; row[3] = 1u;
        row[4] = TEST_NONE; row[6] = 0u; row[7] = 2u;
        row[8] = TEST_NONE;
    }
    const uint32_t output[6] = {1u, 0u, 0u, 0u, 0u, 0u};
    if (!initialize(memory.bytes, sizeof(memory.bytes), components, blocks, 3u, specs, 3u, strings, 2u, output, 16u)) return 1;

    Mock mock;
    mock_reset(&mock);
    NECommandHooks hooks = {&mock, mock_powered, mock_pending, mock_schedule, mock_notify};
    if (ne_command_update(memory.bytes, 0u, &hooks) != 0u || mock.schedule_count != 0u) return 2;
    mock.power[0] = 1u;
    if (ne_command_update(memory.bytes, 0u, &hooks) != 0u || mock.schedule_count != 1u) return 3;
    if (mock.scheduled_component[0] != 0u || mock.scheduled_delay[0] != 1u || mock.scheduled_priority[0] != 3u) return 4;
    if (ne_command_update(memory.bytes, 0u, &hooks) != 0u || mock.schedule_count != 1u) return 5;
    mock.pending[0] = 0u;
    if (ne_command_tick(memory.bytes, 0u, 1u, 1u, &hooks) != 0u) return 6;
    if (mock.notify_count != 2u || mock.notified[0] != 0u || mock.notified[1] != 2u) return 7;
    if (ne_command_effect_words(memory.bytes) != 2u * TEST_EFFECT_WORDS) return 8;
    uint32_t effects[2 * TEST_EFFECT_WORDS];
    if (ne_command_read_effects(memory.bytes, effects, 2u * TEST_EFFECT_WORDS) != 0u) return 9;
    if (effects[0] != 0u || effects[TEST_EFFECT_WORDS] != 2u) return 10;
    uint32_t state[3 * TEST_STATE_WORDS];
    if (ne_command_state_words(memory.bytes) != 3u * TEST_STATE_WORDS
        || ne_command_read_state(memory.bytes, state, 3u * TEST_STATE_WORDS) != 0u) return 11;
    if (state[2] != 1u || state[2 * TEST_STATE_WORDS + 2] != 1u) return 12;
    ne_command_ack_effects(memory.bytes, 2u, 1u);
    if (ne_command_effect_words(memory.bytes) != 0u) return 13;
    uint32_t output_after[6];
    if (ne_command_read_output_state(memory.bytes, 1u, output_after) != 0u || output_after[4] != 2u) return 14;

    /* Falling then rising power schedules a fresh impulse pulse. */
    mock.power[0] = 0u;
    if (ne_command_update(memory.bytes, 0u, &hooks) != 0u) return 15;
    mock.power[0] = 1u;
    if (ne_command_update(memory.bytes, 0u, &hooks) != 0u || mock.schedule_count != 2u) return 16;

    /* Repeating mode schedules while powered, executes, and schedules again. */
    mock.power[1] = 1u;
    if (ne_command_update(memory.bytes, 1u, &hooks) != 0u || mock.schedule_count != 3u) return 17;
    mock.pending[1] = 0u;
    if (ne_command_tick(memory.bytes, 1u, 2u, 2u, &hooks) != 0u || mock.schedule_count != 4u) return 18;
    const uint32_t effects_before_duplicate = ne_command_effect_words(memory.bytes);
    const uint32_t notifications_before_duplicate = mock.notify_count;
    if (ne_command_tick(memory.bytes, 1u, 2u, 2u, &hooks) != 0u
        || ne_command_effect_words(memory.bytes) != effects_before_duplicate
        || mock.notify_count != notifications_before_duplicate) return 19;

    /* Prepared links follow coordinates, including inactive and conditional chains. */
    blocks[24] = TEST_ENTITY | TEST_TRACK_OUTPUT | TEST_UPDATE_LAST_EXECUTION | 1u;
    if (!initialize(memory.bytes, sizeof(memory.bytes), components, blocks, 3u, specs, 3u, strings, 2u, output, 16u)) return 20;
    mock_reset(&mock);
    if (ne_command_tick(memory.bytes, 0u, 1u, 1u, &hooks) != 0u
        || ne_command_effect_words(memory.bytes) != TEST_EFFECT_WORDS || mock.notify_count != 1u) return 21;
    blocks[24] |= TEST_POWERED;
    if (!initialize(memory.bytes, sizeof(memory.bytes), components, blocks, 3u, specs, 3u, strings, 2u, output, 16u)) return 22;
    mock_reset(&mock);
    if (ne_command_tick(memory.bytes, 0u, 1u, 1u, &hooks) != 0u
        || ne_command_effect_words(memory.bytes) != 2u * TEST_EFFECT_WORDS || mock.notify_count != 2u) return 23;
    specs[0] = 0u; specs[2] = 0u; specs[3] = 0u; specs[6] = TEST_NONE; specs[7] = 0u;
    if (!initialize(memory.bytes, sizeof(memory.bytes), components, blocks, 3u, specs, 3u, strings, 2u, output, 16u)) return 24;
    mock_reset(&mock);
    if (ne_command_tick(memory.bytes, 0u, 1u, 1u, &hooks) != 0u
        || ne_command_effect_words(memory.bytes) != 0u || mock.notify_count != 2u) return 25;
    if (ne_command_read_state(memory.bytes, state, 3u * TEST_STATE_WORDS) != 0u
        || state[2 * TEST_STATE_WORDS + 2] != 0u || (state[2 * TEST_STATE_WORDS + 1] & TEST_CONDITION_MET)) return 26;
    return 0;
}

static int precedence_case(uint32_t available, uint32_t limited, uint32_t count, uint32_t bytes,
    uint32_t queued, uint32_t result, uint32_t message_len, uint32_t expected_error) {
    static union { uint64_t align; uint8_t bytes[2048]; } memory;
    MchprsNativeComponent components[3] = {0};
    const uint8_t strings[] = {'b','a','d','x','x','x','x','x','x','x'};
    uint32_t block[TEST_BLOCK_WORDS] = {0};
    block[0] = 0u; block[1] = 0u; block[2] = 0u; block[3] = 5u;
    block[4] = TEST_ENTITY | TEST_CONDITION_MET | TEST_TRACK_OUTPUT | TEST_UPDATE_LAST_EXECUTION;
    block[5] = 4u; block[6] = UINT32_MAX; block[7] = UINT32_MAX; block[8] = TEST_NONE;
    uint32_t spec[TEST_SPEC_WORDS] = {0};
    spec[0] = 4u; spec[1] = 1u; spec[2] = result;
    if (result == 1u) {
        spec[3] = 1u; spec[4] = TEST_NONE; spec[6] = 3u; spec[7] = message_len; spec[8] = TEST_NONE;
    } else {
        spec[3] = 0u; spec[4] = TEST_NONE; spec[6] = TEST_NONE; spec[8] = 0u; spec[9] = 3u;
    }
    uint32_t output[6] = {available, limited, count, bytes, queued, 0u};
    if (!initialize(memory.bytes, sizeof(memory.bytes), components, block, 1u, spec, 1u,
        strings, sizeof(strings), output, 4u)) return 1;
    Mock mock;
    mock_reset(&mock);
    NECommandHooks hooks = {&mock, mock_powered, mock_pending, mock_schedule, mock_notify};
    if (ne_command_tick(memory.bytes, 0u, 7u, 1u, &hooks) != 0u) return 2;
    uint32_t state[TEST_STATE_WORDS];
    if (ne_command_read_state(memory.bytes, state, TEST_STATE_WORDS) != 0u) return 3;
    if (state[5] != 2u || state[8] != expected_error) return 4;
    return 0;
}

static int empty_program_test(void) {
    static union { uint64_t align; uint8_t bytes[512]; } memory;
    const uint32_t output[6] = {1u, 0u, 0u, 0u, 0u, 0u};
    if (!ne_command_state_size(0u, 0u)
        || ne_command_init(memory.bytes, sizeof(memory.bytes), 0u, 0, 0, 0u, 0, 0u, 0, 0u,
            output, 0u, 0u) != 0u) return 1;
    if (ne_command_state_words(memory.bytes) != 0u || ne_command_read_state(memory.bytes, 0, 0u) != 0u
        || ne_command_effect_words(memory.bytes) != 0u || ne_command_read_effects(memory.bytes, 0, 0u) != 0u) return 2;
    return 0;
}

int ne_command_engine_test(void) {
    int result = lifecycle_test();
    if (result) return 100 + result;
    result = empty_program_test();
    if (result) return 150 + result;
    /* Unavailable state wins over a stored static parser error. */
    result = precedence_case(0u, 0u, 0u, 0u, 0u, 2u, 0u, 1u);
    if (result) return 200 + result;
    /* Count/queue rate limits win over a static parser error. */
    result = precedence_case(1u, 1u, 64u, 0u, 0u, 2u, 0u, 2u);
    if (result) return 300 + result;
    result = precedence_case(1u, 1u, 0u, 0u, 64u, 2u, 0u, 2u);
    if (result) return 350 + result;
    /* A static parser error wins over the parsed-message byte limit. */
    result = precedence_case(1u, 1u, 0u, 65536u, 0u, 2u, 0u, 4u);
    if (result) return 400 + result;
    /* A parsed success can still fail the output-byte limit. */
    result = precedence_case(1u, 1u, 0u, 65530u, 0u, 1u, 7u, 3u);
    if (result) return 500 + result;
    return 0;
}
