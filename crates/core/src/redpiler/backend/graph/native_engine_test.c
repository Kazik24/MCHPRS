/* Standalone freestanding checks for the native runtime's future semantics.
   This includes the implementation so tests can inspect scheduler internals. */
#define NE_COMPILED_POWER 1
#define NE_COMPILED_GUARDS 1
#include "native_engine.c"

static uint8_t ne_test_compiled_power_value;
static uint32_t ne_test_compiled_guard_id = NE_NONE;
static uint8_t ne_test_compiled_guard_value;
static uint32_t ne_test_compiled_guard_calls;
static uint8_t ne_compiled_guard(NEStateView state, uint32_t guard, uint8_t *handled) {
    (void)state;
    ne_test_compiled_guard_calls++;
    *handled = (uint8_t)(guard == ne_test_compiled_guard_id);
    return ne_test_compiled_guard_value;
}
static uint8_t ne_compiled_power(NEStateView state, uint8_t kind, uint32_t target, uint8_t port, uint8_t *handled) {
    (void)state;
    *handled = (uint8_t)(kind == NE_TARGET_INPUT && target == 3u && port == 1u);
    return ne_test_compiled_power_value;
}

/* Clang may lower aggregate zero-initializers to memset even in freestanding
   mode. Provide the tiny local implementation needed by this test DLL. */
void *memset(void *destination, int value, size_t size) {
    volatile uint8_t *bytes = (volatile uint8_t *)destination;
    for (size_t i = 0; i < size; ++i) bytes[i] = (uint8_t)value;
    return destination;
}

typedef union {
    max_align_t alignment;
    uint8_t bytes[128u * 1024u];
} NETestArena;

static NETestArena ne_test_arena;
static size_t ne_test_arena_used;

static void *ne_test_allocate(void *context, size_t size, size_t align) {
    (void)context;
    if (align == 0u) return 0;
    uintptr_t base = (uintptr_t)ne_test_arena.bytes;
    uintptr_t current = base + ne_test_arena_used;
    uintptr_t mask = (uintptr_t)align - 1u;
    uintptr_t aligned = (current + mask) & ~mask;
    size_t offset = (size_t)(aligned - base);
    if (offset > sizeof(ne_test_arena.bytes) || size > sizeof(ne_test_arena.bytes) - offset) return 0;
    ne_test_arena_used = offset + size;
    return (void *)aligned;
}

static void ne_test_deallocate(void *context, void *ptr, size_t size, size_t align) {
    (void)context;
    (void)ptr;
    (void)size;
    (void)align;
}

static NEInitialTick ne_test_tick(const NENode *nodes, uint32_t node, uint32_t priority,
                                  uint32_t delay, int typed, int stale) {
    NEInitialTick tick = {0};
    tick.node = node;
    tick.priority = priority;
    tick.delay = delay;
    tick.registry_type = nodes[node].registry_type;
    tick.current_registry_type = tick.registry_type + (uint32_t)(stale != 0);
    tick.block_type = nodes[node].registry_type;
    tick.has_block_type = (uint8_t)(typed != 0);
    tick.position = nodes[node].position;
    return tick;
}

static NEFutureTick ne_test_future(const NEInitialTick *initial) {
    NEFutureTick tick = {0};
    tick.node = initial->node;
    tick.priority = initial->priority;
    tick.delay = initial->delay;
    tick.registry_type = initial->registry_type;
    tick.current_registry_type = initial->current_registry_type;
    tick.block_type = initial->block_type;
    tick.has_block_type = initial->has_block_type;
    tick.position = initial->position;
    return tick;
}

static uint32_t ne_test_classic_runtime(void) {
#define classic_assert(expression) do { if (!(expression)) { failure = (uint32_t)__LINE__; goto cleanup; } } while (0)
    uint32_t failure = 0;
    NEEngine *engine = 0;
    ne_test_arena_used = 0;

    NENode nodes[3] = {0};
    NEComponentState components[3] = {0};
    NEProfile profiles[1] = {0};
    NESite sites[2] = {0};
    NEContact contacts[1] = {0};
    NEContactVariant variants[1] = {0};
    NEGuard guards[1] = {{.kind=NE_GUARD_ALWAYS}};
    NERoute routes[1] = {0};
    NEDelivery deliveries[1] = {0};
    NERange input_ranges[3u * 3u] = {0};
    NERange conductor_ranges[2u * 2u] = {0};
    NEClassicNet nets[1] = {{.source=0u,.first_wire=0u,.wire_count=1u,.first_input=0u,.input_count=1u,.first_entry=0u,.entry_count=1u}};
    NEClassicWire wires[1] = {{.node=1u,.net=0u,.attenuation=1u}};
    NEClassicInput inputs[1] = {{.receiver=2u,.net=0u,.attenuation=2u,.source_attenuation=1u}};
    NEClassicEntry entries[1] = {{.net=0u,.first_op=0u,.op_count=1u,.commit=1u}};
    NEClassicOp ops[1] = {{.receiver=2u,.nonzero=1u,.when={UINT64_C(1)<<56,0u,0u,0u}}};
    NEClassicRouteEntry route_entries[1] = {{.ordinal=0u,.entry=0u}};
    NERange route_ranges[1] = {{.start=0u,.count=1u}};
    NEClassicWireBinding wire_bindings[3] = {{.net=NE_NONE},{.net=0u,.attenuation=1u},{.net=NE_NONE}};
    NEClassicInputBinding input_bindings[3] = {{.net=NE_NONE},{.net=NE_NONE},{.net=0u,.attenuation=2u,.source_attenuation=1u}};
    uint8_t signals[1] = {3u};
    uint8_t dirty[3] = {0};
    NEProgram program = {0};
    NEAllocator allocator = {0};
    allocator.allocate = ne_test_allocate;
    allocator.deallocate = ne_test_deallocate;

    nodes[0].kind = NE_NODE_ELECTRICAL;
    nodes[0].electrical_kind = NE_ELECTRICAL_LEVER;
    nodes[0].output_route = NE_NONE;
    nodes[0].profile = NE_NONE;
    nodes[1].kind = NE_NODE_WIRE;
    nodes[1].output_route = NE_NONE;
    nodes[1].profile = 0u;
    nodes[2].kind = NE_NODE_ELECTRICAL;
    nodes[2].electrical_kind = NE_ELECTRICAL_TORCH;
    nodes[2].output_route = NE_NONE;
    nodes[2].profile = NE_NONE;
    components[0].output_strength = 8u;
    components[1].wire_power = 9u; /* Projection must use committed net value. */
    components[2].powered = 1u;
    profiles[0].weak_faces = 1u << 2;
    profiles[0].electrical = NE_PROFILE_COMPONENT;
    sites[0].source_kind = NE_SOURCE_COMPONENT;
    sites[0].source_id = 1u;
    sites[1].source_kind = NE_SOURCE_CONTACT;
    sites[1].source_id = 0u;
    contacts[0].first_variant = 0u;
    contacts[0].variant_count = 1u;
    variants[0].guard = 0u;
    variants[0].profile = 0u;
    variants[0].receiver_kind = NE_RECEIVER_COMPONENT;
    variants[0].receiver_id = 2u;
    routes[0].first_delivery = 0u;
    routes[0].delivery_count = 1u;
    routes[0].first_instant_step = NE_NONE;
    deliveries[0].target_site = 0u;
    deliveries[0].guard = 0u;
    deliveries[0].kind = NE_DELIVERY_UPDATE;
    deliveries[0].receiver_kind = NE_RECEIVER_COMPONENT;
    deliveries[0].receiver_id = 2u;

    program.node_count = 3u;
    program.site_count = 2u;
    program.route_count = 1u;
    program.delivery_count = 1u;
    program.profile_count = 1u;
    program.guard_count = 1u;
    program.contact_count = 1u;
    program.contact_variant_count = 1u;
    program.power_link_count = 0u;
    program.classic_net_count = 1u;
    program.classic_wire_count = 1u;
    program.classic_input_count = 1u;
    program.classic_entry_count = 1u;
    program.classic_op_count = 1u;
    program.classic_route_entry_count = 1u;
    program.nodes = nodes;
    program.sites = sites;
    program.profiles = profiles;
    program.contacts = contacts;
    program.contact_variants = variants;
    program.guards = guards;
    program.routes = routes;
    program.deliveries = deliveries;
    program.input_power_ranges = input_ranges;
    program.conductor_power_ranges = conductor_ranges;
    program.classic_nets = nets;
    program.classic_wires = wires;
    program.classic_inputs = inputs;
    program.classic_entries = entries;
    program.classic_ops = ops;
    program.classic_route_ranges = route_ranges;
    program.classic_route_entries = route_entries;
    program.classic_wire_by_node = wire_bindings;
    program.classic_input_by_node = input_bindings;

    NEStateView state = {components, 0, 0, signals};
    engine = ne_create(&program, state, allocator, 0u, 0u, 0, 0u);
    classic_assert(engine != 0);
    classic_assert(ne_query_power(engine, state, NE_TARGET_INPUT, 2u, 0u, 0u) == 7u);
    components[2].output_strength = 3u;
    classic_assert(ne_site_power(engine, state, 1u, NE_PORT_OUTPUT, 0u, 0u, 0, 0u) == 3u);
    classic_assert(ne_site_power(engine, state, 1u, NE_PORT_WEAK, 2u, 0u, 0, 0u) == 3u);
    components[2].output_strength = 0u;
    NEObserved observed = {0};
    classic_assert(ne_resolve_site(engine, state, 0u, 0, &observed));
    classic_assert(observed.wire_power == 2u);

    classic_assert(ne_deliver_route(engine, state, 0u) == NE_OK);
    classic_assert(signals[0] == 8u);
    classic_assert(engine->counters.classic_net_invocations == 1u);
    classic_assert(engine->counters.classic_receiver_ops == 1u);
    classic_assert(engine->counters.callbacks == 0u); /* Owned callback was skipped. */
    classic_assert(engine->pending_standard[2] == 1u);
    classic_assert(engine->ticks[2u][3u].count == 1u);
    classic_assert(engine->ticks[2u][3u].items[0].node == 2u);
    classic_assert(ne_resolve_site(engine, state, 0u, 0, &observed));
    classic_assert(observed.wire_power == 7u);
    components[0].output_strength = 0u;
    classic_assert(ne_query_power(engine, state, NE_TARGET_INPUT, 2u, 0u, 0u) == 6u);
    components[0].output_strength = 8u;

    classic_assert(ne_copy_dirty(engine, dirty, 3u) == 1u);
    classic_assert(dirty[0] == 0u && dirty[1] == 1u && dirty[2] == 0u);
    classic_assert(ne_copy_dirty(engine, dirty, 3u) == 0u);

    entries[0].commit = 0u;
    signals[0] = 3u;
    engine->ticks[2u][3u].count = 0u;
    engine->pending_standard[2] = 0u;
    classic_assert(ne_deliver_route(engine, state, 0u) == NE_OK);
    classic_assert(signals[0] == 3u);
    classic_assert(engine->counters.classic_net_invocations == 1u);
    classic_assert(engine->counters.classic_receiver_ops == 1u);
    classic_assert(engine->counters.callbacks == 0u);

cleanup:
    if (engine != 0) ne_destroy(engine);
#undef classic_assert
    return failure;
}

/* Returns zero on success; nonzero is the assertion's source line. */
__declspec(dllexport) uint32_t __cdecl ne_native_engine_selftest(void) {
#define assert(expression) do { if (!(expression)) { failure = (uint32_t)__LINE__; goto cleanup; } } while (0)
    uint32_t failure = 0;
    NEEngine *engine = 0;
    ne_test_arena_used = 0;

    NENode nodes[8] = {0};
    NEComponentState components[8] = {0};
    NEPistonState piston_states[1] = {0};
    NEPiston pistons[1] = {0};
    NESite sites[1] = {0};
    NEGuard guards[3] = {0};
    NEProfile profiles[1] = {0};
    NERange input_ranges[8u * 3u] = {0};
    NERoute routes[1] = {0};
    NEDelivery deliveries[3] = {0};
    NEInstantStep instant_steps[3] = {0};
    NEProgram program = {0};
    NEInitialTick initial[8];
    NEObserved indexed_items[2] = {0};
    uint32_t indexed_slots[4] = {0u, 0u, 2u, 2u};
    uint32_t indexed_generations[4] = {0u, 7u, 6u, 7u};
    NECaptureSet indexed_capture = {0};
    NECaptureSet linear_capture = {0};

    for (uint32_t i = 0; i < 8u; ++i) {
        nodes[i].kind = NE_NODE_ELECTRICAL;
        nodes[i].electrical_kind = NE_ELECTRICAL_BUTTON;
        nodes[i].output_route = NE_NONE;
        nodes[i].profile = NE_NONE;
        nodes[i].registry_type = 100u + i;
        nodes[i].position.x = (int32_t)i;
    }
    nodes[6].kind = NE_NODE_BUILTIN_RESET;
    nodes[6].kind_data[0] = 1u;
    nodes[7].kind = NE_NODE_BUILTIN_RESET;
    nodes[7].kind_data[0] = 0u;
    pistons[0].node = 2u;
    for (uint32_t i = 0; i < 4u; ++i) {
        components[i].powered = 1u;
        components[i].output_strength = 15u;
    }

    sites[0].source_kind = NE_SOURCE_CONSTANT;
    sites[0].source_id = 0u;
    profiles[0].solid = 1u;
    guards[0].kind = NE_GUARD_SITE;
    guards[0].a = 0u;
    guards[0].view = 1u;
    guards[0].test = NE_SITE_SOLID;
    guards[1] = guards[0];
    guards[1].view = 0u;
    guards[2].kind = NE_GUARD_ALWAYS;

    routes[0].node = 0u;
    routes[0].first_delivery = 0u;
    routes[0].delivery_count = 3u;
    routes[0].first_instant_step = 0u;
    routes[0].instant_step_count = 3u;
    deliveries[0].target_site = 0u;
    deliveries[0].guard = 1u; /* Live guard for the Sample instruction. */
    deliveries[1].target_site = 0u;
    deliveries[1].guard = 2u;
    deliveries[1].receiver_kind = NE_RECEIVER_COMPONENT;
    deliveries[1].receiver_id = 6u;
    deliveries[1].kind = NE_DELIVERY_UPDATE;
    deliveries[1].direction = 1u;
    deliveries[2].target_site = 0u;
    deliveries[2].guard = 0u; /* Captured guard is absent; ArmReset is certified unconditional. */
    deliveries[2].view = 1u;
    instant_steps[0].delivery = 0u;
    instant_steps[0].operand = 0u;
    instant_steps[0].kind = NE_INSTANT_SAMPLE;
    instant_steps[1].delivery = 1u;
    instant_steps[1].operand = NE_NONE;
    instant_steps[1].kind = NE_INSTANT_DISPATCH;
    instant_steps[2].delivery = 2u;
    instant_steps[2].operand = 7u;
    instant_steps[2].kind = NE_INSTANT_ARM_RESET;

    program.node_count = 8u;
    program.site_count = 1u;
    program.route_count = 1u;
    program.delivery_count = 3u;
    program.instant_step_count = 3u;
    program.guard_count = 3u;
    program.profile_count = 1u;
    program.piston_count = 1u;
    program.sound_button_off = 7u;
    program.nodes = nodes;
    program.routes = routes;
    program.deliveries = deliveries;
    program.instant_steps = instant_steps;
    program.sites = sites;
    program.guards = guards;
    program.profiles = profiles;
    program.pistons = pistons;
    program.input_power_ranges = input_ranges;

    initial[0] = ne_test_tick(nodes, 0u, 3u, 0u, 1, 0);
    initial[1] = ne_test_tick(nodes, 2u, 3u, 0u, 1, 0); /* zero-delay FIFO */
    initial[2] = ne_test_tick(nodes, 4u, 3u, 0u, 1, 1); /* stale imported identity */
    initial[3] = ne_test_tick(nodes, 3u, 1u, 1u, 1, 0);
    initial[4] = ne_test_tick(nodes, 1u, 2u, 1u, 1, 0);
    initial[5] = ne_test_tick(nodes, 4u, 1u, 5u, 1, 0);
    initial[6] = initial[5]; /* identical typed imported ticks are retained */
    initial[7] = ne_test_tick(nodes, 5u, 1u, 2u, 0, 0); /* untyped imported tick */

    NEAllocator allocator = {0};
    allocator.allocate = ne_test_allocate;
    allocator.deallocate = ne_test_deallocate;
    NEStateView state = {components, piston_states, 0, 0};

    indexed_items[0].site = 1u;
    indexed_items[1].site = 2u;
    indexed_capture.items = indexed_items;
    indexed_capture.count = 2u;
    indexed_capture.capacity = 4u;
    indexed_capture.slot_of_site = indexed_slots;
    indexed_capture.generation_of_site = indexed_generations;
    indexed_capture.generation = 7u;
    indexed_capture.item_stride = sizeof(NEObserved);
    assert(ne_capture_find(&indexed_capture, 1u) == &indexed_items[0]);
    assert(ne_capture_find(&indexed_capture, 2u) == 0); /* stale generation is absent, despite a matching list row */
    indexed_items[1].site = 3u;
    assert(ne_capture_find(&indexed_capture, 3u) == 0); /* current generation with an invalid slot is absent */
    indexed_items[1].site = 2u;
    assert(ne_capture_find(&indexed_capture, 4u) == 0); /* IDs outside the full index are absent */
    linear_capture.items = indexed_items;
    linear_capture.count = 2u;
    linear_capture.capacity = 4u;
    linear_capture.item_stride = sizeof(NEObserved);
    assert(ne_capture_find(&linear_capture, 2u) == &indexed_items[1]);
    assert(ne_capture_find(&linear_capture, 3u) == 0);

    engine = ne_create(&program, state, allocator, 42u, 31u, initial, 8u);
    assert(engine != 0);

    NEInitialTick illegal_zero_delay = ne_test_tick(nodes, 5u, 2u, 0u, 1, 0);
    NEFutureTick illegal_zero_probe = ne_test_future(&illegal_zero_delay);
    assert(ne_schedule_raw(engine, illegal_zero_probe));
    assert(engine->ticks[31u][2u].count == 0u && engine->pending_standard[5] == 0u);

    NEFutureTick typed_probe = ne_test_future(&initial[5]);
    NEFutureTick untyped_probe = ne_test_future(&initial[7]);
    NEFutureTick stale_probe = ne_test_future(&initial[2]);
    assert(ne_is_standard_tick(&program, &typed_probe));
    assert(!ne_is_standard_tick(&program, &untyped_probe));
    assert(!ne_is_standard_tick(&program, &stale_probe));
    assert(engine->pending_standard[4] == 2u); /* duplicate typed entries count separately */
    assert(engine->pending_standard[5] == 0u); /* untyped entries do not suppress scheduling */
    assert(ne_schedule_node(engine, 4u, 7u, 2u));
    assert(engine->pending_standard[4] == 2u); /* typed pending event suppresses another tick */
    assert(ne_schedule_node(engine, 5u, 4u, 1u));
    assert(engine->pending_standard[5] == 1u);

    NEFutureTick future_ticks[16];
    NEFutureSnapshot snapshot = {0};
    snapshot.ticks = future_ticks;
    snapshot.tick_capacity = 16u;
    assert(ne_snapshot_future(engine, &snapshot) == NE_OK);
    assert(snapshot.logical_tick == 42u && snapshot.scheduler_position == 31u);
    assert(snapshot.tick_count == 9u); /* imported duplicates and untyped entries remain queued */

    ne_test_compiled_guard_id = 1u;
    ne_test_compiled_guard_value = 0u;
    ne_test_compiled_guard_calls = 0u;
    ne_verify_compiled_power(engine, 0u);
    assert(ne_guard_matches(engine, state, 1u, 0, 0u) == 0); /* Compiled root fast path. */
    assert(ne_test_compiled_guard_calls == 1u);
    ne_verify_compiled_power(engine, 1u);
    assert(ne_guard_matches(engine, state, 1u, 0, 0u) == 1); /* Verification returns general result. */
    assert(engine->status == NE_ERR_BAD_STATE && engine->error_node == NE_NONE);
    engine->status = NE_OK;
    engine->error_node = NE_NONE;
    ne_test_compiled_guard_value = 1u;
    assert(ne_guard_matches(engine, state, 1u, 0, 0u) == 1);
    assert(engine->status == NE_OK);
    assert(engine->counters.compiled_guard_evaluations == 3u);
    ne_verify_compiled_power(engine, 0u);
    ne_test_compiled_guard_id = NE_NONE;

    assert(ne_guard_matches(engine, state, 0u, 0, 0u) == 0); /* captured never falls back to live */
    NECaptureSet empty_capture = {0};
    assert(ne_guard_matches(engine, state, 0u, &empty_capture, 0u) == 0);
    NEObserved captured_site = {0};
    captured_site.site = 0u;
    captured_site.profile = 0u;
    captured_site.block_profile = 0u;
    NECaptureSet one_capture = {0};
    one_capture.items = &captured_site;
    one_capture.count = 1u;
    one_capture.capacity = 1u;
    one_capture.item_stride = sizeof(NEObserved);
    assert(ne_guard_matches(engine, state, 0u, &one_capture, 0u) == 1);
    assert(ne_guard_matches(engine, state, 1u, 0, 0u) == 1); /* live view resolves the solid constant */

    NEWalkFrame *outer_frame = ne_walk_acquire(engine);
    assert(outer_frame != 0);
    outer_frame->layer = 0x1234u;
    outer_frame->queue_count[1] = 7u;
    uintptr_t outer_address = (uintptr_t)outer_frame;
    for (uint32_t i = 1u; i < 12u; ++i) assert(ne_walk_acquire(engine) != 0);
    assert((uintptr_t)engine->walk_frames == outer_address);
    assert(outer_frame->layer == 0x1234u && outer_frame->queue_count[1] == 7u);
    for (uint32_t i = 0; i < 12u; ++i) ne_walk_release(engine);
    assert(engine->walk_depth == 0u);

    assert(ne_tick(engine, state) == NE_OK);
    assert(engine->scheduler_position == 0u && engine->logical_tick == 43u);
    assert(engine->counters.scheduled_ticks == 4u); /* stale identity was discarded */
    assert(components[0].powered == 0u && components[1].powered == 0u &&
           components[2].powered == 0u && components[3].powered == 0u);

    NEEffect effects[8];
    assert(ne_copy_effects(engine, effects, 8u) == 4u);
    assert(effects[0].position.x == 0 && effects[1].position.x == 2); /* zero-delay priority-3 FIFO */
    assert(effects[2].position.x == 3 && effects[3].position.x == 1); /* next-slot priority 1 then 2 */

    snapshot.tick_count = 0u;
    assert(ne_snapshot_future(engine, &snapshot) == NE_OK);
    assert(snapshot.scheduler_position == 0u && snapshot.tick_count == 4u);
    assert(future_ticks[0].node == 5u && future_ticks[0].delay == 1u);
    assert(future_ticks[1].node == 5u && future_ticks[1].delay == 3u);
    assert(future_ticks[2].node == 4u && future_ticks[2].delay == 4u);
    assert(future_ticks[3].node == 4u && future_ticks[3].delay == 4u);

    uint64_t callbacks_before = engine->counters.callbacks;
    assert(ne_deliver_route_captured(engine, state, 0u, 0) == NE_OK);
    assert(engine->counters.piston_samples == 1u);
    assert(engine->counters.callbacks == callbacks_before + 1u); /* Sample/ArmReset do not dispatch callbacks. */
    assert(engine->ticks[2u][3u].count == 2u);
    assert(engine->ticks[2u][3u].items[0].node == 6u && engine->ticks[2u][3u].items[1].node == 7u);

    ne_test_compiled_power_value = 7u;
    ne_verify_compiled_power(engine, 0u);
    assert(ne_query_power(engine, state, NE_TARGET_INPUT, 3u, 1u, 0u) == 7u);
    assert(engine->counters.compiled_power_queries == 1u);
    ne_verify_compiled_power(engine, 1u);
    assert(ne_query_power(engine, state, NE_TARGET_INPUT, 3u, 1u, 0u) == 0u);
    assert(engine->counters.compiled_power_queries == 2u);
    assert(engine->status == NE_ERR_BAD_STATE && engine->error_node == 3u);
    assert(engine->pending_standard[6] == 1u && engine->pending_standard[7] == 1u);

    /* Missing captured data suppresses the guarded Sample; the certified reset
       still schedules directly even though its delivery's captured guard is absent. */
    profiles[0].solid = 0u;
    engine->ticks[2u][3u].count = 0u;
    engine->pending_standard[6] = 0u;
    engine->pending_standard[7] = 0u;
    callbacks_before = engine->counters.callbacks;
    assert(ne_deliver_route_captured(engine, state, 0u, 0) == NE_OK);
    assert(engine->counters.piston_samples == 1u);
    assert(engine->counters.callbacks == callbacks_before + 1u);
    assert(engine->ticks[2u][3u].count == 2u);
    assert(engine->ticks[2u][3u].items[0].node == 6u && engine->ticks[2u][3u].items[1].node == 7u);

    ne_destroy(engine);
    engine = 0;
    failure = ne_test_classic_runtime();

cleanup:
    if (engine != 0) ne_destroy(engine);
#undef assert
    return failure;
}
