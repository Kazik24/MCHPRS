#include "native_engine.h"

/* This file is deliberately freestanding: Clang builds it without a C runtime.
   Dynamic storage comes from Rust's allocator callbacks; no graph callback is
   invoked from this file. */

#define NE_SCHEDULER_SLOTS 32u
#define NE_PRIORITY_COUNT 4u
#define NE_MAX_RECURSION 128u

typedef struct {
    uint32_t site;
    uint32_t profile;
    uint32_t block_profile;
    uint32_t receiver_id;
    uint8_t receiver_kind;
    uint8_t powered;
    uint8_t locked;
    uint8_t wire_power;
    uint8_t wire_north, wire_south, wire_east, wire_west;
    uint8_t piston_extended;
    uint8_t flags;
    uint8_t reserved[2];
} NEObserved;

typedef struct {
    NEObserved *items;
    uint32_t count;
    uint32_t capacity;
    uint32_t *slot_of_site;
    uint32_t *generation_of_site;
    uint32_t generation;
    uint32_t item_stride;
} NECaptureSet;

typedef struct {
    NEObserved observed;
    uint8_t visited;
    int8_t x_bias;
    int8_t z_bias;
    uint8_t heading;
    uint8_t heading_valid;
    uint8_t neighbors_valid;
    uint8_t reserved;
    uint32_t layer;
    uint32_t neighbor_slots[24];
} NEWalkSite;

typedef struct {
    NEWalkSite *sites;
    uint32_t site_count;
    uint32_t site_capacity;
    uint32_t *slot_of_site;
    uint32_t *generation_of_site;
    uint32_t generation;
    uint32_t *queues[3];
    uint32_t queue_count[3];
    uint32_t queue_capacity[3];
    uint32_t layer;
} NEWalkFrame;

enum {
    NE_OBS_ELECTRICAL = 1u << 0,
    NE_OBS_WIRE = 1u << 1,
    NE_OBS_PISTON = 1u << 2
};

typedef struct {
    NEFutureTick *items;
    uint32_t head;
    uint32_t count;
    uint32_t capacity;
} NETickQueue;

typedef struct {
    NEFuturePistonRequest *items;
    uint32_t head;
    uint32_t count;
    uint32_t capacity;
} NERequestQueue;

typedef struct {
    NEFutureDeferredPiston *items;
    uint32_t head;
    uint32_t count;
    uint32_t capacity;
} NEDeferredQueue;

typedef struct {
    NEEffect *items;
    uint32_t head;
    uint32_t count;
    uint32_t capacity;
} NEEffectQueue;

struct NEEngine {
    const NEProgram *program;
    NEAllocator allocator;
    NETickQueue ticks[NE_SCHEDULER_SLOTS][NE_PRIORITY_COUNT];
    NERequestQueue requests;
    NEDeferredQueue deferred;
    NEEffectQueue effects;
    uint32_t *pending_standard;
    uint8_t *dirty;
    uint8_t *dirty_classic_nets;
    NEWalkFrame *walk_frames;
    uint32_t walk_frame_count;
    uint32_t walk_frame_capacity;
    uint32_t walk_depth;
    uint32_t scheduler_position;
    uint64_t logical_tick;
    uint8_t phase;
    uint32_t error_node;
    NEStatus status;
    uint8_t verify_compiled_power;
    NECounters counters;
};

static void ne_bytes_zero(void *ptr, size_t size) {
    uint8_t *out = (uint8_t *)ptr;
    for (size_t i = 0; i < size; ++i) out[i] = 0;
}

static void ne_bytes_copy(void *dst, const void *src, size_t size) {
    uint8_t *out = (uint8_t *)dst;
    const uint8_t *in = (const uint8_t *)src;
    for (size_t i = 0; i < size; ++i) out[i] = in[i];
}

static void *ne_alloc(NEEngine *engine, size_t size, size_t align) {
    if (size == 0 || engine->allocator.allocate == 0) return 0;
    return engine->allocator.allocate(engine->allocator.context, size, align);
}

static void ne_free(NEEngine *engine, void *ptr, size_t size, size_t align) {
    if (ptr != 0 && engine->allocator.deallocate != 0)
        engine->allocator.deallocate(engine->allocator.context, ptr, size, align);
}

static uint32_t ne_next_capacity(uint32_t old_capacity, uint32_t needed) {
    uint32_t next = old_capacity == 0 ? 8u : old_capacity;
    while (next < needed) {
        if (next > UINT32_MAX / 2u) return 0;
        next *= 2u;
    }
    return next;
}

static int ne_tick_queue_reserve(NEEngine *engine, NETickQueue *queue, uint32_t needed) {
    if (needed <= queue->capacity) return 1;
    uint32_t capacity = ne_next_capacity(queue->capacity, needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(NEFutureTick)) return 0;
    NEFutureTick *items = (NEFutureTick *)ne_alloc(engine, (size_t)capacity * sizeof(NEFutureTick), _Alignof(NEFutureTick));
    if (items == 0) return 0;
    for (uint32_t i = 0; i < queue->count; ++i)
        items[i] = queue->items[queue->head + i];
    ne_free(engine, queue->items, (size_t)queue->capacity * sizeof(NEFutureTick), _Alignof(NEFutureTick));
    queue->items = items;
    queue->capacity = capacity;
    queue->head = 0;
    return 1;
}

static int ne_tick_queue_push(NEEngine *engine, NETickQueue *queue, NEFutureTick tick) {
    if (queue->head + queue->count == queue->capacity && queue->head != 0) {
        for (uint32_t i = 0; i < queue->count; ++i) queue->items[i] = queue->items[queue->head + i];
        queue->head = 0;
    }
    if (!ne_tick_queue_reserve(engine, queue, queue->count + 1u)) return 0;
    queue->items[queue->head + queue->count] = tick;
    queue->count++;
    return 1;
}

static int ne_tick_queue_pop(NETickQueue *queue, NEFutureTick *tick) {
    if (queue->count == 0) return 0;
    *tick = queue->items[queue->head++];
    queue->count--;
    if (queue->count == 0) queue->head = 0;
    return 1;
}

static int ne_request_reserve(NEEngine *engine, uint32_t needed) {
    NERequestQueue *queue = &engine->requests;
    if (needed <= queue->capacity) return 1;
    uint32_t capacity = ne_next_capacity(queue->capacity, needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(NEFuturePistonRequest)) return 0;
    NEFuturePistonRequest *items = (NEFuturePistonRequest *)ne_alloc(engine, (size_t)capacity * sizeof(NEFuturePistonRequest), _Alignof(NEFuturePistonRequest));
    if (items == 0) return 0;
    for (uint32_t i = 0; i < queue->count; ++i) items[i] = queue->items[queue->head + i];
    ne_free(engine, queue->items, (size_t)queue->capacity * sizeof(NEFuturePistonRequest), _Alignof(NEFuturePistonRequest));
    queue->items = items;
    queue->capacity = capacity;
    queue->head = 0;
    return 1;
}

static int ne_request_push(NEEngine *engine, NEFuturePistonRequest request) {
    NERequestQueue *queue = &engine->requests;
    if (queue->head + queue->count == queue->capacity && queue->head != 0) {
        for (uint32_t i = 0; i < queue->count; ++i) queue->items[i] = queue->items[queue->head + i];
        queue->head = 0;
    }
    if (!ne_request_reserve(engine, queue->count + 1u)) return 0;
    queue->items[queue->head + queue->count++] = request;
    return 1;
}

static int ne_deferred_reserve(NEEngine *engine, uint32_t needed) {
    NEDeferredQueue *queue = &engine->deferred;
    if (needed <= queue->capacity) return 1;
    uint32_t capacity = ne_next_capacity(queue->capacity, needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(NEFutureDeferredPiston)) return 0;
    NEFutureDeferredPiston *items = (NEFutureDeferredPiston *)ne_alloc(engine, (size_t)capacity * sizeof(NEFutureDeferredPiston), _Alignof(NEFutureDeferredPiston));
    if (items == 0) return 0;
    for (uint32_t i = 0; i < queue->count; ++i) items[i] = queue->items[queue->head + i];
    ne_free(engine, queue->items, (size_t)queue->capacity * sizeof(NEFutureDeferredPiston), _Alignof(NEFutureDeferredPiston));
    queue->items = items;
    queue->capacity = capacity;
    queue->head = 0;
    return 1;
}

static int ne_deferred_push(NEEngine *engine, NEFutureDeferredPiston deferred) {
    NEDeferredQueue *queue = &engine->deferred;
    if (queue->head + queue->count == queue->capacity && queue->head != 0) {
        for (uint32_t i = 0; i < queue->count; ++i) queue->items[i] = queue->items[queue->head + i];
        queue->head = 0;
    }
    if (!ne_deferred_reserve(engine, queue->count + 1u)) return 0;
    queue->items[queue->head + queue->count++] = deferred;
    return 1;
}

static int ne_effect_reserve(NEEngine *engine, uint32_t needed) {
    NEEffectQueue *queue = &engine->effects;
    if (needed <= queue->capacity) return 1;
    uint32_t capacity = ne_next_capacity(queue->capacity, needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(NEEffect)) return 0;
    NEEffect *items = (NEEffect *)ne_alloc(engine, (size_t)capacity * sizeof(NEEffect), _Alignof(NEEffect));
    if (items == 0) return 0;
    for (uint32_t i = 0; i < queue->count; ++i) items[i] = queue->items[queue->head + i];
    ne_free(engine, queue->items, (size_t)queue->capacity * sizeof(NEEffect), _Alignof(NEEffect));
    queue->items = items;
    queue->capacity = capacity;
    queue->head = 0;
    return 1;
}

static int ne_effect_push(NEEngine *engine, NEEffect effect) {
    NEEffectQueue *queue = &engine->effects;
    if (queue->head + queue->count == queue->capacity && queue->head != 0) {
        for (uint32_t i = 0; i < queue->count; ++i) queue->items[i] = queue->items[queue->head + i];
        queue->head = 0;
    }
    if (!ne_effect_reserve(engine, queue->count + 1u)) return 0;
    queue->items[queue->head + queue->count++] = effect;
    return 1;
}

static void ne_mark_dirty(NEEngine *engine, uint32_t node) {
    if (node < engine->program->node_count) engine->dirty[node] = 1;
}

static NEStatus ne_set_node(NEEngine *engine, NEStateView state, uint32_t node, uint8_t powered,
                            uint8_t strength, int locked, int notify);

static uint32_t ne_delay_slot(uint32_t slot, uint32_t delay) {
    return (slot + delay) & (NE_SCHEDULER_SLOTS - 1u);
}

static int ne_is_standard_tick(const NEProgram *program, const NEFutureTick *tick) {
    if (tick->node >= program->node_count) return 0;
    const NENode *node = &program->nodes[tick->node];
    return node->registry_type == tick->registry_type &&
           node->registry_type == tick->current_registry_type &&
           tick->has_block_type && tick->block_type == node->registry_type &&
           node->position.x == tick->position.x && node->position.y == tick->position.y &&
           node->position.z == tick->position.z;
}

static int ne_schedule_raw(NEEngine *engine, NEFutureTick tick) {
    if (tick.delay >= NE_SCHEDULER_SLOTS || tick.priority >= NE_PRIORITY_COUNT) return 1;
    if (tick.delay == 0 && tick.priority != 3u) return 1;
    uint32_t slot = ne_delay_slot(engine->scheduler_position, tick.delay);
    NETickQueue *queue = &engine->ticks[slot][tick.priority];
    if (!ne_tick_queue_push(engine, queue, tick)) {
        engine->status = NE_ERR_ALLOC;
        return 0;
    }
    if (ne_is_standard_tick(engine->program, &tick)) engine->pending_standard[tick.node]++;
    return 1;
}

static int ne_schedule_node(NEEngine *engine, uint32_t node, uint32_t delay, uint32_t priority) {
    if (node >= engine->program->node_count) return 0;
    if (delay >= NE_SCHEDULER_SLOTS || priority >= NE_PRIORITY_COUNT || (delay == 0 && priority != 3u)) return 1;
    if (engine->pending_standard[node] != 0) return 1;
    const NENode *spec = &engine->program->nodes[node];
    NEFutureTick tick;
    tick.node = node;
    tick.priority = priority;
    tick.delay = delay;
    tick.registry_type = spec->registry_type;
    tick.current_registry_type = spec->registry_type;
    tick.block_type = spec->registry_type;
    tick.has_block_type = 1;
    tick.reserved[0] = tick.reserved[1] = tick.reserved[2] = 0;
    tick.position = spec->position;
    return ne_schedule_raw(engine, tick);
}

static int ne_schedule_game_ticks(NEEngine *engine, uint32_t node, uint32_t delay, uint32_t priority) {
    if (delay > UINT32_MAX / 2u) return 1;
    return ne_schedule_node(engine, node, delay * 2u, priority);
}

/* Semantic entry points are below. They operate only on NEProgram/NEStateView
   and native future storage. The implementation is intentionally independent
   of GraphBackend callbacks and of the symbolic-region programs. */
static NEStatus ne_tick_node(NEEngine *engine, NEStateView state, uint32_t node);
static NEStatus ne_deliver_route_captured(NEEngine *engine, NEStateView state, uint32_t route, const NECaptureSet *capture);
static NEStatus ne_deliver_route(NEEngine *engine, NEStateView state, uint32_t route);
static NEStatus ne_drain_piston_actions(NEEngine *engine, NEStateView state);
static NEStatus ne_publish_deferred(NEEngine *engine, NEStateView state);

NEEngine *ne_create(const NEProgram *program, NEStateView state, NEAllocator allocator,
                    uint64_t logical_tick, uint32_t scheduler_position,
                    const NEInitialTick *initial_ticks, uint32_t initial_tick_count) {
    if (program == 0 || allocator.allocate == 0 || allocator.deallocate == 0 ||
        (program->node_count != 0 && (program->nodes == 0 || state.components == 0)) ||
        (program->route_count != 0 && program->routes == 0) ||
        (program->delivery_count != 0 && program->deliveries == 0) ||
        (program->instant_step_count != 0 && program->instant_steps == 0) ||
        (program->wire_count != 0 && program->wires == 0) ||
        (program->dust_path_count != 0 && program->dust_paths == 0) ||
        (program->dust_path_wire_count != 0 && program->dust_path_wires == 0) ||
        (program->piston_count != 0 && (program->pistons == 0 || state.pistons == 0)) ||
        (program->resource_count != 0 && (program->resources == 0 || state.resources == 0)) ||
        (program->classic_net_count != 0 &&
            (program->classic_nets == 0 || program->classic_route_ranges == 0 ||
             program->classic_wire_by_node == 0 || program->classic_input_by_node == 0 ||
             state.classic_signals == 0)) ||
        (program->classic_wire_count != 0 && program->classic_wires == 0) ||
        (program->classic_input_count != 0 && program->classic_inputs == 0) ||
        (program->classic_entry_count != 0 && program->classic_entries == 0) ||
        (program->classic_op_count != 0 && program->classic_ops == 0) ||
        (program->classic_route_entry_count != 0 && program->classic_route_entries == 0) ||
        (initial_tick_count != 0 && initial_ticks == 0) || scheduler_position >= NE_SCHEDULER_SLOTS)
        return 0;
    NEEngine *engine = (NEEngine *)allocator.allocate(allocator.context, sizeof(NEEngine), _Alignof(NEEngine));
    if (engine == 0) return 0;
    ne_bytes_zero(engine, sizeof(NEEngine));
    engine->program = program;
    engine->allocator = allocator;
    engine->logical_tick = logical_tick;
    engine->scheduler_position = scheduler_position;
    engine->error_node = NE_NONE;
    engine->status = NE_OK;
    if (program->node_count != 0) {
        engine->pending_standard = (uint32_t *)ne_alloc(engine, (size_t)program->node_count * sizeof(uint32_t), _Alignof(uint32_t));
        if (engine->pending_standard == 0) { ne_destroy(engine); return 0; }
        ne_bytes_zero(engine->pending_standard, (size_t)program->node_count * sizeof(uint32_t));
        engine->dirty = (uint8_t *)ne_alloc(engine, (size_t)program->node_count * sizeof(uint8_t), _Alignof(uint8_t));
        if (engine->dirty == 0) { ne_destroy(engine); return 0; }
        ne_bytes_zero(engine->dirty, (size_t)program->node_count * sizeof(uint8_t));
    }
    if (program->classic_net_count != 0) {
        engine->dirty_classic_nets = (uint8_t *)ne_alloc(engine, (size_t)program->classic_net_count, _Alignof(uint8_t));
        if (engine->dirty_classic_nets == 0) { ne_destroy(engine); return 0; }
        ne_bytes_zero(engine->dirty_classic_nets, (size_t)program->classic_net_count);
    }
    for (uint32_t i = 0; i < initial_tick_count; ++i) {
        NEFutureTick tick;
        tick.node = initial_ticks[i].node;
        tick.priority = initial_ticks[i].priority;
        tick.delay = initial_ticks[i].delay;
        tick.registry_type = initial_ticks[i].registry_type;
        tick.current_registry_type = initial_ticks[i].current_registry_type;
        tick.block_type = initial_ticks[i].block_type;
        tick.has_block_type = initial_ticks[i].has_block_type;
        tick.reserved[0] = tick.reserved[1] = tick.reserved[2] = 0;
        tick.position = initial_ticks[i].position;
        if (tick.delay >= NE_SCHEDULER_SLOTS || tick.priority >= NE_PRIORITY_COUNT ||
            (tick.delay == 0 && tick.priority != 3u) || !ne_schedule_raw(engine, tick)) {
            ne_destroy(engine);
            return 0;
        }
    }
    return engine;
}

void ne_destroy(NEEngine *engine) {
    if (engine == 0) return;
    NEAllocator allocator = engine->allocator;
    for (uint32_t slot = 0; slot < NE_SCHEDULER_SLOTS; ++slot)
        for (uint32_t priority = 0; priority < NE_PRIORITY_COUNT; ++priority) {
            NETickQueue *queue = &engine->ticks[slot][priority];
            ne_free(engine, queue->items, (size_t)queue->capacity * sizeof(NEFutureTick), _Alignof(NEFutureTick));
        }
    ne_free(engine, engine->requests.items, (size_t)engine->requests.capacity * sizeof(NEFuturePistonRequest), _Alignof(NEFuturePistonRequest));
    ne_free(engine, engine->deferred.items, (size_t)engine->deferred.capacity * sizeof(NEFutureDeferredPiston), _Alignof(NEFutureDeferredPiston));
    ne_free(engine, engine->effects.items, (size_t)engine->effects.capacity * sizeof(NEEffect), _Alignof(NEEffect));
    ne_free(engine, engine->pending_standard, (size_t)engine->program->node_count * sizeof(uint32_t), _Alignof(uint32_t));
    ne_free(engine, engine->dirty, (size_t)engine->program->node_count * sizeof(uint8_t), _Alignof(uint8_t));
    ne_free(engine, engine->dirty_classic_nets, (size_t)engine->program->classic_net_count, _Alignof(uint8_t));
    for (uint32_t i = 0; i < engine->walk_frame_count; ++i) {
        NEWalkFrame *frame = &engine->walk_frames[i];
        ne_free(engine, frame->sites, (size_t)frame->site_capacity * sizeof(NEWalkSite), _Alignof(NEWalkSite));
        ne_free(engine, frame->slot_of_site, (size_t)engine->program->site_count * sizeof(uint32_t), _Alignof(uint32_t));
        ne_free(engine, frame->generation_of_site, (size_t)engine->program->site_count * sizeof(uint32_t), _Alignof(uint32_t));
        for (uint32_t q = 0; q < 3u; ++q)
            ne_free(engine, frame->queues[q], (size_t)frame->queue_capacity[q] * sizeof(uint32_t), _Alignof(uint32_t));
    }
    ne_free(engine, engine->walk_frames, (size_t)engine->walk_frame_capacity * sizeof(NEWalkFrame), _Alignof(NEWalkFrame));
    allocator.deallocate(allocator.context, engine, sizeof(NEEngine), _Alignof(NEEngine));
}

NEStatus ne_use_block(NEEngine *engine, NEStateView state, uint32_t node) {
    if (engine == 0 || node >= engine->program->node_count) return NE_ERR_INVALID;
    if (engine->status != NE_OK) return engine->status;
    const NENode *spec = &engine->program->nodes[node];
    if (spec->kind != NE_NODE_ELECTRICAL) return NE_OK;
    NEComponentState *component = &state.components[node];
    if (spec->electrical_kind == NE_ELECTRICAL_LEVER) {
        uint8_t powered = (uint8_t)!component->powered;
        return ne_set_node(engine, state, node, powered, powered ? 15u : 0u, -1, 1);
    } else if (spec->electrical_kind == NE_ELECTRICAL_BUTTON) {
        if (!component->powered) {
            (void)ne_schedule_game_ticks(engine, node, 10u, 3u);
            return ne_set_node(engine, state, node, 1u, 15u, -1, 1);
        }
    }
    return engine->status;
}

NEStatus ne_set_pressure_plate(NEEngine *engine, NEStateView state, uint32_t node, uint8_t powered) {
    if (engine == 0 || node >= engine->program->node_count) return NE_ERR_INVALID;
    const NENode *spec = &engine->program->nodes[node];
    if (spec->kind == NE_NODE_ELECTRICAL && spec->electrical_kind == NE_ELECTRICAL_PRESSURE_PLATE) {
        return ne_set_node(engine, state, node, (uint8_t)(powered != 0), powered ? 15u : 0u, -1, 1);
    }
    return engine->status;
}

static NEStatus ne_drain_current_ticks(NEEngine *engine, NEStateView state) {
    for (uint32_t priority = 0; priority < NE_PRIORITY_COUNT; ++priority) {
        NETickQueue *queue = &engine->ticks[engine->scheduler_position][priority];
        NEFutureTick tick;
        while (ne_tick_queue_pop(queue, &tick)) {
            if (ne_is_standard_tick(engine->program, &tick) && engine->pending_standard[tick.node] != 0)
                engine->pending_standard[tick.node]--;
            if (tick.registry_type != tick.current_registry_type || tick.node >= engine->program->node_count) continue;
            engine->counters.scheduled_ticks++;
            NEStatus status = ne_tick_node(engine, state, tick.node);
            if (status != NE_OK) {
                engine->status = status;
                engine->error_node = tick.node;
                return status;
            }
        }
    }
    return NE_OK;
}

NEStatus ne_tick(NEEngine *engine, NEStateView state) {
    if (engine == 0) return NE_ERR_INVALID;
    if (engine->status != NE_OK) return engine->status;
    engine->logical_tick++;
    engine->counters.ticks++;
    engine->phase = 1u;
    NEStatus status = ne_drain_current_ticks(engine, state);
    if (status != NE_OK) return status;
    engine->scheduler_position = (engine->scheduler_position + 1u) & (NE_SCHEDULER_SLOTS - 1u);
    status = ne_drain_current_ticks(engine, state);
    if (status != NE_OK) return status;
    engine->phase = 2u;
    status = ne_drain_piston_actions(engine, state);
    if (status != NE_OK) return status;
    engine->phase = 3u;
    status = ne_publish_deferred(engine, state);
    if (status != NE_OK) return status;
    engine->phase = 0u;
    return engine->status;
}

NEStatus ne_snapshot_future(const NEEngine *engine, NEFutureSnapshot *snapshot) {
    if (engine == 0 || snapshot == 0) return NE_ERR_INVALID;
    uint32_t tick_count = 0;
    for (uint32_t delay = 0; delay < NE_SCHEDULER_SLOTS; ++delay) {
        uint32_t slot = ne_delay_slot(engine->scheduler_position, delay);
        for (uint32_t priority = 0; priority < NE_PRIORITY_COUNT; ++priority)
            tick_count += engine->ticks[slot][priority].count;
    }
    snapshot->logical_tick = engine->logical_tick;
    snapshot->scheduler_position = engine->scheduler_position;
    snapshot->tick_count = tick_count;
    snapshot->request_count = engine->requests.count;
    snapshot->deferred_count = engine->deferred.count;
    if (tick_count > snapshot->tick_capacity || engine->requests.count > snapshot->request_capacity ||
        engine->deferred.count > snapshot->deferred_capacity) return NE_ERR_CAPACITY;
    if ((tick_count != 0 && snapshot->ticks == 0) ||
        (engine->requests.count != 0 && snapshot->requests == 0) ||
        (engine->deferred.count != 0 && snapshot->deferred == 0)) return NE_ERR_INVALID;
    uint32_t out = 0;
    for (uint32_t delay = 0; delay < NE_SCHEDULER_SLOTS; ++delay) {
        uint32_t slot = ne_delay_slot(engine->scheduler_position, delay);
        for (uint32_t priority = 0; priority < NE_PRIORITY_COUNT; ++priority) {
            const NETickQueue *queue = &engine->ticks[slot][priority];
            for (uint32_t i = 0; i < queue->count; ++i) {
                NEFutureTick tick = queue->items[queue->head + i];
                tick.delay = delay;
                snapshot->ticks[out++] = tick;
            }
        }
    }
    for (uint32_t i = 0; i < engine->requests.count; ++i)
        snapshot->requests[i] = engine->requests.items[engine->requests.head + i];
    for (uint32_t i = 0; i < engine->deferred.count; ++i)
        snapshot->deferred[i] = engine->deferred.items[engine->deferred.head + i];
    return NE_OK;
}

const NECounters *ne_counters(const NEEngine *engine) {
    return engine == 0 ? 0 : &engine->counters;
}

uint32_t ne_error_node(const NEEngine *engine) {
    return engine == 0 ? NE_NONE : engine->error_node;
}

void ne_verify_compiled_power(NEEngine *engine, uint8_t enabled) {
    if (engine != 0) engine->verify_compiled_power = (uint8_t)(enabled != 0);
}

static const NEObserved *ne_capture_find(const NECaptureSet *capture, uint32_t site) {
    if (capture == 0) return 0;
    if (capture->slot_of_site != 0 && capture->generation_of_site != 0) {
        if (site >= capture->capacity || capture->generation_of_site[site] != capture->generation)
            return 0;
        uint32_t slot = capture->slot_of_site[site];
        if (slot >= capture->count) return 0;
        return (const NEObserved *)((const uint8_t *)capture->items + (size_t)slot * capture->item_stride);
    }
    for (uint32_t i = 0; i < capture->count; ++i)
        if (((const NEObserved *)((const uint8_t *)capture->items + (size_t)i * capture->item_stride))->site == site)
            return (const NEObserved *)((const uint8_t *)capture->items + (size_t)i * capture->item_stride);
    return 0;
}

static const NEProfile *ne_profile(const NEProgram *program, uint32_t profile) {
    return profile < program->profile_count ? &program->profiles[profile] : 0;
}

static void ne_fill_profile_properties(NEObserved *observed, const NEProfile *profile) {
    if (profile == 0 || (profile->flags & NE_PROFILE_FLAG_IS_WIRE) == 0 || (observed->flags & NE_OBS_WIRE) != 0) return;
    observed->flags |= NE_OBS_WIRE;
    observed->wire_power = (uint8_t)(profile->wire_bits & 15u);
    observed->wire_north = (uint8_t)((profile->wire_bits >> 4) & 3u);
    observed->wire_south = (uint8_t)((profile->wire_bits >> 6) & 3u);
    observed->wire_east = (uint8_t)((profile->wire_bits >> 8) & 3u);
    observed->wire_west = (uint8_t)((profile->wire_bits >> 10) & 3u);
}

static const NEContactVariant *ne_selected_contact_variant(NEEngine *engine, NEStateView state, uint32_t contact);
static int ne_guard_matches(NEEngine *engine, NEStateView state, uint32_t guard_id, const NECaptureSet *capture, uint32_t depth);
#ifdef NE_COMPILED_GUARDS
static uint8_t ne_compiled_guard(NEStateView state, uint32_t guard, uint8_t *handled);
#endif
static int ne_guard_matches_general(NEEngine *engine, NEStateView state, uint32_t guard_id,
                                    const NECaptureSet *capture, uint32_t depth);

static int ne_guard_matches_general(NEEngine *engine, NEStateView state, uint32_t guard_id,
                                    const NECaptureSet *capture, uint32_t depth) {
    const NEProgram *program = engine->program;
    if (guard_id >= program->guard_count || depth >= NE_MAX_RECURSION) return 0;
    const NEGuard *guard = &program->guards[guard_id];
    switch (guard->kind) {
        case NE_GUARD_ALWAYS: return 1;
        case NE_GUARD_STATE:
            switch (guard->a) {
                case NE_STATE_PISTON_EXTENDED:
                    return guard->b < program->piston_count && state.pistons[guard->b].extended != 0;
                case NE_STATE_PISTON_HEAD_PRESENT:
                    return guard->b < program->piston_count && state.pistons[guard->b].head_present != 0;
                case NE_STATE_PISTON_AVAILABILITY:
                    return guard->b < program->piston_count && state.pistons[guard->b].availability == guard->value;
                case NE_STATE_RESOURCE_AT:
                    return guard->b < program->resource_count && state.resources[guard->b].at == guard->child_start;
                case NE_STATE_RESOURCE_ENABLED:
                    return guard->b < program->resource_count && state.resources[guard->b].enabled != 0;
                default: return 0;
            }
        case NE_GUARD_SITE: {
            if (guard->a >= program->site_count) return 0;
            uint32_t profile_id = NE_NONE;
            uint32_t block_profile_id = NE_NONE;
            int is_wire = 0;
            const NEObserved *observed = guard->view == 1u ? ne_capture_find(capture, guard->a) : 0;
            if (guard->view == 1u && observed == 0) return 0;
            if (observed != 0) {
                profile_id = observed->profile;
                block_profile_id = observed->block_profile;
                is_wire = (observed->flags & NE_OBS_WIRE) != 0;
            } else {
                const NESite *site = &program->sites[guard->a];
                if (site->source_kind == NE_SOURCE_COMPONENT && site->source_id < program->node_count) {
                    const NENode *node = &program->nodes[site->source_id];
                    profile_id = NE_NONE;
                    block_profile_id = node->profile;
                    is_wire = node->kind == NE_NODE_WIRE;
                } else if (site->source_kind == NE_SOURCE_CONSTANT) {
                    profile_id = site->source_id;
                    block_profile_id = site->source_id;
                } else if (site->source_kind == NE_SOURCE_CONTACT) {
                    const NEContactVariant *variant = ne_selected_contact_variant(engine, state, site->source_id);
                    if (variant == 0) return 0;
                    profile_id = variant->profile;
                    block_profile_id = variant->profile;
                    const NEProfile *profile = ne_profile(program, block_profile_id);
                    is_wire = profile != 0 && (profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0;
                }
            }
            const NEProfile *profile = ne_profile(program, block_profile_id);
            if (profile == 0) return 0;
            switch (guard->test) {
                case NE_SITE_SOLID: return profile->solid != 0;
                case NE_SITE_TRANSPARENT: return profile->transparent != 0;
                case NE_SITE_IS_WIRE: return is_wire || (profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0;
                case NE_SITE_IS_PROFILE: return profile_id == guard->b;
                default: return 0;
            }
        }
        case NE_GUARD_NOT:
            return !ne_guard_matches(engine, state, guard->a, capture, depth + 1u);
        case NE_GUARD_ALL:
            if ((uint64_t)guard->child_start + guard->child_count > program->guard_child_count) return 0;
            for (uint32_t i = 0; i < guard->child_count; ++i)
                if (!ne_guard_matches(engine, state, program->guard_children[guard->child_start + i], capture, depth + 1u)) return 0;
            return 1;
        case NE_GUARD_ANY:
            if ((uint64_t)guard->child_start + guard->child_count > program->guard_child_count) return 0;
            for (uint32_t i = 0; i < guard->child_count; ++i)
                if (ne_guard_matches(engine, state, program->guard_children[guard->child_start + i], capture, depth + 1u)) return 1;
            return 0;
        default: return 0;
    }
}

static int ne_guard_matches(NEEngine *engine, NEStateView state, uint32_t guard_id,
                            const NECaptureSet *capture, uint32_t depth) {
#ifdef NE_COMPILED_GUARDS
    if (depth == 0u) {
        uint8_t handled = 0;
        uint8_t compiled = ne_compiled_guard(state, guard_id, &handled);
        if (handled) {
            engine->counters.compiled_guard_evaluations++;
            if (!engine->verify_compiled_power) return compiled != 0;
            int general = ne_guard_matches_general(engine, state, guard_id, capture, depth);
            if ((compiled != 0) != (general != 0)) {
                engine->status = NE_ERR_BAD_STATE;
                engine->error_node = NE_NONE;
            }
            return general;
        }
    }
#endif
    return ne_guard_matches_general(engine, state, guard_id, capture, depth);
}

static uint8_t ne_saturating_sub_power(uint8_t power, uint8_t attenuation) {
    return power > attenuation ? (uint8_t)(power - attenuation) : 0u;
}

/* Classic wires keep their committed signal in the native-owned net buffer.
   Read that signal lazily so private-wire projection never races route order. */
static uint8_t ne_classic_wire_power(NEEngine *engine, NEStateView state, uint32_t node, uint8_t fallback) {
    const NEProgram *program = engine->program;
    if (node >= program->node_count || program->classic_wire_by_node == 0 || state.classic_signals == 0)
        return fallback;
    const NEClassicWireBinding *binding = &program->classic_wire_by_node[node];
    if (binding->net >= program->classic_net_count) return fallback;
    return ne_saturating_sub_power(state.classic_signals[binding->net], binding->attenuation);
}

static int ne_resolve_site(NEEngine *engine, NEStateView state, uint32_t site_id, const NECaptureSet *capture, NEObserved *observed) {
    const NEProgram *program = engine->program;
    if (site_id >= program->site_count) return 0;
    const NEObserved *cached = ne_capture_find(capture, site_id);
    if (cached != 0) { *observed = *cached; return 1; }
    ne_bytes_zero(observed, sizeof(*observed));
    observed->site = site_id;
    observed->profile = NE_NONE;
    observed->block_profile = NE_NONE;
    const NESite *site = &program->sites[site_id];
    if (site->source_kind == NE_SOURCE_CONSTANT) {
        observed->profile = site->source_id;
        observed->block_profile = site->source_id;
        const NEProfile *profile = ne_profile(program, observed->profile);
        ne_fill_profile_properties(observed, profile);
        return profile != 0;
    }
    if (site->source_kind == NE_SOURCE_COMPONENT) {
        if (site->source_id >= program->node_count) return 0;
        const NENode *node = &program->nodes[site->source_id];
        observed->profile = NE_NONE;
        observed->block_profile = node->profile;
        observed->receiver_kind = NE_RECEIVER_COMPONENT;
        observed->receiver_id = site->source_id;
        if (node->kind == NE_NODE_WIRE) {
            const NEComponentState *wire = &state.components[site->source_id];
            observed->flags |= NE_OBS_WIRE;
            observed->wire_north = wire->wire_north;
            observed->wire_south = wire->wire_south;
            observed->wire_east = wire->wire_east;
            observed->wire_west = wire->wire_west;
            observed->wire_power = ne_classic_wire_power(engine, state, site->source_id, wire->wire_power);
        } else if (node->kind == NE_NODE_ELECTRICAL || node->kind == NE_NODE_OBSERVER || node->kind == NE_NODE_BUILTIN_RESET) {
            const NEComponentState *component = &state.components[site->source_id];
            observed->flags |= NE_OBS_ELECTRICAL;
            observed->powered = component->powered;
            observed->locked = component->repeater_locked;
        } else if (node->kind == NE_NODE_PISTON) {
            uint32_t piston_id = node->kind_data[0];
            if (piston_id >= program->piston_count) return 0;
            observed->flags |= NE_OBS_PISTON;
            observed->piston_extended = state.pistons[piston_id].extended;
            observed->receiver_kind = NE_RECEIVER_PISTON_BASE;
            observed->receiver_id = piston_id;
        }
        const NEProfile *profile = ne_profile(program, observed->block_profile);
        ne_fill_profile_properties(observed, profile);
        return profile != 0;
    }
    if (site->source_kind != NE_SOURCE_CONTACT || site->source_id >= program->contact_count) return 0;
    const NEContactVariant *variant = ne_selected_contact_variant(engine, state, site->source_id);
    if (variant == 0) return 0;
    observed->profile = variant->profile;
    observed->block_profile = variant->profile;
    observed->receiver_kind = variant->receiver_kind;
    observed->receiver_id = variant->receiver_id;
    if (variant->receiver_kind == NE_RECEIVER_COMPONENT && variant->receiver_id < program->node_count) {
        uint32_t id = variant->receiver_id;
        const NENode *node = &program->nodes[id];
        if (node->kind == NE_NODE_WIRE) {
            const NEComponentState *wire = &state.components[id];
            observed->flags |= NE_OBS_WIRE;
            observed->wire_north = wire->wire_north;
            observed->wire_south = wire->wire_south;
            observed->wire_east = wire->wire_east;
            observed->wire_west = wire->wire_west;
            observed->wire_power = ne_classic_wire_power(engine, state, id, wire->wire_power);
        } else {
            const NEComponentState *component = &state.components[id];
            observed->flags |= NE_OBS_ELECTRICAL;
            observed->powered = component->powered;
            observed->locked = component->repeater_locked;
        }
    } else if ((variant->receiver_kind == NE_RECEIVER_PISTON_BASE || variant->receiver_kind == NE_RECEIVER_PISTON_HEAD) &&
               variant->receiver_id < program->piston_count) {
        observed->flags |= NE_OBS_PISTON;
        observed->piston_extended = state.pistons[variant->receiver_id].extended;
    }
    const NEProfile *profile = ne_profile(program, observed->profile);
    ne_fill_profile_properties(observed, profile);
    return profile != 0;
}

static const NEContactVariant *ne_selected_contact_variant(NEEngine *engine, NEStateView state, uint32_t contact) {
    const NEProgram *program = engine->program;
    if (contact >= program->contact_count) return 0;
    const NEContact *spec = &program->contacts[contact];
    if ((uint64_t)spec->first_variant + spec->variant_count > program->contact_variant_count) return 0;
    for (uint32_t i = 0; i < spec->variant_count; ++i) {
        const NEContactVariant *variant = &program->contact_variants[spec->first_variant + i];
        if (ne_guard_matches(engine, state, variant->guard, 0, 0)) return variant;
    }
    return 0;
}

static uint8_t ne_query_power(NEEngine *engine, NEStateView state, uint8_t target_kind, uint32_t target_id,
                              uint8_t target_port, uint32_t depth);
#ifdef NE_COMPILED_POWER
static uint8_t ne_compiled_power(NEStateView state, uint8_t kind, uint32_t target, uint8_t port, uint8_t *handled);
#endif

static void ne_wire_raw_sides(NEEngine *engine, NEStateView state, uint32_t wire_index,
                              uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west);
static uint8_t ne_regulate_wire_side(uint8_t north, uint8_t south, uint8_t east, uint8_t west,
                                    uint8_t raw_north, uint8_t raw_south, uint8_t raw_east, uint8_t raw_west,
                                    uint8_t *out_south, uint8_t *out_east, uint8_t *out_west);

static uint8_t ne_source_strength(NEEngine *engine, NEStateView state, uint32_t site_id, const NEObserved *observed) {
    const NEProgram *program = engine->program;
    if (observed->receiver_kind == NE_RECEIVER_COMPONENT && observed->receiver_id < program->node_count) {
        const NENode *node = &program->nodes[observed->receiver_id];
        if (node->kind == NE_NODE_ELECTRICAL && node->electrical_kind == NE_ELECTRICAL_COMPARATOR)
            return state.components[observed->receiver_id].output_strength;
        if (node->kind == NE_NODE_WIRE) return observed->wire_power;
        const NEProfile *node_profile = ne_profile(program, node->profile);
        if (node_profile != 0 && node_profile->electrical == NE_PROFILE_REDSTONE_SOURCE) return 15u;
        if (!observed->powered) return 0u;
        if (node->kind == NE_NODE_OBSERVER || node->kind == NE_NODE_BUILTIN_RESET) return 15u;
        if (node->kind == NE_NODE_ELECTRICAL) {
            switch (node->electrical_kind) {
                case NE_ELECTRICAL_TORCH:
                case NE_ELECTRICAL_REPEATER:
                case NE_ELECTRICAL_BUTTON:
                case NE_ELECTRICAL_LEVER:
                case NE_ELECTRICAL_PRESSURE_PLATE: return 15u;
                default: return 0u;
            }
        }
        return 0u;
    }
    if (site_id < program->site_count) {
        const NESite *site = &program->sites[site_id];
        if (site->source_kind == NE_SOURCE_CONSTANT) return site->source_strength;
    }
    const NEProfile *profile = ne_profile(program, observed->profile);
    return profile != 0 && profile->electrical == NE_PROFILE_REDSTONE_SOURCE ? 15u : 0u;
}

static uint8_t ne_observed_stored_strength(NEEngine *engine, NEStateView state, uint32_t site_id,
                                           const NEObserved *observed) {
    const NEProgram *program = engine->program;
    if (observed->receiver_kind != NE_RECEIVER_COMPONENT || observed->receiver_id >= program->node_count)
        return ne_source_strength(engine, state, site_id, observed);
    const NENode *node = &program->nodes[observed->receiver_id];
    if (node->kind == NE_NODE_WIRE)
        return ne_classic_wire_power(engine, state, observed->receiver_id,
                                     state.components[observed->receiver_id].wire_power);
    if (node->kind == NE_NODE_ELECTRICAL || node->kind == NE_NODE_OBSERVER || node->kind == NE_NODE_BUILTIN_RESET)
        return state.components[observed->receiver_id].output_strength;
    return 0u;
}

static int ne_emits(NEEngine *engine, NEStateView state, uint32_t site_id, uint32_t profile_id,
                    uint8_t face, int strong, const NEObserved *observed) {
    const NEProfile *profile = ne_profile(engine->program, profile_id);
    if (profile == 0 || face > 5u) return 0;
    if ((profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0) {
        if (face == 1u) return 1;
        if (face == 0u) return 0;
        uint8_t north = NE_WIRE_SIDE_NONE, south = NE_WIRE_SIDE_NONE;
        uint8_t east = NE_WIRE_SIDE_NONE, west = NE_WIRE_SIDE_NONE;
        int dynamic_wire = 0;
        if (observed != 0 && (observed->flags & NE_OBS_WIRE) != 0 &&
            observed->receiver_kind == NE_RECEIVER_COMPONENT && observed->receiver_id < engine->program->node_count &&
            site_id < engine->program->site_count &&
            engine->program->sites[site_id].source_kind == NE_SOURCE_COMPONENT &&
            engine->program->sites[site_id].source_id == observed->receiver_id) {
            const NENode *node = &engine->program->nodes[observed->receiver_id];
            if (node->kind == NE_NODE_WIRE) {
                uint32_t wire_index = node->kind_data[0];
                ne_wire_raw_sides(engine, state, wire_index, &north, &south, &east, &west);
                const NEObserved *wire = observed;
                uint8_t out_south, out_east, out_west;
                north = ne_regulate_wire_side(wire->wire_north, wire->wire_south, wire->wire_east, wire->wire_west,
                                              north, south, east, west, &out_south, &out_east, &out_west);
                south = out_south; east = out_east; west = out_west;
                dynamic_wire = 1;
            }
        }
        if (!dynamic_wire) {
            uint32_t bits = profile->wire_bits;
            north = (uint8_t)((bits >> 4) & 3u);
            south = (uint8_t)((bits >> 6) & 3u);
            east = (uint8_t)((bits >> 8) & 3u);
            west = (uint8_t)((bits >> 10) & 3u);
        }
        uint8_t side = face == 2u ? south : face == 3u ? north : face == 4u ? east : face == 5u ? west : NE_WIRE_SIDE_NONE;
        (void)site_id;
        (void)strong;
        return side != NE_WIRE_SIDE_NONE;
    }
    uint32_t mask = strong ? profile->strong_faces : profile->weak_faces;
    return (mask & (1u << face)) != 0;
}

static uint8_t ne_site_power(NEEngine *engine, NEStateView state, uint32_t site_id, uint8_t port,
                             uint8_t face, uint8_t include_dust, const NECaptureSet *capture, uint32_t depth) {
    if (depth >= NE_MAX_RECURSION) return 0;
    const NEProgram *program = engine->program;
    if (site_id >= program->site_count) return 0;
    const NESite *site = &program->sites[site_id];
    if ((port == NE_PORT_OUTPUT || port == NE_PORT_STORED) && site->source_kind == NE_SOURCE_COMPONENT &&
        site->source_id < program->node_count) {
        const NENode *node = &program->nodes[site->source_id];
        return node->kind == NE_NODE_WIRE
                   ? ne_classic_wire_power(engine, state, site->source_id, state.components[site->source_id].wire_power) :
               (node->kind == NE_NODE_ELECTRICAL || node->kind == NE_NODE_OBSERVER || node->kind == NE_NODE_BUILTIN_RESET)
                   ? state.components[site->source_id].output_strength : 0u;
    }
    NEObserved observed;
    if (!ne_resolve_site(engine, state, site_id, capture, &observed)) return 0;
    uint32_t profile_id = observed.block_profile;
    const NEProfile *profile = ne_profile(program, profile_id);
    if (profile == 0) return 0;
    uint8_t strength = ne_observed_stored_strength(engine, state, site_id, &observed);
    switch (port) {
        case NE_PORT_OUTPUT:
        case NE_PORT_STORED: return strength;
        case NE_PORT_WEAK:
            return strength != 0 && ne_emits(engine, state, site_id, profile_id, face, 0, &observed) ? strength : 0;
        case NE_PORT_STRONG:
            return strength != 0 && ne_emits(engine, state, site_id, profile_id, face, 1, &observed) ? strength : 0;
        case NE_PORT_CONDUCTED:
            return ne_query_power(engine, state, NE_TARGET_CONDUCTOR, site_id, include_dust, depth + 1u);
        default: return 0;
    }
}

static uint8_t ne_query_power(NEEngine *engine, NEStateView state, uint8_t target_kind, uint32_t target_id,
                              uint8_t target_port, uint32_t depth) {
    if (depth >= NE_MAX_RECURSION) return 0;
    const NEProgram *program = engine->program;
    const NERange *range = 0;
    if (target_kind == NE_TARGET_INPUT && target_id < program->node_count && target_port < 3u)
        range = &program->input_power_ranges[target_id * 3u + target_port];
    else if (target_kind == NE_TARGET_CONDUCTOR && target_id < program->site_count && target_port < 2u)
        range = &program->conductor_power_ranges[target_id * 2u + target_port];
    if (range == 0 || (uint64_t)range->start + range->count > program->power_link_count) return 0;
    if (target_kind == NE_TARGET_INPUT && target_port == 0u && target_id < program->node_count &&
        program->classic_input_by_node != 0 && state.classic_signals != 0) {
        const NEClassicInputBinding *binding = &program->classic_input_by_node[target_id];
        if (binding->net < program->classic_net_count) {
            const NEClassicNet *net = &program->classic_nets[binding->net];
            uint8_t committed = ne_saturating_sub_power(state.classic_signals[binding->net], binding->attenuation);
            uint8_t live = net->source < program->node_count
                ? ne_saturating_sub_power(state.components[net->source].output_strength, binding->source_attenuation)
                : 0u;
            return committed > live ? committed : live;
        }
    }
#ifdef NE_COMPILED_POWER
    uint8_t compiled_handled = 0;
    uint8_t compiled_power = ne_compiled_power(state, target_kind, target_id, target_port, &compiled_handled);
    if (compiled_handled) {
        engine->counters.compiled_power_queries++;
        if (!engine->verify_compiled_power) return compiled_power;
    }
#endif
    uint8_t maximum = 0;
    for (uint32_t i = 0; i < range->count; ++i) {
        const NEPowerLink *link = &program->power_links[range->start + i];
        if (!ne_guard_matches(engine, state, link->guard, 0, 0)) continue;
        uint8_t power = ne_site_power(engine, state, link->from_site, link->from_port, link->face,
                                      link->include_dust, 0, depth + 1u);
        power = power > link->attenuation ? (uint8_t)(power - link->attenuation) : 0u;
        if (power > maximum) maximum = power;
        if (maximum == 15u) break;
    }
#ifdef NE_COMPILED_POWER
    if (compiled_handled && compiled_power != maximum) {
        engine->status = NE_ERR_BAD_STATE;
        engine->error_node = target_kind == NE_TARGET_INPUT ? target_id : NE_NONE;
    }
#endif
    return maximum;
}

static uint8_t ne_input_strength(NEEngine *engine, NEStateView state, uint32_t node, uint32_t port) {
    return ne_query_power(engine, state, NE_TARGET_INPUT, node, (uint8_t)port, 0);
}

static uint8_t ne_conducted_power(NEEngine *engine, NEStateView state, uint32_t site, uint8_t include_dust) {
    return ne_query_power(engine, state, NE_TARGET_CONDUCTOR, site, include_dust != 0, 0);
}

static void ne_wire_raw_sides(NEEngine *engine, NEStateView state, uint32_t wire_index,
                              uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west);
static uint8_t ne_regulate_wire_side(uint8_t north, uint8_t south, uint8_t east, uint8_t west,
                                    uint8_t raw_north, uint8_t raw_south, uint8_t raw_east, uint8_t raw_west,
                                    uint8_t *out_south, uint8_t *out_east, uint8_t *out_west);
static uint8_t ne_wire_seed_power(NEEngine *engine, NEStateView state, const NEWire *wire,
                                  const NEObserved *captured);
static NEStatus ne_run_wire_hops(NEEngine *engine, NEStateView state, uint32_t node, uint32_t wire_index);
static NEStatus ne_evaluate_isolated_turbo_wire(NEEngine *engine, NEStateView state, uint32_t wire_index);
static NEStatus ne_evaluate_instant_reset_wire(NEEngine *engine, NEStateView state, uint32_t wire_index);

static const uint8_t NE_TURBO_ORDER[4][24] = {
    {2,3,16,19,0,4,1,5,7,8,17,20,12,13,18,21,6,9,22,14,11,10,23,15},
    {2,3,16,19,4,1,5,0,17,20,12,13,18,21,7,8,22,14,11,15,23,9,6,10},
    {2,3,16,19,1,5,0,4,12,13,18,21,7,8,17,20,11,15,23,10,6,14,22,9},
    {2,3,16,19,5,0,4,1,18,21,7,8,17,20,12,13,23,10,6,9,22,15,11,14}
};

static NEStatus ne_set_node(NEEngine *engine, NEStateView state, uint32_t node, uint8_t powered,
                            uint8_t strength, int locked, int notify) {
    if (node >= engine->program->node_count) return NE_ERR_INVALID;
    NEComponentState *current = &state.components[node];
    uint8_t changed = current->powered != powered || current->output_strength != strength ||
                      (locked >= 0 && current->repeater_locked != (uint8_t)locked);
    current->powered = powered;
    current->output_strength = strength;
    if (locked >= 0) current->repeater_locked = (uint8_t)locked;
    if (changed) {
        ne_mark_dirty(engine, node);
        const NENode *spec = &engine->program->nodes[node];
        if (notify && spec->output_route != NE_NONE) return ne_deliver_route(engine, state, spec->output_route);
    }
    return NE_OK;
}

static int ne_site_facts(NEEngine *engine, NEStateView state, uint32_t site, const NECaptureSet *capture,
                         uint32_t *profile_id, int *solid, int *transparent, int *is_wire) {
    NEObserved observed;
    if (!ne_resolve_site(engine, state, site, capture, &observed)) return 0;
    const NEProfile *profile = ne_profile(engine->program, observed.block_profile);
    if (profile == 0) return 0;
    if (profile_id != 0) *profile_id = observed.block_profile;
    if (solid != 0) *solid = profile->solid != 0;
    if (transparent != 0) *transparent = profile->transparent != 0;
    if (is_wire != 0) *is_wire = (observed.flags & NE_OBS_WIRE) != 0 || (profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0;
    return 1;
}

static uint8_t ne_wire_strength_site(NEEngine *engine, NEStateView state, uint32_t site, const NECaptureSet *capture) {
    NEObserved observed;
    if (!ne_resolve_site(engine, state, site, capture, &observed)) return 0;
    const NEProfile *profile = ne_profile(engine->program, observed.block_profile);
    if ((observed.flags & NE_OBS_WIRE) != 0) return observed.wire_power;
    if (profile != 0 && (profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0)
        return (uint8_t)(profile->wire_bits & 15u);
    return 0;
}

static uint8_t ne_power_no_dust(NEEngine *engine, NEStateView state, uint32_t site, uint8_t face,
                                const NECaptureSet *capture, uint32_t depth) {
    if (depth >= NE_MAX_RECURSION) return 0;
    NEObserved observed;
    if (!ne_resolve_site(engine, state, site, capture, &observed)) return 0;
    const NEProfile *profile = ne_profile(engine->program, observed.block_profile);
    if (profile == 0) return 0;
    if (profile->solid) return ne_conducted_power(engine, state, site, 0u);
    if ((observed.flags & NE_OBS_WIRE) != 0 || (profile->flags & NE_PROFILE_FLAG_IS_WIRE) != 0) return 0;
    uint8_t strength = ne_source_strength(engine, state, site, &observed);
    return strength != 0 && ne_emits(engine, state, site, observed.block_profile, face, 0, &observed) ? strength : 0;
}

static int ne_wire_geometry(NEEngine *engine, NEStateView state, const NEWire *wire, uint32_t slot,
                            const NECaptureSet *capture, int *solid, int *transparent) {
    if (slot >= 24u) return 0;
    uint32_t bit = 1u << slot;
    if ((wire->flags & NE_WIRE_FLAG_QUERY_FACTS) != 0 && (wire->query_known & bit) != 0) {
        if (solid != 0) *solid = (wire->query_solid & bit) != 0;
        if (transparent != 0) *transparent = (wire->query_transparent & bit) != 0;
        return 1;
    }
    return ne_site_facts(engine, state, wire->neighbors[slot], capture, 0, solid, transparent, 0);
}

static uint8_t ne_wire_neighbor_power_no_dust(NEEngine *engine, NEStateView state, const NEWire *wire,
                                               uint32_t slot, uint8_t face, const NECaptureSet *capture) {
    uint32_t bit = 1u << slot;
    uint32_t site = wire->neighbors[slot];
    if ((wire->flags & NE_WIRE_FLAG_QUERY_FACTS) != 0) {
        if ((wire->query_zero_no_dust & bit) != 0) return 0;
        if ((wire->query_solid & bit) != 0) return ne_conducted_power(engine, state, site, 0u);
    }
    return ne_power_no_dust(engine, state, site, face, capture, 0);
}

static uint32_t ne_wire_selected_inputs(NEEngine *engine, NEStateView state, const NEWire *wire,
                                         uint32_t direct, uint32_t conditional, int turbo,
                                         const NECaptureSet *capture) {
    uint32_t selected = direct;
    int center_up_solid = 0;
    (void)ne_wire_geometry(engine, state, wire, 3u, capture, &center_up_solid, 0);
    while (conditional != 0) {
        uint32_t slot = 0;
        while (slot < 24u && ((conditional >> slot) & 1u) == 0u) ++slot;
        if (slot == 24u) break;
        conditional &= conditional - 1u;
        uint32_t horizontal;
        int ascent;
        switch (slot) {
            case 7u: horizontal = 0u; ascent = 0; break;
            case 8u: horizontal = 0u; ascent = 1; break;
            case 12u: horizontal = 1u; ascent = 0; break;
            case 13u: horizontal = 1u; ascent = 1; break;
            case 17u: horizontal = 4u; ascent = 0; break;
            case 20u: horizontal = 4u; ascent = 1; break;
            case 18u: horizontal = 5u; ascent = 0; break;
            case 21u: horizontal = 5u; ascent = 1; break;
            default: continue;
        }
        int solid = 0, transparent = 0;
        (void)ne_wire_geometry(engine, state, wire, horizontal, capture, &solid, &transparent);
        int enabled = ascent ? ((!turbo || solid) && !transparent && !center_up_solid) : !solid;
        if (enabled) selected |= 1u << slot;
    }
    return selected;
}

static void ne_wire_set_side(uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west, uint8_t face, uint8_t value) {
    switch (face) {
        case 2: *north = value; break;
        case 3: *south = value; break;
        case 5: *east = value; break;
        case 4: *west = value; break;
        default: break;
    }
}

static void ne_wire_raw_sides(NEEngine *engine, NEStateView state, uint32_t wire_index,
                              uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west) {
    const NEProgram *program = engine->program;
    *north = *south = *east = *west = NE_WIRE_SIDE_NONE;
    if (wire_index >= program->wire_count) return;
    const NEWire *wire = &program->wires[wire_index];
    int up_solid = 0;
    (void)ne_site_facts(engine, state, wire->neighbors[3], 0, 0, &up_solid, 0, 0);
    const uint8_t faces[4] = {2u, 3u, 5u, 4u}; /* north, south, east, west */
    const uint8_t neighbor_slots[4] = {4u, 5u, 1u, 0u};
    const uint8_t above_slots[4] = {20u, 21u, 13u, 8u};
    const uint8_t below_slots[4] = {17u, 18u, 12u, 7u};
    uint8_t *out[4] = {north, south, east, west};
    for (uint32_t i = 0; i < 4u; ++i) {
        uint32_t neighbor_site = wire->neighbors[neighbor_slots[i]];
        uint32_t neighbor_profile = NE_NONE;
        int neighbor_solid = 0, neighbor_wire = 0;
        (void)ne_site_facts(engine, state, neighbor_site, 0, &neighbor_profile, &neighbor_solid, 0, &neighbor_wire);
        const NEProfile *neighbor_facts = ne_profile(program, neighbor_profile);
        if (neighbor_facts != 0 && (neighbor_facts->wire_connect_faces & (1u << faces[i])) != 0) {
            *out[i] = NE_WIRE_SIDE_SIDE;
            continue;
        }
        int above_wire = 0;
        int below_wire = 0;
        (void)ne_site_facts(engine, state, wire->neighbors[above_slots[i]], 0, 0, 0, 0, &above_wire);
        (void)ne_site_facts(engine, state, wire->neighbors[below_slots[i]], 0, 0, 0, 0, &below_wire);
        if (!up_solid && above_wire) *out[i] = NE_WIRE_SIDE_UP;
        else if (!neighbor_solid && below_wire) *out[i] = NE_WIRE_SIDE_SIDE;
    }
}

static uint8_t ne_regulate_wire_side(uint8_t north, uint8_t south, uint8_t east, uint8_t west,
                                    uint8_t raw_north, uint8_t raw_south, uint8_t raw_east, uint8_t raw_west,
                                    uint8_t *out_south, uint8_t *out_east, uint8_t *out_west) {
    uint8_t rn = raw_north, rs = raw_south, re = raw_east, rw = raw_west;
    if (north == NE_WIRE_SIDE_NONE && south == NE_WIRE_SIDE_NONE && east == NE_WIRE_SIDE_NONE && west == NE_WIRE_SIDE_NONE &&
        rn == NE_WIRE_SIDE_NONE && rs == NE_WIRE_SIDE_NONE && re == NE_WIRE_SIDE_NONE && rw == NE_WIRE_SIDE_NONE) {
        *out_south = rs; *out_east = re; *out_west = rw;
        return rn;
    }
    int north_none = rn == NE_WIRE_SIDE_NONE;
    int south_none = rs == NE_WIRE_SIDE_NONE;
    int east_none = re == NE_WIRE_SIDE_NONE;
    int west_none = rw == NE_WIRE_SIDE_NONE;
    int ns_none = north_none && south_none;
    int ew_none = east_none && west_none;
    if (north_none && ew_none) rn = NE_WIRE_SIDE_SIDE;
    if (south_none && ew_none) rs = NE_WIRE_SIDE_SIDE;
    if (east_none && ns_none) re = NE_WIRE_SIDE_SIDE;
    if (west_none && ns_none) rw = NE_WIRE_SIDE_SIDE;
    *out_south = rs; *out_east = re; *out_west = rw;
    return rn;
}

static int ne_wire_is_dot(uint8_t north, uint8_t south, uint8_t east, uint8_t west) {
    return north == NE_WIRE_SIDE_NONE && south == NE_WIRE_SIDE_NONE && east == NE_WIRE_SIDE_NONE && west == NE_WIRE_SIDE_NONE;
}

static int ne_wire_is_cross(uint8_t north, uint8_t south, uint8_t east, uint8_t west) {
    return north == NE_WIRE_SIDE_SIDE && south == NE_WIRE_SIDE_SIDE && east == NE_WIRE_SIDE_SIDE && west == NE_WIRE_SIDE_SIDE;
}

static void ne_wire_make_cross(uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west) {
    *north = *south = *east = *west = NE_WIRE_SIDE_SIDE;
}

static void ne_wire_state_from_observed(const NEComponentState *current, const NEObserved *captured,
                                        uint8_t *north, uint8_t *south, uint8_t *east, uint8_t *west, uint8_t *power) {
    if (captured != 0 && (captured->flags & NE_OBS_WIRE) != 0) {
        *north = captured->wire_north; *south = captured->wire_south;
        *east = captured->wire_east; *west = captured->wire_west; *power = captured->wire_power;
    } else {
        *north = current->wire_north; *south = current->wire_south;
        *east = current->wire_east; *west = current->wire_west; *power = current->wire_power;
    }
}

static int ne_set_wire_state(NEEngine *engine, NEStateView state, uint32_t node,
                             uint8_t north, uint8_t south, uint8_t east, uint8_t west, uint8_t power) {
    NEComponentState *current = &state.components[node];
    if (current->wire_north == north && current->wire_south == south && current->wire_east == east &&
        current->wire_west == west && current->wire_power == power) return 0;
    current->wire_north = north; current->wire_south = south;
    current->wire_east = east; current->wire_west = west; current->wire_power = power;
    ne_mark_dirty(engine, node);
    return 1;
}

static uint8_t ne_wire_seed_power(NEEngine *engine, NEStateView state, const NEWire *wire,
                                  const NEObserved *captured) {
    const NEProgram *program = engine->program;
    (void)captured;
    if (wire->dust_path < program->dust_path_count && program->dust_paths != 0 && program->dust_path_wires != 0) {
        const NEDustPath *path = &program->dust_paths[wire->dust_path];
        uint32_t position = wire->dust_path_position;
        if (position < path->wire_count && (uint64_t)path->first_wire + path->wire_count <= program->dust_path_wire_count &&
            program->dust_path_wires[path->first_wire + position] < program->wire_count &&
            program->dust_path_wires[path->first_wire + position] == (uint32_t)(wire - program->wires)) {
            engine->counters.wire_dust_path_evaluations++;
            uint8_t block_power = 0u;
            if (position == 0u)
                block_power = ne_power_no_dust(engine, state, path->input_site, path->input_face, 0, 0u);
            uint8_t wire_power = 0u;
            if (position != 0u) {
                uint32_t neighbor = program->dust_path_wires[path->first_wire + position - 1u];
                if (neighbor < program->wire_count) {
                    uint32_t node = program->wires[neighbor].node;
                    if (node < program->node_count) wire_power = state.components[node].wire_power;
                }
            }
            if (position + 1u < path->wire_count) {
                uint32_t neighbor = program->dust_path_wires[path->first_wire + position + 1u];
                if (neighbor < program->wire_count) {
                    uint32_t node = program->wires[neighbor].node;
                    if (node < program->node_count && state.components[node].wire_power > wire_power)
                        wire_power = state.components[node].wire_power;
                }
            }
            uint8_t attenuated = wire_power > 0u ? (uint8_t)(wire_power - 1u) : 0u;
            return block_power > attenuated ? block_power : attenuated;
        }
    }
    if ((wire->flags & (NE_WIRE_FLAG_QUERY_FACTS | NE_WIRE_FLAG_TRANSFER)) ==
        (NE_WIRE_FLAG_QUERY_FACTS | NE_WIRE_FLAG_TRANSFER)) {
        engine->counters.wire_query_fact_evaluations++;
        uint8_t block_power = 0u;
        const uint8_t faces[6] = {1u, 0u, 2u, 3u, 5u, 4u};
        const uint8_t slots[6] = {3u, 2u, 4u, 5u, 1u, 0u};
        for (uint32_t i = 0; i < 6u; ++i) {
            uint8_t strength = ne_wire_neighbor_power_no_dust(engine, state, wire, slots[i], faces[i], 0);
            if (strength > block_power) block_power = strength;
        }
        uint32_t inputs = ne_wire_selected_inputs(engine, state, wire, wire->seed_direct,
                                                  wire->seed_conditional, 0, 0);
        uint8_t wire_power = 0u;
        while (inputs != 0u) {
            uint32_t slot = 0;
            while (slot < 24u && ((inputs >> slot) & 1u) == 0u) ++slot;
            if (slot == 24u) break;
            inputs &= inputs - 1u;
            uint8_t strength = ne_wire_strength_site(engine, state, wire->neighbors[slot], 0);
            if (strength > wire_power) wire_power = strength;
        }
        uint8_t attenuated = wire_power > 0u ? (uint8_t)(wire_power - 1u) : 0u;
        return block_power > attenuated ? block_power : attenuated;
    }
    uint8_t block_power = 0, wire_power = 0;
    int up_solid = 0;
    (void)ne_site_facts(engine, state, wire->neighbors[3], 0, 0, &up_solid, 0, 0);
    const uint8_t faces[6] = {1u, 0u, 2u, 3u, 5u, 4u};
    const uint8_t slots[6] = {3u, 2u, 4u, 5u, 1u, 0u};
    for (uint32_t i = 0; i < 6u; ++i) {
        uint32_t site = wire->neighbors[slots[i]];
        int solid = 0, transparent = 0, is_wire = 0;
        (void)ne_site_facts(engine, state, site, 0, 0, &solid, &transparent, &is_wire);
        uint8_t strength = ne_power_no_dust(engine, state, site, faces[i], 0, 0);
        if (strength > block_power) block_power = strength;
        uint8_t neighbor_wire_power = is_wire ? ne_wire_strength_site(engine, state, site, 0) : 0u;
        if (neighbor_wire_power > wire_power) wire_power = neighbor_wire_power;
        if (i >= 2u) {
            uint8_t above_slot = i == 2u ? 20u : i == 3u ? 21u : i == 4u ? 13u : 8u;
            uint8_t below_slot = i == 2u ? 17u : i == 3u ? 18u : i == 4u ? 12u : 7u;
            if (!up_solid && !transparent) {
                uint32_t above = wire->neighbors[above_slot];
                if (ne_site_facts(engine, state, above, 0, 0, 0, 0, &is_wire) && is_wire) {
                    neighbor_wire_power = ne_wire_strength_site(engine, state, above, 0);
                    if (neighbor_wire_power > wire_power) wire_power = neighbor_wire_power;
                }
            }
            if (!solid) {
                uint32_t below = wire->neighbors[below_slot];
                if (ne_site_facts(engine, state, below, 0, 0, 0, 0, &is_wire) && is_wire) {
                    neighbor_wire_power = ne_wire_strength_site(engine, state, below, 0);
                    if (neighbor_wire_power > wire_power) wire_power = neighbor_wire_power;
                }
            }
        }
    }
    uint8_t attenuated = wire_power > 0 ? (uint8_t)(wire_power - 1u) : 0u;
    return block_power > attenuated ? block_power : attenuated;
}

static NEStatus ne_update_wire_shape(NEEngine *engine, NEStateView state, uint32_t node, uint8_t direction,
                                     const NEObserved *captured) {
    if (direction == 1u) return NE_OK;
    const NENode *node_spec = &engine->program->nodes[node];
    uint32_t wire_index = node_spec->kind_data[0];
    if (wire_index >= engine->program->wire_count) return NE_ERR_INVALID;
    const NEComponentState *current = &state.components[node];
    uint8_t north, south, east, west, power;
    ne_wire_state_from_observed(current, captured, &north, &south, &east, &west, &power);
    uint8_t raw_north, raw_south, raw_east, raw_west;
    ne_wire_raw_sides(engine, state, wire_index, &raw_north, &raw_south, &raw_east, &raw_west);
    uint8_t old_north = north, old_south = south, old_east = east, old_west = west;
    if (direction == 0u) {
        north = ne_regulate_wire_side(north, south, east, west, raw_north, raw_south, raw_east, raw_west,
                                      &south, &east, &west);
    } else {
        uint8_t updated_side = NE_WIRE_SIDE_NONE;
        switch (direction) {
            case 2: updated_side = raw_south; break; /* north neighbor writes south side */
            case 3: updated_side = raw_north; break;
            case 5: updated_side = raw_west; break;
            case 4: updated_side = raw_east; break;
            default: return NE_OK;
        }
        uint8_t side_face = direction == 2u ? 3u : direction == 3u ? 2u : direction == 5u ? 4u : 5u;
        ne_wire_set_side(&north, &south, &east, &west, side_face, updated_side);
        uint8_t regulated_north, regulated_south, regulated_east, regulated_west;
        regulated_north = ne_regulate_wire_side(north, south, east, west, raw_north, raw_south, raw_east, raw_west,
                                                 &regulated_south, &regulated_east, &regulated_west);
        north = regulated_north; south = regulated_south; east = regulated_east; west = regulated_west;
        if (ne_wire_is_cross(old_north, old_south, old_east, old_west) && updated_side == NE_WIRE_SIDE_NONE) {
            north = old_north; south = old_south; east = old_east; west = old_west;
        } else if (!ne_wire_is_dot(old_north, old_south, old_east, old_west) && ne_wire_is_dot(north, south, east, west)) {
            ne_wire_make_cross(&north, &south, &east, &west);
        }
    }
    if (ne_set_wire_state(engine, state, node, north, south, east, west, power)) {
        const NEWire *wire = &engine->program->wires[wire_index];
        if (wire->shape_route != NE_NONE) return ne_deliver_route(engine, state, wire->shape_route);
    }
    return NE_OK;
}

static NEStatus ne_evaluate_seed(NEEngine *engine, NEStateView state, uint32_t node, const NEObserved *captured) {
    const NENode *spec = &engine->program->nodes[node];
    uint32_t wire_index = spec->kind_data[0];
    if (wire_index >= engine->program->wire_count) return NE_ERR_INVALID;
    const NEWire *wire = &engine->program->wires[wire_index];
    const NEComponentState *current = &state.components[node];
    uint8_t north, south, east, west, power;
    ne_wire_state_from_observed(current, captured, &north, &south, &east, &west, &power);
    uint8_t next_power = ne_wire_seed_power(engine, state, wire, captured);
    if (power == next_power) return NE_OK;
    if (ne_set_wire_state(engine, state, node, north, south, east, west, next_power)) {
        engine->counters.wire_walks++;
        if ((wire->flags & NE_WIRE_FLAG_INSTANT_RESET) != 0)
            return ne_evaluate_instant_reset_wire(engine, state, wire_index);
        if ((wire->flags & NE_WIRE_FLAG_ISOLATED) != 0)
            return ne_evaluate_isolated_turbo_wire(engine, state, wire_index);
        return ne_run_wire_hops(engine, state, node, wire_index);
    }
    return NE_OK;
}

static int ne_node_pending(const NEEngine *engine, uint32_t node) {
    return node < engine->program->node_count && engine->pending_standard[node] != 0;
}

static NEStatus ne_update_component(NEEngine *engine, NEStateView state, uint32_t node,
                                    uint8_t delivery_kind, uint8_t direction, const NEObserved *captured);

static int ne_walk_frame_init(NEEngine *engine, NEWalkFrame *frame) {
    uint32_t map_count = engine->program->site_count;
    if (map_count != 0 && frame->slot_of_site == 0) {
        frame->slot_of_site = (uint32_t *)ne_alloc(engine, (size_t)map_count * sizeof(uint32_t), _Alignof(uint32_t));
        if (frame->slot_of_site == 0) return 0;
        ne_bytes_zero(frame->slot_of_site, (size_t)map_count * sizeof(uint32_t));
    }
    if (map_count != 0 && frame->generation_of_site == 0) {
        frame->generation_of_site = (uint32_t *)ne_alloc(engine, (size_t)map_count * sizeof(uint32_t), _Alignof(uint32_t));
        if (frame->generation_of_site == 0) return 0;
        ne_bytes_zero(frame->generation_of_site, (size_t)map_count * sizeof(uint32_t));
    }
    return 1;
}

static int ne_walk_sites_reserve(NEEngine *engine, NEWalkFrame *frame, uint32_t needed) {
    if (needed <= frame->site_capacity) return 1;
    uint32_t capacity = ne_next_capacity(frame->site_capacity, needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(NEWalkSite)) return 0;
    NEWalkSite *sites = (NEWalkSite *)ne_alloc(engine, (size_t)capacity * sizeof(NEWalkSite), _Alignof(NEWalkSite));
    if (sites == 0) return 0;
    if (frame->site_count != 0) ne_bytes_copy(sites, frame->sites, (size_t)frame->site_count * sizeof(NEWalkSite));
    ne_free(engine, frame->sites, (size_t)frame->site_capacity * sizeof(NEWalkSite), _Alignof(NEWalkSite));
    frame->sites = sites;
    frame->site_capacity = capacity;
    return 1;
}

static int ne_walk_queue_reserve(NEEngine *engine, NEWalkFrame *frame, uint32_t queue_index, uint32_t needed) {
    if (queue_index >= 3u) return 0;
    if (needed <= frame->queue_capacity[queue_index]) return 1;
    uint32_t capacity = ne_next_capacity(frame->queue_capacity[queue_index], needed);
    if (capacity == 0 || (size_t)capacity > SIZE_MAX / sizeof(uint32_t)) return 0;
    uint32_t *items = (uint32_t *)ne_alloc(engine, (size_t)capacity * sizeof(uint32_t), _Alignof(uint32_t));
    if (items == 0) return 0;
    if (frame->queue_count[queue_index] != 0)
        ne_bytes_copy(items, frame->queues[queue_index], (size_t)frame->queue_count[queue_index] * sizeof(uint32_t));
    ne_free(engine, frame->queues[queue_index], (size_t)frame->queue_capacity[queue_index] * sizeof(uint32_t), _Alignof(uint32_t));
    frame->queues[queue_index] = items;
    frame->queue_capacity[queue_index] = capacity;
    return 1;
}

static int ne_walk_queue_push(NEEngine *engine, NEWalkFrame *frame, uint32_t queue_index, uint32_t slot) {
    if (!ne_walk_queue_reserve(engine, frame, queue_index, frame->queue_count[queue_index] + 1u)) return 0;
    frame->queues[queue_index][frame->queue_count[queue_index]++] = slot;
    return 1;
}

static NEWalkFrame *ne_walk_acquire(NEEngine *engine) {
    uint32_t index = engine->walk_depth;
    if (index >= NE_MAX_RECURSION) return 0;
    if (engine->walk_frames == 0) {
        /* Nested walks keep pointers to their descriptors on the C stack.
           Allocate the bounded descriptor pool once so deeper recursion can
           never relocate an outer frame. Per-frame maps/sites remain lazy. */
        if ((size_t)NE_MAX_RECURSION > SIZE_MAX / sizeof(NEWalkFrame)) return 0;
        NEWalkFrame *frames = (NEWalkFrame *)ne_alloc(engine,
            (size_t)NE_MAX_RECURSION * sizeof(NEWalkFrame), _Alignof(NEWalkFrame));
        if (frames == 0) return 0;
        ne_bytes_zero(frames, (size_t)NE_MAX_RECURSION * sizeof(NEWalkFrame));
        engine->walk_frames = frames;
        engine->walk_frame_capacity = NE_MAX_RECURSION;
    }
    if (index >= engine->walk_frame_count) engine->walk_frame_count = index + 1u;
    NEWalkFrame *frame = &engine->walk_frames[index];
    if (!ne_walk_frame_init(engine, frame)) return 0;
    frame->site_count = 0;
    frame->queue_count[0] = frame->queue_count[1] = frame->queue_count[2] = 0;
    frame->layer = 0;
    frame->generation++;
    if (frame->generation == 0) {
        ne_bytes_zero(frame->generation_of_site, (size_t)engine->program->site_count * sizeof(uint32_t));
        frame->generation = 1;
    }
    engine->walk_depth++;
    return frame;
}

static void ne_walk_release(NEEngine *engine) {
    if (engine->walk_depth != 0) engine->walk_depth--;
}

static NECaptureSet ne_walk_capture_set(NEEngine *engine, NEWalkFrame *frame) {
    NECaptureSet capture;
    capture.items = (NEObserved *)frame->sites;
    capture.count = frame->site_count;
    capture.capacity = engine->program->site_count;
    capture.slot_of_site = frame->slot_of_site;
    capture.generation_of_site = frame->generation_of_site;
    capture.generation = frame->generation;
    capture.item_stride = sizeof(NEWalkSite);
    return capture;
}

static int ne_walk_capture_site(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                NECaptureSet *capture, uint32_t site) {
    if (site >= engine->program->site_count) return 0;
    if (frame->generation_of_site[site] == frame->generation) return 1;
    if (!ne_walk_sites_reserve(engine, frame, frame->site_count + 1u)) return 0;
    uint32_t slot = frame->site_count;
    NEWalkSite *walk_site = &frame->sites[slot];
    ne_bytes_zero(walk_site, sizeof(*walk_site));
    if (!ne_resolve_site(engine, state, site, capture, &walk_site->observed)) return 0;
    walk_site->observed.site = site;
    frame->slot_of_site[site] = slot;
    frame->generation_of_site[site] = frame->generation;
    frame->site_count++;
    capture->items = (NEObserved *)frame->sites;
    capture->count = frame->site_count;
    return 1;
}

static uint32_t ne_walk_slot(const NEWalkFrame *frame, uint32_t site) {
    if (site == NE_NONE) return NE_NONE;
    return frame->generation_of_site[site] == frame->generation ? frame->slot_of_site[site] : NE_NONE;
}

static uint8_t ne_compute_heading(int x_bias, int z_bias) {
    int code = (x_bias + 1) + 3 * (z_bias + 1);
    switch (code) {
        case 0: case 1: return 0;
        case 2: case 5: return 1;
        case 3: case 4: return 3;
        case 6: case 7: case 8: return 2;
        default: return 3;
    }
}

static int ne_walk_identify_neighbors(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                      NECaptureSet *capture, uint32_t node, uint32_t *root_slot_out,
                                      uint32_t neighbors_out[24], uint8_t *heading_out) {
    const NEProgram *program = engine->program;
    if (node >= program->node_count || program->component_sites == 0) return 0;
    uint32_t root_site = program->component_sites[node];
    uint32_t wire_index = program->nodes[node].kind_data[0];
    if (root_site >= program->site_count || wire_index >= program->wire_count) return 0;
    if (!ne_walk_capture_site(engine, state, frame, capture, root_site)) return 0;
    uint32_t root_slot = ne_walk_slot(frame, root_site);
    if (root_slot == NE_NONE) return 0;
    NEWalkSite *root = &frame->sites[root_slot];
    if (root->heading_valid) {
        for (uint32_t i = 0; i < 24u; ++i) neighbors_out[i] = root->neighbor_slots[i];
        *heading_out = root->heading;
        *root_slot_out = root_slot;
        return 1;
    }
    const NEWire *wire = &program->wires[wire_index];
    int old_x = root->x_bias;
    int old_z = root->z_bias;
    for (uint32_t i = 0; i < 24u; ++i) {
        if (!ne_walk_capture_site(engine, state, frame, capture, wire->neighbors[i])) return 0;
        neighbors_out[i] = ne_walk_slot(frame, wire->neighbors[i]);
        if (neighbors_out[i] == NE_NONE) return 0;
    }
    int from_west = frame->sites[neighbors_out[0]].visited || frame->sites[neighbors_out[7]].visited || frame->sites[neighbors_out[8]].visited;
    int from_east = frame->sites[neighbors_out[1]].visited || frame->sites[neighbors_out[12]].visited || frame->sites[neighbors_out[13]].visited;
    int from_north = frame->sites[neighbors_out[4]].visited || frame->sites[neighbors_out[17]].visited || frame->sites[neighbors_out[20]].visited;
    int from_south = frame->sites[neighbors_out[5]].visited || frame->sites[neighbors_out[18]].visited || frame->sites[neighbors_out[21]].visited;
    int x_bias = from_west - from_east;
    int z_bias = from_north - from_south;
    if (x_bias == 0 && z_bias == 0) {
        x_bias = old_x;
        z_bias = old_z;
    } else if (x_bias != 0 && z_bias != 0) {
        if (old_x != 0) z_bias = 0;
        if (old_z != 0) x_bias = 0;
    }
    uint8_t heading = ne_compute_heading(x_bias, z_bias);
    for (uint32_t i = 0; i < 24u; ++i) {
        frame->sites[neighbors_out[i]].x_bias = (int8_t)x_bias;
        frame->sites[neighbors_out[i]].z_bias = (int8_t)z_bias;
    }
    root = &frame->sites[root_slot];
    root->heading = heading;
    root->heading_valid = 1;
    root->neighbors_valid = 1;
    for (uint32_t i = 0; i < 24u; ++i) root->neighbor_slots[i] = neighbors_out[i];
    *heading_out = heading;
    *root_slot_out = root_slot;
    return 1;
}

static const NEDelivery *ne_turbo_delivery(NEEngine *engine, NEStateView state, uint32_t wire_index,
                                           uint32_t slot, uint32_t site, const NECaptureSet *capture) {
    const NEProgram *program = engine->program;
    if (wire_index >= program->wire_count || slot >= 24u) return 0;
    const NEWire *wire = &program->wires[wire_index];
    if (wire->turbo_route >= program->route_count) return 0;
    const NERoute *route = &program->routes[wire->turbo_route];
    uint32_t start = route->first_delivery;
    uint32_t end = start + route->delivery_count;
    if (wire->turbo_offsets[0] != NE_NONE) {
        uint32_t begin_offset = wire->turbo_offsets[slot];
        uint32_t end_offset = wire->turbo_offsets[slot + 1u];
        if (begin_offset > end_offset || end_offset > route->delivery_count) return 0;
        start += begin_offset;
        end = route->first_delivery + end_offset;
    }
    if (end > program->delivery_count) return 0;
    for (uint32_t i = start; i < end; ++i) {
        const NEDelivery *delivery = &program->deliveries[i];
        if (delivery->target_site == site && delivery->receiver_kind != NE_RECEIVER_NONE && delivery->view == 1u &&
            ne_guard_matches(engine, state, delivery->guard, capture, 0)) return delivery;
    }
    return 0;
}

static void ne_walk_rotate_queues(NEWalkFrame *frame) {
    uint32_t *queue = frame->queues[0];
    frame->queues[0] = frame->queues[1];
    frame->queues[1] = frame->queues[2];
    frame->queues[2] = queue;
    uint32_t count = frame->queue_count[0];
    frame->queue_count[0] = frame->queue_count[1];
    frame->queue_count[1] = frame->queue_count[2];
    frame->queue_count[2] = 0;
    uint32_t capacity = frame->queue_capacity[0];
    frame->queue_capacity[0] = frame->queue_capacity[1];
    frame->queue_capacity[1] = frame->queue_capacity[2];
    frame->queue_capacity[2] = capacity;
    (void)count;
}

static NEStatus ne_propagate_wire_changes(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                          NECaptureSet *capture, uint32_t node, uint32_t wire_index,
                                          uint32_t layer) {
    const NEWire *wire = &engine->program->wires[wire_index];
    if (wire->turbo_route == NE_NONE) return NE_OK;
    uint32_t root_slot, neighbors[24];
    uint8_t heading;
    if (!ne_walk_identify_neighbors(engine, state, frame, capture, node, &root_slot, neighbors, &heading)) return NE_ERR_ALLOC;
    (void)root_slot;
    for (uint32_t i = 0; i < 24u; ++i) {
        uint32_t slot = NE_TURBO_ORDER[heading][i];
        uint32_t walk_slot = neighbors[slot];
        uint32_t next_layer = layer + 1u;
        if (next_layer <= frame->sites[walk_slot].layer) continue;
        frame->sites[walk_slot].layer = next_layer;
        if (ne_turbo_delivery(engine, state, wire_index, slot, frame->sites[walk_slot].observed.site, capture) != 0 &&
            !ne_walk_queue_push(engine, frame, 1u, walk_slot)) return NE_ERR_ALLOC;
    }
    for (uint32_t i = 0; i < 4u; ++i) {
        uint32_t slot = NE_TURBO_ORDER[heading][i];
        uint32_t walk_slot = neighbors[slot];
        uint32_t next_layer = layer + 2u;
        if (next_layer <= frame->sites[walk_slot].layer) continue;
        frame->sites[walk_slot].layer = next_layer;
        if (ne_turbo_delivery(engine, state, wire_index, slot, frame->sites[walk_slot].observed.site, capture) != 0 &&
            !ne_walk_queue_push(engine, frame, 2u, walk_slot)) return NE_ERR_ALLOC;
    }
    return NE_OK;
}

static uint8_t ne_walk_wire_power(const NEWalkFrame *frame, uint32_t slot) {
    const NEObserved *observed = &frame->sites[slot].observed;
    return (observed->flags & NE_OBS_WIRE) != 0 ? observed->wire_power : 0u;
}

static uint8_t ne_turbo_wire_power(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                   NECaptureSet *capture, uint32_t wire_index,
                                   const uint32_t neighbors[24], uint8_t heading) {
    const NEProgram *program = engine->program;
    const NEWire *wire = &engine->program->wires[wire_index];
    if (wire->dust_path < program->dust_path_count && program->dust_paths != 0 && program->dust_path_wires != 0) {
        const NEDustPath *path = &program->dust_paths[wire->dust_path];
        uint32_t position = wire->dust_path_position;
        if (position < path->wire_count && (uint64_t)path->first_wire + path->wire_count <= program->dust_path_wire_count &&
            program->dust_path_wires[path->first_wire + position] < program->wire_count &&
            program->dust_path_wires[path->first_wire + position] == wire_index) {
            uint8_t input_power = 0u;
            int valid = 1;
            if (position == 0u) {
                const NEObserved *observed = ne_capture_find(capture, path->input_site);
                if (observed == 0) valid = 0;
                else input_power = ne_power_no_dust(engine, state, path->input_site, path->input_face, capture, 0u);
            }
            uint8_t wire_power = 0u;
            if (input_power < 15u) {
                for (uint32_t adjacent = 0; adjacent < 2u && valid; ++adjacent) {
                    uint32_t neighbor_position;
                    if (adjacent == 0u) {
                        if (position == 0u) continue;
                        neighbor_position = position - 1u;
                    } else {
                        if (position + 1u >= path->wire_count) continue;
                        neighbor_position = position + 1u;
                    }
                    uint32_t neighbor_wire = program->dust_path_wires[path->first_wire + neighbor_position];
                    if (neighbor_wire >= program->wire_count || program->component_sites == 0) { valid = 0; break; }
                    uint32_t neighbor_node = program->wires[neighbor_wire].node;
                    if (neighbor_node >= program->node_count || program->component_sites[neighbor_node] >= program->site_count) {
                        valid = 0;
                        break;
                    }
                    const NEObserved *observed = ne_capture_find(capture, program->component_sites[neighbor_node]);
                    if (observed == 0 || (observed->flags & NE_OBS_WIRE) == 0) { valid = 0; break; }
                    if (observed->wire_power > wire_power) wire_power = observed->wire_power;
                }
            }
            if (valid) {
                engine->counters.wire_dust_path_evaluations++;
                uint8_t attenuated = wire_power > 0u ? (uint8_t)(wire_power - 1u) : 0u;
                return input_power > attenuated ? input_power : attenuated;
            }
        }
    }
    if ((wire->flags & (NE_WIRE_FLAG_QUERY_FACTS | NE_WIRE_FLAG_TRANSFER)) ==
        (NE_WIRE_FLAG_QUERY_FACTS | NE_WIRE_FLAG_TRANSFER)) {
        engine->counters.wire_query_fact_evaluations++;
        const uint8_t faces[6] = {1u, 0u, 2u, 3u, 5u, 4u};
        const uint8_t direct_slots[6] = {3u, 2u, 4u, 5u, 1u, 0u};
        uint8_t direct_power = 0u;
        for (uint32_t i = 0; i < 6u; ++i) {
            uint8_t power = ne_wire_neighbor_power_no_dust(engine, state, wire, direct_slots[i], faces[i], capture);
            if (power > direct_power) direct_power = power;
            if (direct_power == 15u) break;
        }
        uint8_t wire_power = 0u;
        if (direct_power < 15u) {
            uint32_t inputs = ne_wire_selected_inputs(engine, state, wire, wire->turbo_direct,
                                                      wire->turbo_conditional, 1, capture);
            while (inputs != 0u) {
                uint32_t slot = 0;
                while (slot < 24u && ((inputs >> slot) & 1u) == 0u) ++slot;
                if (slot == 24u) break;
                inputs &= inputs - 1u;
                uint8_t power = ne_walk_wire_power(frame, neighbors[slot]);
                if (power > wire_power) wire_power = power;
            }
        }
        uint8_t attenuated = wire_power > 0u ? (uint8_t)(wire_power - 1u) : 0u;
        return direct_power > attenuated ? direct_power : attenuated;
    }
    const uint8_t faces[6] = {1u, 0u, 2u, 3u, 5u, 4u};
    const uint8_t direct_slots[6] = {3u, 2u, 4u, 5u, 1u, 0u};
    uint8_t direct_power = 0;
    for (uint32_t i = 0; i < 6u; ++i) {
        uint32_t slot = direct_slots[i];
        uint8_t power = ne_power_no_dust(engine, state, wire->neighbors[slot], faces[i], capture, 0);
        if (power > direct_power) direct_power = power;
        if (direct_power == 15u) break;
    }
    uint8_t diagonal_power = 0;
    if (direct_power < 15u) {
        const uint8_t *order = NE_TURBO_ORDER[heading];
        uint32_t center_up = neighbors[order[1]];
        int center_up_solid = 0;
        (void)ne_site_facts(engine, state, frame->sites[center_up].observed.site, capture, 0, &center_up_solid, 0, 0);
        const uint8_t horizontal[4] = {4u, 5u, 6u, 7u};
        const uint8_t below[4] = {8u, 10u, 12u, 14u};
        const uint8_t above[4] = {9u, 11u, 13u, 15u};
        for (uint32_t i = 0; i < 4u; ++i) {
            uint32_t horizontal_slot = neighbors[order[horizontal[i]]];
            int solid = 0, transparent = 0;
            (void)ne_site_facts(engine, state, frame->sites[horizontal_slot].observed.site, capture,
                                0, &solid, &transparent, 0);
            uint8_t power = ne_walk_wire_power(frame, horizontal_slot);
            if (power > diagonal_power) diagonal_power = power;
            uint32_t vertical_slot;
            if (!solid) {
                vertical_slot = neighbors[order[below[i]]];
                power = ne_walk_wire_power(frame, vertical_slot);
                if (power > diagonal_power) diagonal_power = power;
            } else if (!center_up_solid && !transparent) {
                vertical_slot = neighbors[order[above[i]]];
                power = ne_walk_wire_power(frame, vertical_slot);
                if (power > diagonal_power) diagonal_power = power;
            }
        }
    }
    uint8_t attenuated = diagonal_power > 0 ? (uint8_t)(diagonal_power - 1u) : 0u;
    return direct_power > attenuated ? direct_power : attenuated;
}

static NEStatus ne_sample_piston(NEEngine *engine, NEStateView state, uint32_t piston_id);

static const NEDelivery *ne_turbo_delivery_for_observed(NEEngine *engine, NEStateView state, uint32_t wire_index,
                                                        uint32_t slot, const NEObserved *observed) {
    if (observed == 0) return 0;
    NEObserved snapshot = *observed;
    NECaptureSet capture = {0};
    capture.items = &snapshot;
    capture.count = 1u;
    capture.capacity = 1u;
    capture.item_stride = sizeof(NEObserved);
    return ne_turbo_delivery(engine, state, wire_index, slot, snapshot.site, &capture);
}

static NEStatus ne_dispatch_turbo_nonwire(NEEngine *engine, NEStateView state, const NEObserved *observed) {
    if (observed->receiver_kind == NE_RECEIVER_NONE) return NE_OK;
    engine->counters.callbacks++;
    if (observed->receiver_kind == NE_RECEIVER_COMPONENT) {
        if (observed->receiver_id >= engine->program->node_count) return NE_ERR_INVALID;
        if (engine->program->nodes[observed->receiver_id].kind == NE_NODE_WIRE) return NE_ERR_BAD_STATE;
        return ne_update_component(engine, state, observed->receiver_id, NE_DELIVERY_UPDATE, NE_FACE_NONE, observed);
    }
    if (observed->receiver_kind == NE_RECEIVER_PISTON_BASE || observed->receiver_kind == NE_RECEIVER_PISTON_HEAD)
        return ne_sample_piston(engine, state, observed->receiver_id);
    return observed->receiver_kind == NE_RECEIVER_CONTACT ? NE_OK : NE_ERR_INVALID;
}

static NEStatus ne_evaluate_isolated_turbo_wire(NEEngine *engine, NEStateView state, uint32_t wire_index) {
    const NEProgram *program = engine->program;
    if (wire_index >= program->wire_count) return NE_ERR_INVALID;
    const NEWire *wire = &program->wires[wire_index];
    if (wire->turbo_route >= program->route_count) return NE_OK;
    NEObserved observed[24];
    uint8_t selected[24];
    for (uint32_t slot = 0; slot < 24u; ++slot) {
        if (!ne_resolve_site(engine, state, wire->neighbors[slot], 0, &observed[slot])) return NE_ERR_INVALID;
    }
    /* Select the whole layer schedule before callbacks can mutate guards or state. */
    for (uint32_t slot = 0; slot < 24u; ++slot)
        selected[slot] = (uint8_t)(ne_turbo_delivery_for_observed(engine, state, wire_index, slot, &observed[slot]) != 0);
    engine->counters.wire_isolated_replays++;
    const uint8_t *order = NE_TURBO_ORDER[3]; /* fresh walk heading */
    for (uint32_t pass = 0; pass < 2u; ++pass) {
        uint32_t count = pass == 0u ? 24u : 4u;
        for (uint32_t i = 0; i < count; ++i) {
            uint32_t slot = order[i];
            if (selected[slot]) {
                NEStatus status = ne_dispatch_turbo_nonwire(engine, state, &observed[slot]);
                if (status != NE_OK) return status;
            }
        }
    }
    return NE_OK;
}

static NEStatus ne_evaluate_instant_reset_wire(NEEngine *engine, NEStateView state, uint32_t wire_index) {
    const NEProgram *program = engine->program;
    if (wire_index >= program->wire_count) return NE_ERR_INVALID;
    const NEWire *wire = &program->wires[wire_index];
    uint32_t partner_index = wire->instant_partner;
    if (partner_index >= program->wire_count) return NE_ERR_INVALID;
    const NEWire *partner = &program->wires[partner_index];
    if (wire->turbo_route >= program->route_count || partner->turbo_route >= program->route_count ||
        wire->node >= program->node_count || partner->node >= program->node_count) return NE_ERR_INVALID;
    uint32_t source_site = wire->neighbors[4];
    uint32_t head_site = wire->neighbors[22];
    uint32_t repeater_site = wire->neighbors[23];
    uint32_t tail_site = partner->neighbors[23];
    NEObserved source, head, repeater, tail;
    if (!ne_resolve_site(engine, state, source_site, 0, &source) ||
        !ne_resolve_site(engine, state, head_site, 0, &head) ||
        !ne_resolve_site(engine, state, repeater_site, 0, &repeater) ||
        !ne_resolve_site(engine, state, tail_site, 0, &tail)) return NE_ERR_INVALID;

    /* Mirror the graph plan's original target/guard scan before dispatch begins. */
    int root_repeater = ne_turbo_delivery_for_observed(engine, state, wire_index, 23u, &repeater) != 0;
    int root_head = ne_turbo_delivery_for_observed(engine, state, wire_index, 22u, &head) != 0;
    int partner_repeater = ne_turbo_delivery_for_observed(engine, state, partner_index, 5u, &repeater) != 0;
    int partner_tail = ne_turbo_delivery_for_observed(engine, state, partner_index, 23u, &tail) != 0;
    uint8_t source_power = ne_power_no_dust(engine, state, source_site, 2u, &((NECaptureSet){.items = &source, .count = 1u,
        .capacity = 1u, .item_stride = sizeof(NEObserved)}), 0u);
    uint8_t root_power = state.components[wire->node].wire_power;
    uint8_t partner_power = state.components[partner->node].wire_power;
    engine->counters.wire_instant_reset_replays++;
    for (;;) {
        uint8_t next_partner_power = root_power > 0u ? (uint8_t)(root_power - 1u) : 0u;
        int partner_changed = next_partner_power != partner_power;
        if (partner_changed) {
            const NEComponentState *current = &state.components[partner->node];
            (void)ne_set_wire_state(engine, state, partner->node, current->wire_north, current->wire_south,
                                    current->wire_east, current->wire_west, next_partner_power);
            partner_power = next_partner_power;
        }
        if (root_repeater) {
            NEStatus status = ne_dispatch_turbo_nonwire(engine, state, &repeater);
            if (status != NE_OK) return status;
        }
        if (root_head) {
            NEStatus status = ne_dispatch_turbo_nonwire(engine, state, &head);
            if (status != NE_OK) return status;
        }
        if (!partner_changed) return NE_OK;

        if (partner_repeater) {
            NEStatus status = ne_dispatch_turbo_nonwire(engine, state, &repeater);
            if (status != NE_OK) return status;
        }
        uint8_t next_root_power = source_power;
        uint8_t attenuated_partner = partner_power > 0u ? (uint8_t)(partner_power - 1u) : 0u;
        if (attenuated_partner > next_root_power) next_root_power = attenuated_partner;
        int root_changed = next_root_power != root_power;
        if (root_changed) {
            const NEComponentState *current = &state.components[wire->node];
            (void)ne_set_wire_state(engine, state, wire->node, current->wire_north, current->wire_south,
                                    current->wire_east, current->wire_west, next_root_power);
            root_power = next_root_power;
        }
        if (partner_tail) {
            NEStatus status = ne_dispatch_turbo_nonwire(engine, state, &tail);
            if (status != NE_OK) return status;
        }
        if (!root_changed) return NE_OK;
    }
}

static NEStatus ne_evaluate_turbo_wire_node(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                            NECaptureSet *capture, uint32_t node) {
    if (node >= engine->program->node_count) return NE_ERR_INVALID;
    uint32_t wire_index = engine->program->nodes[node].kind_data[0];
    if (wire_index >= engine->program->wire_count) return NE_ERR_INVALID;
    uint32_t root_slot, neighbors[24];
    uint8_t heading;
    if (!ne_walk_identify_neighbors(engine, state, frame, capture, node, &root_slot, neighbors, &heading)) return NE_ERR_ALLOC;
    frame->sites[root_slot].visited = 1;
    uint8_t next_power = ne_turbo_wire_power(engine, state, frame, capture, wire_index, neighbors, heading);
    NEObserved *observed = &frame->sites[root_slot].observed;
    if (observed->wire_power == next_power) return NE_OK;
    uint8_t north = observed->wire_north, south = observed->wire_south;
    uint8_t east = observed->wire_east, west = observed->wire_west;
    /* The graph backend propagates every captured-power change, even when the
     * current live wire state already matches the newly computed state. */
    (void)ne_set_wire_state(engine, state, node, north, south, east, west, next_power);
    observed->wire_power = next_power;
    return ne_propagate_wire_changes(engine, state, frame, capture, node, wire_index, frame->layer);
}

static NEStatus ne_dispatch_turbo_site(NEEngine *engine, NEStateView state, NEWalkFrame *frame,
                                       NECaptureSet *capture, uint32_t walk_slot) {
    NEObserved *observed = &frame->sites[walk_slot].observed;
    if (observed->receiver_kind == NE_RECEIVER_NONE) return NE_OK;
    engine->counters.callbacks++;
    if (observed->receiver_kind == NE_RECEIVER_COMPONENT) {
        uint32_t node = observed->receiver_id;
        if (node >= engine->program->node_count) return NE_ERR_INVALID;
        if (engine->program->nodes[node].kind == NE_NODE_WIRE)
            return ne_evaluate_turbo_wire_node(engine, state, frame, capture, node);
        return ne_update_component(engine, state, node, NE_DELIVERY_UPDATE, NE_FACE_NONE, observed);
    }
    if (observed->receiver_kind == NE_RECEIVER_PISTON_BASE || observed->receiver_kind == NE_RECEIVER_PISTON_HEAD)
        return ne_sample_piston(engine, state, observed->receiver_id);
    return observed->receiver_kind == NE_RECEIVER_CONTACT ? NE_OK : NE_ERR_INVALID;
}

static NEStatus ne_run_wire_hops(NEEngine *engine, NEStateView state, uint32_t node, uint32_t wire_index) {
    NEWalkFrame *frame = ne_walk_acquire(engine);
    if (frame == 0) return NE_ERR_ALLOC;
    NECaptureSet capture = ne_walk_capture_set(engine, frame);
    const NEProgram *program = engine->program;
    if (node >= program->node_count || wire_index >= program->wire_count || program->component_sites == 0 ||
        program->component_sites[node] >= program->site_count ||
        !ne_walk_capture_site(engine, state, frame, &capture, program->component_sites[node])) {
        ne_walk_release(engine);
        return NE_ERR_INVALID;
    }
    uint32_t root_slot = ne_walk_slot(frame, program->component_sites[node]);
    frame->sites[root_slot].visited = 1;
    NEStatus status = ne_propagate_wire_changes(engine, state, frame, &capture, node, wire_index, 0u);
    if (status == NE_OK) {
        ne_walk_rotate_queues(frame);
        frame->layer = 1u;
        while (frame->queue_count[0] != 0 || frame->queue_count[1] != 0) {
            uint32_t count = frame->queue_count[0];
            for (uint32_t i = 0; i < count; ++i) {
                uint32_t walk_slot = frame->queues[0][i];
                status = ne_dispatch_turbo_site(engine, state, frame, &capture, walk_slot);
                if (status != NE_OK) break;
            }
            if (status != NE_OK) break;
            frame->queue_count[0] = 0;
            ne_walk_rotate_queues(frame);
            frame->layer++;
        }
    }
    ne_walk_release(engine);
    return status;
}

uint32_t ne_copy_dirty(NEEngine *engine, uint8_t *destination, uint32_t capacity) {
    if (engine == 0 || (engine->program->node_count != 0 && destination == 0) || capacity < engine->program->node_count)
        return UINT32_MAX;
    const NEProgram *program = engine->program;
    for (uint32_t net_id = 0; net_id < program->classic_net_count; ++net_id) {
        if (engine->dirty_classic_nets == 0 || engine->dirty_classic_nets[net_id] == 0) continue;
        const NEClassicNet *net = &program->classic_nets[net_id];
        if ((uint64_t)net->first_wire + net->wire_count > program->classic_wire_count) return UINT32_MAX;
        for (uint32_t i = 0; i < net->wire_count; ++i) {
            uint32_t node = program->classic_wires[net->first_wire + i].node;
            if (node >= program->node_count) return UINT32_MAX;
            engine->dirty[node] = 1;
        }
        engine->dirty_classic_nets[net_id] = 0;
    }
    uint32_t count = 0;
    for (uint32_t i = 0; i < engine->program->node_count; ++i) {
        destination[i] = engine->dirty[i];
        if (engine->dirty[i] != 0) {
            engine->dirty[i] = 0;
            count++;
        }
    }
    return count;
}

uint32_t ne_copy_effects(NEEngine *engine, NEEffect *destination, uint32_t capacity) {
    if (engine == 0) return UINT32_MAX;
    if (engine->effects.count > capacity || (engine->effects.count != 0 && destination == 0)) return UINT32_MAX;
    for (uint32_t i = 0; i < engine->effects.count; ++i)
        destination[i] = engine->effects.items[engine->effects.head + i];
    uint32_t count = engine->effects.count;
    engine->effects.count = 0;
    engine->effects.head = 0;
    return count;
}

uint32_t ne_pending_effect_count(const NEEngine *engine) {
    return engine == 0 ? UINT32_MAX : engine->effects.count;
}

static int ne_push_sound(NEEngine *engine, NEPosition position, uint32_t sound_id, float volume, float pitch) {
    NEEffect effect;
    ne_bytes_zero(&effect, sizeof(effect));
    effect.kind = NE_EFFECT_SOUND;
    effect.position = position;
    effect.sound_id = sound_id;
    effect.sound_category = 4u;
    effect.volume = volume;
    effect.pitch = pitch;
    return ne_effect_push(engine, effect);
}

static uint8_t ne_comparator_input(NEEngine *engine, NEStateView state, const NEComparator *plan) {
    uint8_t rear;
    if (!plan->has_direct_override)
        rear = ne_input_strength(engine, state, plan->node, 0u);
    else if (plan->direct_override_live)
        rear = plan->direct_override_node < engine->program->node_count
            ? state.components[plan->direct_override_node].output_strength : 0u;
    else
        rear = plan->direct_override_strength;
    if (plan->has_direct_override) return rear;
    if (rear >= plan->far_threshold || !plan->has_far_override) return rear;
    if (!ne_guard_matches(engine, state, plan->far_override_guard, 0, 0)) return rear;
    return plan->far_override_live
        ? (plan->far_override_node < engine->program->node_count ? state.components[plan->far_override_node].output_strength : 0u)
        : plan->far_override_strength;
}

static uint8_t ne_comparator_output(NEEngine *engine, NEStateView state, const NEComparator *plan) {
    uint8_t input = ne_comparator_input(engine, state, plan);
    uint8_t side = ne_input_strength(engine, state, plan->node, 1u);
    if (plan->mode == 0u) return input >= side ? input : 0u;
    return input > side ? (uint8_t)(input - side) : 0u;
}

static uint8_t ne_comparator_powered(NEEngine *engine, NEStateView state, const NEComparator *plan) {
    uint8_t input = ne_comparator_input(engine, state, plan);
    uint8_t side = ne_input_strength(engine, state, plan->node, 1u);
    if (input == 0) return 0;
    if (input > side) return 1;
    return input == side && plan->mode == 0u;
}

static NEStatus ne_sample_piston(NEEngine *engine, NEStateView state, uint32_t piston);
static NEStatus ne_evaluate_seed(NEEngine *engine, NEStateView state, uint32_t wire_node, const NEObserved *captured);
static NEStatus ne_update_wire_shape(NEEngine *engine, NEStateView state, uint32_t wire_node, uint8_t direction,
                                     const NEObserved *captured);

static NEStatus ne_update_observer(NEEngine *engine, NEStateView state, uint32_t node, uint8_t direction,
                                   const NEObserved *captured) {
    const NENode *spec = &engine->program->nodes[node];
    uint8_t facing = (uint8_t)spec->kind_data[0];
    uint8_t powered = captured != 0 && (captured->flags & NE_OBS_ELECTRICAL) != 0
        ? captured->powered : state.components[node].powered;
    int qualifies = direction != NE_FACE_NONE ? facing == direction : powered != 0;
    if (qualifies && !ne_node_pending(engine, node)) return ne_schedule_game_ticks(engine, node, 1u, 3u) ? NE_OK : NE_ERR_ALLOC;
    return NE_OK;
}

static NEStatus ne_update_reset(NEEngine *engine, NEStateView state, uint32_t node, uint8_t direction,
                                const NEObserved *captured) {
    const NENode *spec = &engine->program->nodes[node];
    uint8_t facing = (uint8_t)spec->kind_data[0];
    uint8_t powered = captured != 0 && (captured->flags & NE_OBS_ELECTRICAL) != 0
        ? captured->powered : state.components[node].powered;
    int qualifies = direction != NE_FACE_NONE ? direction == facing : powered != 0;
    if (qualifies) return ne_schedule_game_ticks(engine, node, 1u, 3u) ? NE_OK : NE_ERR_ALLOC;
    return NE_OK;
}

static NEStatus ne_update_component(NEEngine *engine, NEStateView state, uint32_t node,
                                    uint8_t delivery_kind, uint8_t direction, const NEObserved *captured) {
    const NEProgram *program = engine->program;
    if (node >= program->node_count) return NE_ERR_INVALID;
    const NENode *spec = &program->nodes[node];
    if (delivery_kind == NE_DELIVERY_SHAPE)
        return spec->kind == NE_NODE_WIRE ? ne_update_wire_shape(engine, state, node, direction, captured) : NE_OK;
    switch (spec->kind) {
        case NE_NODE_OBSERVER: return ne_update_observer(engine, state, node, direction, captured);
        case NE_NODE_BUILTIN_RESET: return ne_update_reset(engine, state, node, direction, captured);
        case NE_NODE_WIRE: return ne_evaluate_seed(engine, state, node, captured);
        case NE_NODE_PISTON: return ne_sample_piston(engine, state, spec->kind_data[0]);
        case NE_NODE_CONTACT: return NE_OK;
        case NE_NODE_ELECTRICAL: break;
        default: return NE_ERR_UNSUPPORTED;
    }
    NEComponentState *current = &state.components[node];
    uint8_t captured_powered = captured != 0 && (captured->flags & NE_OBS_ELECTRICAL) != 0
        ? captured->powered : current->powered;
    uint8_t captured_locked = captured != 0 && (captured->flags & NE_OBS_ELECTRICAL) != 0
        ? captured->locked : current->repeater_locked;
    uint8_t main = ne_input_strength(engine, state, node, 0u);
    switch (spec->electrical_kind) {
        case NE_ELECTRICAL_TORCH:
            if ((captured_powered != 0) == (main != 0) && !ne_node_pending(engine, node))
                return ne_schedule_game_ticks(engine, node, 1u, 3u) ? NE_OK : NE_ERR_ALLOC;
            return NE_OK;
        case NE_ELECTRICAL_REPEATER: {
            uint8_t should_lock = ne_input_strength(engine, state, node, 1u) != 0;
            if (captured_locked != should_lock) {
                NEStatus status = ne_set_node(engine, state, node, captured_powered,
                                              (uint8_t)(captured_powered ? 15u : 0u), should_lock, 0);
                if (status != NE_OK) return status;
            }
            if (should_lock || ne_node_pending(engine, node)) return NE_OK;
            uint8_t should_power = main != 0;
            if (should_power == captured_powered) return NE_OK;
            uint32_t priority = (spec->flags & NE_NODE_FLAG_REPEATER_FACING_DIODE) != 0 ? 0u :
                                !should_power ? 1u : 2u;
            return ne_schedule_game_ticks(engine, node, spec->kind_data[0], priority) ? NE_OK : NE_ERR_ALLOC;
        }
        case NE_ELECTRICAL_COMPARATOR: {
            if (ne_node_pending(engine, node)) return NE_OK;
            uint32_t index = spec->kind_data[0];
            if (index >= program->comparator_count) return NE_ERR_INVALID;
            const NEComparator *plan = &program->comparators[index];
            uint8_t strength = ne_comparator_output(engine, state, plan);
            uint8_t powered = ne_comparator_powered(engine, state, plan);
            if (strength != current->output_strength || powered != current->powered) {
                uint32_t priority = (spec->flags & NE_NODE_FLAG_COMPARATOR_FACING_DIODE) != 0 ? 2u : 3u;
                return ne_schedule_game_ticks(engine, node, 1u, priority) ? NE_OK : NE_ERR_ALLOC;
            }
            return NE_OK;
        }
        case NE_ELECTRICAL_LAMP:
            if (captured_powered && main == 0) {
                if (!ne_node_pending(engine, node))
                    return ne_schedule_game_ticks(engine, node, 2u, 3u) ? NE_OK : NE_ERR_ALLOC;
            } else if (!captured_powered && main != 0) {
                return ne_set_node(engine, state, node, 1u, 0u, -1, 0);
            }
            return NE_OK;
        case NE_ELECTRICAL_HOPPER:
            if (captured_powered != (main != 0))
                return ne_set_node(engine, state, node, (uint8_t)(main != 0), current->output_strength, -1, 0);
            return NE_OK;
        case NE_ELECTRICAL_COPPER_BULB:
            if (current->powered != (main != 0)) {
                uint8_t lit = main != 0 ? current->output_strength == 0 : current->output_strength > 0;
                if (main != 0 && !ne_push_sound(engine, spec->position, lit ? program->sound_bulb_on : program->sound_bulb_off, 1.0f, 1.0f))
                    return NE_ERR_ALLOC;
                NEStatus status = ne_set_node(engine, state, node, (uint8_t)(main != 0), lit ? 15u : 0u, -1, 0);
                if (status != NE_OK) return status;
                if (spec->output_route != NE_NONE) return ne_deliver_route(engine, state, spec->output_route);
            }
            return NE_OK;
        case NE_ELECTRICAL_COMMAND_BLOCK:
        case NE_ELECTRICAL_NOTE_BLOCK:
        case NE_ELECTRICAL_TRAPDOOR:
            return NE_ERR_UNSUPPORTED;
        case NE_ELECTRICAL_BUTTON:
        case NE_ELECTRICAL_LEVER:
        case NE_ELECTRICAL_PRESSURE_PLATE:
        case NE_ELECTRICAL_CONSTANT:
            return NE_OK;
        default: return NE_ERR_UNSUPPORTED;
    }
}

static NEStatus ne_dispatch_delivery(NEEngine *engine, NEStateView state, const NEDelivery *delivery,
                                     const NECaptureSet *capture) {
    const NEProgram *program = engine->program;
    if (!ne_guard_matches(engine, state, delivery->guard, capture, 0)) return NE_OK;
    const NEObserved *observed = delivery->view == 1u ? ne_capture_find(capture, delivery->target_site) : 0;
    if (delivery->view == 1u && observed == 0) return NE_OK;
    uint8_t receiver_kind = delivery->receiver_kind;
    uint32_t receiver_id = delivery->receiver_id;
    if (receiver_kind == NE_RECEIVER_NONE && delivery->kind == NE_DELIVERY_UPDATE && delivery->target_site < program->site_count) {
        const NESite *site = &program->sites[delivery->target_site];
        if (site->source_kind == NE_SOURCE_COMPONENT) {
            receiver_kind = NE_RECEIVER_COMPONENT;
            receiver_id = site->source_id;
        }
    }
    if (receiver_kind == NE_RECEIVER_NONE) return NE_OK;
    engine->counters.callbacks++;
    if (receiver_kind == NE_RECEIVER_COMPONENT)
        return ne_update_component(engine, state, receiver_id, delivery->kind, delivery->direction, observed);
    if (receiver_kind == NE_RECEIVER_PISTON_BASE || receiver_kind == NE_RECEIVER_PISTON_HEAD)
        return ne_sample_piston(engine, state, receiver_id);
    return NE_OK;
}

static NEStatus ne_classic_schedule(NEEngine *engine, NEStateView state, uint32_t node, uint8_t nonzero) {
    const NEProgram *program = engine->program;
    if (node >= program->node_count || ne_node_pending(engine, node)) return NE_OK;
    const NENode *spec = &program->nodes[node];
    const NEComponentState *current = &state.components[node];
    uint32_t delay = 0;
    uint32_t priority = 3u;
    if (spec->kind == NE_NODE_ELECTRICAL && spec->electrical_kind == NE_ELECTRICAL_TORCH) {
        if (current->powered != (nonzero != 0)) return NE_OK;
        delay = 1u;
        priority = 3u; /* Normal */
    } else if (spec->kind == NE_NODE_ELECTRICAL && spec->electrical_kind == NE_ELECTRICAL_REPEATER) {
        if (current->powered == (nonzero != 0)) return NE_OK;
        delay = spec->kind_data[0];
        priority = (spec->flags & NE_NODE_FLAG_REPEATER_FACING_DIODE) != 0 ? 0u :
                   (nonzero == 0 ? 1u : 2u); /* Highest, Higher, High */
    } else {
        return NE_OK;
    }
    return ne_schedule_game_ticks(engine, node, delay, priority) ? NE_OK : NE_ERR_ALLOC;
}

static NEStatus ne_run_classic_entry(NEEngine *engine, NEStateView state, uint32_t entry_id) {
    const NEProgram *program = engine->program;
    if (entry_id >= program->classic_entry_count) return NE_ERR_INVALID;
    const NEClassicEntry *entry = &program->classic_entries[entry_id];
    if (!entry->commit) return NE_OK;
    if (entry->net >= program->classic_net_count || state.classic_signals == 0) return NE_ERR_INVALID;
    const NEClassicNet *net = &program->classic_nets[entry->net];
    if (net->source >= program->node_count || (uint64_t)entry->first_op + entry->op_count > program->classic_op_count)
        return NE_ERR_INVALID;
    uint8_t old_signal = state.classic_signals[entry->net];
    uint8_t new_signal = state.components[net->source].output_strength;
    uint32_t pair = ((uint32_t)old_signal << 4) | new_signal;
    state.classic_signals[entry->net] = new_signal;
    engine->counters.classic_net_invocations++;
    for (uint32_t i = 0; i < entry->op_count; ++i) {
        const NEClassicOp *op = &program->classic_ops[entry->first_op + i];
        if (op->receiver >= program->node_count) return NE_ERR_INVALID;
        if ((op->when[pair >> 6] & (UINT64_C(1) << (pair & 63u))) == 0) continue;
        engine->counters.classic_receiver_ops++;
        NEStatus status = ne_classic_schedule(engine, state, op->receiver, op->nonzero);
        if (status != NE_OK) return status;
    }
    if (old_signal != new_signal) {
        if (engine->dirty_classic_nets == 0) return NE_ERR_INVALID;
        engine->dirty_classic_nets[entry->net] = 1;
    }
    return NE_OK;
}

static NEStatus ne_deliver_route_captured(NEEngine *engine, NEStateView state, uint32_t route_id, const NECaptureSet *capture) {
    const NEProgram *program = engine->program;
    if (route_id >= program->route_count) return NE_ERR_INVALID;
    const NERoute *route = &program->routes[route_id];
    uint64_t delivery_end = (uint64_t)route->first_delivery + route->delivery_count;
    if (delivery_end > program->delivery_count) return NE_ERR_INVALID;
    if (route->first_instant_step == NE_NONE) {
        uint32_t classic_start = 0, classic_end = 0, classic_cursor = 0;
        if (program->classic_route_ranges != 0) {
            const NERange *range = &program->classic_route_ranges[route_id];
            if ((uint64_t)range->start + range->count > program->classic_route_entry_count ||
                (range->count != 0 && program->classic_route_entries == 0)) return NE_ERR_INVALID;
            classic_start = range->start;
            classic_end = range->start + range->count;
            classic_cursor = classic_start;
        }
        for (uint32_t i = 0; i < route->delivery_count; ++i) {
            if (classic_cursor < classic_end) {
                const NEClassicRouteEntry *binding = &program->classic_route_entries[classic_cursor];
                if (binding->ordinal < i) return NE_ERR_INVALID;
                if (binding->ordinal == i) {
                    NEStatus status = ne_run_classic_entry(engine, state, binding->entry);
                    if (status != NE_OK) return status;
                    classic_cursor++;
                    continue;
                }
            }
            NEStatus status = ne_dispatch_delivery(engine, state, &program->deliveries[route->first_delivery + i], capture);
            if (status != NE_OK) return status;
        }
        if (classic_cursor != classic_end) return NE_ERR_INVALID;
        return NE_OK;
    }
    uint64_t instant_end = (uint64_t)route->first_instant_step + route->instant_step_count;
    if (instant_end > program->instant_step_count || (route->instant_step_count != 0u && program->instant_steps == 0))
        return NE_ERR_INVALID;
    engine->counters.instant_route_invocations++;
    for (uint32_t i = 0; i < route->instant_step_count; ++i) {
        const NEInstantStep *step = &program->instant_steps[route->first_instant_step + i];
        if (step->delivery < route->first_delivery || step->delivery >= delivery_end) return NE_ERR_INVALID;
        engine->counters.instant_route_steps++;
        const NEDelivery *delivery = &program->deliveries[step->delivery];
        NEStatus status = NE_OK;
        switch (step->kind) {
            case NE_INSTANT_DISPATCH:
                status = ne_dispatch_delivery(engine, state, delivery, capture);
                break;
            case NE_INSTANT_SAMPLE:
                if (ne_guard_matches(engine, state, delivery->guard, capture, 0))
                    status = ne_sample_piston(engine, state, step->operand);
                break;
            case NE_INSTANT_ARM_RESET:
                /* The validated certificate is Always + Live + qualifying face. */
                if (step->operand >= program->node_count) return NE_ERR_INVALID;
                if (!ne_schedule_game_ticks(engine, step->operand, 1u, 3u)) return NE_ERR_ALLOC;
                break;
            default:
                return NE_ERR_INVALID;
        }
        if (status != NE_OK) return status;
    }
    return NE_OK;
}

static NEStatus ne_deliver_route(NEEngine *engine, NEStateView state, uint32_t route) {
    return ne_deliver_route_captured(engine, state, route, 0);
}

static NEStatus ne_tick_node(NEEngine *engine, NEStateView state, uint32_t node) {
    const NEProgram *program = engine->program;
    if (node >= program->node_count) return NE_ERR_INVALID;
    const NENode *spec = &program->nodes[node];
    if (spec->kind == NE_NODE_WIRE || spec->kind == NE_NODE_CONTACT) return NE_OK;
    if (spec->kind == NE_NODE_PISTON) return ne_sample_piston(engine, state, spec->kind_data[0]);
    if (spec->kind == NE_NODE_OBSERVER || spec->kind == NE_NODE_BUILTIN_RESET) {
        uint8_t powered = state.components[node].powered;
        if (!powered && !ne_schedule_game_ticks(engine, node, 1u, 3u)) return NE_ERR_ALLOC;
        if (spec->kind == NE_NODE_OBSERVER)
            return ne_set_node(engine, state, node, (uint8_t)!powered, (uint8_t)(!powered ? 15u : 0u), -1, 1);
        return ne_set_node(engine, state, node, (uint8_t)!powered, (uint8_t)(!powered ? 15u : 0u), -1, 0);
    }
    if (spec->kind != NE_NODE_ELECTRICAL) return NE_ERR_UNSUPPORTED;
    NEComponentState *current = &state.components[node];
    switch (spec->electrical_kind) {
        case NE_ELECTRICAL_TORCH: {
            uint8_t should_be_off = ne_input_strength(engine, state, node, 0u) != 0;
            if (current->powered == should_be_off)
                return ne_set_node(engine, state, node, (uint8_t)!current->powered,
                                   (uint8_t)(!current->powered ? 15u : 0u), -1, 1);
            return NE_OK;
        }
        case NE_ELECTRICAL_REPEATER: {
            if (current->repeater_locked) return NE_OK;
            uint8_t should_power = ne_input_strength(engine, state, node, 0u) != 0;
            if (current->powered && !should_power)
                return ne_set_node(engine, state, node, 0u, 0u, -1, 1);
            if (!current->powered) {
                if (!should_power && !ne_schedule_game_ticks(engine, node, spec->kind_data[0], 1u)) return NE_ERR_ALLOC;
                return ne_set_node(engine, state, node, 1u, 15u, -1, 1);
            }
            return NE_OK;
        }
        case NE_ELECTRICAL_COMPARATOR: {
            uint32_t index = spec->kind_data[0];
            if (index >= program->comparator_count) return NE_ERR_INVALID;
            const NEComparator *plan = &program->comparators[index];
            uint8_t strength = ne_comparator_output(engine, state, plan);
            if (strength != current->output_strength || plan->mode == 0u) {
                uint8_t powered = ne_comparator_powered(engine, state, plan);
                NEStatus status = ne_set_node(engine, state, node, powered, strength, -1, 0);
                if (status != NE_OK) return status;
                if (spec->output_route != NE_NONE) return ne_deliver_route(engine, state, spec->output_route);
            }
            return NE_OK;
        }
        case NE_ELECTRICAL_LAMP:
            if (current->powered && ne_input_strength(engine, state, node, 0u) == 0)
                return ne_set_node(engine, state, node, 0u, 0u, -1, 0);
            return NE_OK;
        case NE_ELECTRICAL_BUTTON:
            if (current->powered) {
                if (!ne_push_sound(engine, spec->position, program->sound_button_off, 1.0f, 1.0f)) return NE_ERR_ALLOC;
                return ne_set_node(engine, state, node, 0u, 0u, -1, 1);
            }
            return NE_OK;
        case NE_ELECTRICAL_COMMAND_BLOCK:
        case NE_ELECTRICAL_NOTE_BLOCK:
        case NE_ELECTRICAL_TRAPDOOR:
            return NE_ERR_UNSUPPORTED;
        case NE_ELECTRICAL_COPPER_BULB:
        case NE_ELECTRICAL_HOPPER:
        case NE_ELECTRICAL_LEVER:
        case NE_ELECTRICAL_PRESSURE_PLATE:
        case NE_ELECTRICAL_CONSTANT:
            return NE_OK;
        default: return NE_ERR_UNSUPPORTED;
    }
}

static int ne_piston_base_pending(uint8_t availability) {
    return availability == NE_PISTON_BASE_PENDING_HEAD_PRESENT || availability == NE_PISTON_BASE_PENDING;
}

static uint32_t ne_resource_at_contact(NEEngine *engine, NEStateView state, uint32_t contact) {
    const NEProgram *program = engine->program;
    if (contact >= program->contact_count || program->resources_at_contact_offsets == 0) return NE_NONE;
    uint32_t start = program->resources_at_contact_offsets[contact];
    uint32_t end = program->resources_at_contact_offsets[contact + 1u];
    if (start > end || end > program->resources_at_contact_count) return NE_NONE;
    for (uint32_t i = start; i < end; ++i) {
        uint32_t resource = program->resources_at_contact[i];
        if (resource < program->resource_count && state.resources[resource].at == contact) return resource;
    }
    return NE_NONE;
}

static int ne_resource_has_contact(NEEngine *engine, uint32_t resource, uint32_t contact) {
    const NEProgram *program = engine->program;
    if (resource >= program->resource_count) return 0;
    const NEResource *spec = &program->resources[resource];
    if ((uint64_t)spec->first_contact + spec->contact_count > program->resource_contact_count) return 0;
    for (uint32_t i = 0; i < spec->contact_count; ++i)
        if (program->resource_contacts[spec->first_contact + i] == contact) return 1;
    return 0;
}

static int ne_contact_is_air(NEEngine *engine, NEStateView state, uint32_t contact) {
    const NEContactVariant *variant = ne_selected_contact_variant(engine, state, contact);
    if (variant == 0) return 0;
    const NEProfile *profile = ne_profile(engine->program, variant->profile);
    return profile != 0 && (profile->flags & NE_PROFILE_FLAG_AIR) != 0;
}

static int ne_resource_contact_free(NEEngine *engine, NEStateView state, uint32_t resource,
                                    uint32_t contact, uint32_t vacated_head) {
    if (resource >= engine->program->resource_count) return 0;
    const NEResourceState *resource_state = &state.resources[resource];
    if (resource_state->at == contact && resource_state->enabled) return 1;
    if (vacated_head < engine->program->piston_count && engine->program->pistons[vacated_head].near_contact == contact)
        return 1;
    return ne_contact_is_air(engine, state, contact);
}

static int ne_resource_contact_available(NEEngine *engine, NEStateView state, uint32_t resource,
                                         uint32_t contact, uint32_t vacated_head) {
    const NEProgram *program = engine->program;
    if (resource >= program->resource_count || contact >= program->contact_count ||
        !ne_resource_has_contact(engine, resource, contact)) return 0;
    if (program->resources_at_contact_offsets != 0) {
        uint32_t start = program->resources_at_contact_offsets[contact];
        uint32_t end = program->resources_at_contact_offsets[contact + 1u];
        if (start > end || end > program->resources_at_contact_count) return 0;
        for (uint32_t i = start; i < end; ++i) {
            uint32_t other = program->resources_at_contact[i];
            if (other != resource && other < program->resource_count && state.resources[other].at == contact) return 0;
        }
    }
    if (program->base_pistons_at_contact_offsets != 0) {
        uint32_t start = program->base_pistons_at_contact_offsets[contact];
        uint32_t end = program->base_pistons_at_contact_offsets[contact + 1u];
        if (start > end || end > program->base_pistons_at_contact_count) return 0;
        for (uint32_t i = start; i < end; ++i) {
            uint32_t piston = program->base_pistons_at_contact[i];
            if (piston >= program->piston_count || !ne_piston_base_pending(state.pistons[piston].availability)) return 0;
        }
    }
    if (program->head_pistons_at_contact_offsets != 0) {
        uint32_t start = program->head_pistons_at_contact_offsets[contact];
        uint32_t end = program->head_pistons_at_contact_offsets[contact + 1u];
        if (start > end || end > program->head_pistons_at_contact_count) return 0;
        for (uint32_t i = start; i < end; ++i) {
            uint32_t piston = program->head_pistons_at_contact[i];
            if (piston >= program->piston_count) return 0;
            const NEPistonState *head = &state.pistons[piston];
            int present = head->availability == NE_PISTON_READY || head->availability == NE_PISTON_HEAD_PENDING ||
                          head->availability == NE_PISTON_BASE_PENDING_HEAD_PRESENT;
            if (head->head_present && present && piston != vacated_head) return 0;
        }
    }
    return ne_resource_contact_free(engine, state, resource, contact, vacated_head);
}

static uint32_t ne_piston_resource_at(NEEngine *engine, NEStateView state, uint32_t piston, uint32_t contact) {
    (void)engine;
    const NEProgram *program = engine->program;
    if (piston >= program->piston_count || program->resource_for_piston == 0) return NE_NONE;
    uint32_t resource = program->resource_for_piston[piston];
    return resource < program->resource_count && state.resources[resource].at == contact ? resource : NE_NONE;
}

static int ne_pending_resource_extension(NEEngine *engine, NEStateView state, uint32_t resource,
                                         uint32_t contact, uint32_t facing) {
    if (resource >= engine->program->resource_count || state.resources[resource].enabled || state.resources[resource].at != contact)
        return 0;
    const NEDeferredQueue *queue = &engine->deferred;
    for (uint32_t i = 0; i < queue->count; ++i) {
        const NEFutureDeferredPiston *event = &queue->items[queue->head + i];
        if (!event->extending || !event->has_resource || event->piston >= engine->program->piston_count) continue;
        const NEPiston *piston = &engine->program->pistons[event->piston];
        if (piston->facing == facing && event->resource == resource && event->resource_revision == state.resources[resource].revision &&
            event->contact == contact) return 1;
    }
    return 0;
}

static int ne_piston_lane_available(NEEngine *engine, NEStateView state, uint32_t piston_id) {
    const NEProgram *program = engine->program;
    if (piston_id >= program->piston_count) return 0;
    const NEPiston *piston = &program->pistons[piston_id];
    const NEContactVariant *near_variant = ne_selected_contact_variant(engine, state, piston->near_contact);
    if (near_variant == 0) return 0;
    const NEProfile *near = ne_profile(program, near_variant->profile);
    if (near == 0 || (near->flags & (NE_PROFILE_FLAG_CONTAINER | NE_PROFILE_FLAG_MOVING_PISTON | NE_PROFILE_FLAG_PISTON_HEAD)) != 0)
        return 0;
    uint32_t resource = ne_piston_resource_at(engine, state, piston_id, piston->near_contact);
    if (resource == NE_NONE)
        return (near->flags & NE_PROFILE_FLAG_AIR) != 0 && ne_resource_at_contact(engine, state, piston->near_contact) == NE_NONE;
    if (piston->lane_kind != NE_LANE_SINGLE_RESOURCE || !state.resources[resource].enabled) return 0;
    uint32_t material = program->resources[resource].material_profile;
    if (near_variant->profile != material) return 0;
    return ne_resource_contact_available(engine, state, resource, piston->lane_far_contact, NE_NONE);
}

static int ne_piston_retraction_available(NEEngine *engine, NEStateView state, uint32_t piston_id, int without_pull) {
    const NEProgram *program = engine->program;
    if (piston_id >= program->piston_count) return 0;
    const NEPiston *piston = &program->pistons[piston_id];
    if (piston->lane_kind != NE_LANE_SINGLE_RESOURCE) return 1;
    uint32_t resource = ne_piston_resource_at(engine, state, piston_id, piston->lane_far_contact);
    if (resource == NE_NONE) return 1;
    int matching_extension = ne_pending_resource_extension(engine, state, resource, piston->lane_far_contact, piston->facing);
    if (!state.resources[resource].enabled)
        return !matching_extension || ne_resource_contact_available(engine, state, resource, piston->lane_far_contact, NE_NONE);
    return without_pull || ne_resource_contact_available(engine, state, resource, piston->near_contact, piston_id);
}

static int ne_enqueue_piston(NEEngine *engine, uint32_t piston, uint32_t action) {
    for (uint32_t i = 0; i < engine->requests.count; ++i) {
        const NEFuturePistonRequest *queued = &engine->requests.items[engine->requests.head + i];
        if (queued->piston == piston && queued->action == action) return 1;
    }
    NEFuturePistonRequest request = {piston, action};
    return ne_request_push(engine, request);
}

static NEStatus ne_sample_piston(NEEngine *engine, NEStateView state, uint32_t piston_id) {
    const NEProgram *program = engine->program;
    if (piston_id >= program->piston_count) return NE_ERR_INVALID;
    const NEPiston *piston = &program->pistons[piston_id];
    NEPistonState *piston_state = &state.pistons[piston_id];
    if (piston_state->availability == NE_PISTON_BASE_PENDING || piston_state->availability == NE_PISTON_BASE_PENDING_HEAD_PRESENT)
        return NE_OK;
    engine->counters.piston_samples++;
    uint8_t powered = ne_input_strength(engine, state, piston->node, 0u) != 0 || ne_input_strength(engine, state, piston->node, 2u) != 0;
    uint32_t action = NE_NONE;
    if (powered && !piston_state->extended && ne_piston_lane_available(engine, state, piston_id))
        action = NE_ACTION_EXTEND;
    else if (!powered && piston_state->extended) {
        int early_retract = 0;
        if (piston->lane_kind == NE_LANE_SINGLE_RESOURCE) {
            uint32_t resource = ne_piston_resource_at(engine, state, piston_id, piston->lane_far_contact);
            if (resource != NE_NONE && ne_pending_resource_extension(engine, state, resource, piston->lane_far_contact, piston->facing)) {
                for (uint32_t i = 0; i < engine->deferred.count; ++i) {
                    const NEFutureDeferredPiston *event = &engine->deferred.items[engine->deferred.head + i];
                    if (event->extending && event->has_resource && event->resource == resource &&
                        event->resource_revision == state.resources[resource].revision && event->contact == piston->lane_far_contact &&
                        event->piston < program->piston_count && program->pistons[event->piston].facing == piston->facing &&
                        (event->due_tick > engine->logical_tick || engine->phase == 1u || engine->phase == 2u)) {
                        early_retract = 1;
                        break;
                    }
                }
            }
        }
        action = early_retract ? NE_ACTION_RETRACT_WITHOUT_PULL : NE_ACTION_RETRACT;
    }
    if (action == NE_NONE) return NE_OK;
    return ne_enqueue_piston(engine, piston_id, action) ? NE_OK : NE_ERR_ALLOC;
}

static int ne_set_resource_contact(NEEngine *engine, NEStateView state, uint32_t resource, uint32_t contact,
                                   uint8_t enabled, uint64_t *revision) {
    if (resource >= engine->program->resource_count || contact >= engine->program->contact_count) return 0;
    NEResourceState *current = &state.resources[resource];
    if (current->at == contact && current->enabled == enabled) return 0;
    current->at = contact;
    current->enabled = enabled;
    current->revision++;
    if (revision != 0) *revision = current->revision;
    return 1;
}

static NEStatus ne_extend_connections(NEEngine *engine, NEStateView state, uint32_t piston_id) {
    const NEPiston *piston = &engine->program->pistons[piston_id];
    NEPistonState *piston_state = &state.pistons[piston_id];
    uint32_t resource_to_move = ne_piston_resource_at(engine, state, piston_id, piston->near_contact);
    piston_state->revision++;
    uint64_t revision = piston_state->revision;
    piston_state->availability = NE_PISTON_HEAD_PENDING;
    piston_state->head_present = 1;
    uint32_t moved_resource = NE_NONE;
    uint64_t resource_revision = 0;
    if (resource_to_move != NE_NONE && piston->lane_kind == NE_LANE_SINGLE_RESOURCE &&
        ne_set_resource_contact(engine, state, resource_to_move, piston->lane_far_contact, 0, &resource_revision))
        moved_resource = resource_to_move;
    NEFutureDeferredPiston deferred;
    ne_bytes_zero(&deferred, sizeof(deferred));
    deferred.piston = piston_id;
    deferred.revision = revision;
    deferred.due_tick = engine->logical_tick + 2u;
    deferred.extending = 1;
    deferred.has_resource = moved_resource != NE_NONE;
    deferred.resource = moved_resource;
    deferred.resource_revision = resource_revision;
    deferred.contact = moved_resource != NE_NONE ? piston->lane_far_contact : NE_NONE;
    if (!ne_deferred_push(engine, deferred)) return NE_ERR_ALLOC;
    if (moved_resource != NE_NONE && piston->routes[3] != NE_NONE) {
        NEStatus status = ne_deliver_route(engine, state, piston->routes[3]);
        if (status != NE_OK) return status;
    }
    if (piston->routes[1] != NE_NONE) {
        NEStatus status = ne_deliver_route(engine, state, piston->routes[1]);
        if (status != NE_OK) return status;
        if (moved_resource != NE_NONE) {
            status = ne_deliver_route(engine, state, piston->routes[1]);
            if (status != NE_OK) return status;
        }
    }
    piston_state->extended = 1;
    if (piston->routes[0] != NE_NONE) return ne_deliver_route(engine, state, piston->routes[0]);
    return NE_OK;
}

static NEStatus ne_retract_connections(NEEngine *engine, NEStateView state, uint32_t piston_id, int without_pull) {
    const NEPiston *piston = &engine->program->pistons[piston_id];
    NEPistonState *piston_state = &state.pistons[piston_id];
    uint32_t resource_at_far = piston->lane_kind == NE_LANE_SINGLE_RESOURCE
        ? ne_piston_resource_at(engine, state, piston_id, piston->lane_far_contact) : NE_NONE;
    int matching_extension = resource_at_far != NE_NONE &&
        ne_pending_resource_extension(engine, state, resource_at_far, piston->lane_far_contact, piston->facing);
    int resource_enabled = resource_at_far != NE_NONE && state.resources[resource_at_far].enabled;
    int restore_far = matching_extension;
    int pull_resource = !without_pull && resource_enabled;
    int interrupted_head = piston_state->availability == NE_PISTON_HEAD_PENDING;
    piston_state->revision++;
    uint64_t revision = piston_state->revision;
    piston_state->availability = interrupted_head ? NE_PISTON_PUBLISHING_HEAD_REMOVAL : NE_PISTON_BASE_PENDING_HEAD_PRESENT;
    if (interrupted_head) {
        if (piston->routes[1] != NE_NONE) {
            NEStatus status = ne_deliver_route(engine, state, piston->routes[1]);
            if (status != NE_OK) return status;
        }
        piston_state->head_present = 0;
        piston_state->availability = NE_PISTON_BASE_PENDING;
        if (piston->routes[0] != NE_NONE) {
            NEStatus status = ne_deliver_route(engine, state, piston->routes[0]);
            if (status != NE_OK) return status;
        }
    } else {
        if (piston->routes[0] != NE_NONE) {
            NEStatus status = ne_deliver_route(engine, state, piston->routes[0]);
            if (status != NE_OK) return status;
        }
        piston_state->availability = NE_PISTON_BASE_PENDING;
        piston_state->head_present = 0;
    }
    if (restore_far) {
        uint64_t restored_revision;
        if (ne_set_resource_contact(engine, state, resource_at_far, piston->lane_far_contact, 1, &restored_revision) &&
            piston->routes[7] != NE_NONE) {
            NEStatus status = ne_deliver_route(engine, state, piston->routes[7]);
            if (status != NE_OK) return status;
        }
    }
    uint32_t moved_resource = NE_NONE;
    uint64_t resource_revision = 0;
    if (pull_resource && resource_at_far != NE_NONE && piston->lane_kind == NE_LANE_SINGLE_RESOURCE &&
        ne_set_resource_contact(engine, state, resource_at_far, piston->near_contact, 0, &resource_revision))
        moved_resource = resource_at_far;
    if (!without_pull && moved_resource != NE_NONE && piston->routes[2] != NE_NONE) {
        NEStatus status = ne_deliver_route(engine, state, piston->routes[2]);
        if (status != NE_OK) return status;
    }
    if (piston->routes[1] != NE_NONE) {
        NEStatus status = ne_deliver_route(engine, state, piston->routes[1]);
        if (status != NE_OK) return status;
    }
    NEFutureDeferredPiston deferred;
    ne_bytes_zero(&deferred, sizeof(deferred));
    deferred.piston = piston_id;
    deferred.revision = revision;
    deferred.due_tick = engine->logical_tick + 2u;
    deferred.extending = 0;
    deferred.has_resource = moved_resource != NE_NONE;
    deferred.resource = moved_resource;
    deferred.resource_revision = resource_revision;
    deferred.contact = moved_resource != NE_NONE ? piston->near_contact : NE_NONE;
    return ne_deferred_push(engine, deferred) ? NE_OK : NE_ERR_ALLOC;
}

static NEStatus ne_accept_piston_action(NEEngine *engine, NEStateView state, NEFuturePistonRequest request) {
    const NEProgram *program = engine->program;
    if (request.piston >= program->piston_count) return NE_ERR_INVALID;
    const NEPiston *piston = &program->pistons[request.piston];
    NEPistonState prior = state.pistons[request.piston];
    uint8_t powered = ne_input_strength(engine, state, piston->node, 0u) != 0 || ne_input_strength(engine, state, piston->node, 2u) != 0;
    int accepted = 0;
    if (request.action == NE_ACTION_EXTEND)
        accepted = powered && !prior.extended && prior.availability == NE_PISTON_READY && ne_piston_lane_available(engine, state, request.piston);
    else if (request.action == NE_ACTION_RETRACT || request.action == NE_ACTION_RETRACT_WITHOUT_PULL)
        accepted = !powered && prior.extended && !ne_piston_base_pending(prior.availability) &&
                   ne_piston_retraction_available(engine, state, request.piston, request.action == NE_ACTION_RETRACT_WITHOUT_PULL);
    if (!accepted) return NE_OK;
    NEStatus status = request.action == NE_ACTION_EXTEND ? ne_extend_connections(engine, state, request.piston) :
                      ne_retract_connections(engine, state, request.piston, request.action == NE_ACTION_RETRACT_WITHOUT_PULL);
    if (status != NE_OK) return status;
    NEEffect effect;
    ne_bytes_zero(&effect, sizeof(effect));
    effect.kind = NE_EFFECT_PISTON;
    effect.position = piston->position;
    effect.action = request.action;
    effect.facing = (uint8_t)piston->facing;
    effect.sticky = (uint8_t)(piston->flags & 1u);
    effect.extended = prior.extended;
    if (!ne_effect_push(engine, effect)) return NE_ERR_ALLOC;
    engine->counters.piston_actions++;
    return NE_OK;
}

static NEStatus ne_drain_piston_actions(NEEngine *engine, NEStateView state) {
    NERequestQueue *queue = &engine->requests;
    while (queue->count != 0) {
        NEFuturePistonRequest request = queue->items[queue->head++];
        queue->count--;
        if (queue->count == 0) queue->head = 0;
        NEStatus status = ne_accept_piston_action(engine, state, request);
        if (status != NE_OK) return status;
    }
    return NE_OK;
}

static NEStatus ne_apply_deferred_resource(NEEngine *engine, NEStateView state, const NEFutureDeferredPiston *event,
                                           const NEPiston *piston) {
    if (!event->has_resource || event->resource >= engine->program->resource_count) return NE_OK;
    const NEResourceState *current = &state.resources[event->resource];
    if (current->revision != event->resource_revision || current->at != event->contact || current->enabled ||
        !ne_resource_contact_available(engine, state, event->resource, event->contact, NE_NONE)) return NE_OK;
    uint64_t revision;
    if (!ne_set_resource_contact(engine, state, event->resource, event->contact, 1u, &revision)) return NE_OK;
    uint32_t route = event->extending ? piston->routes[7] : piston->routes[6];
    return route != NE_NONE ? ne_deliver_route(engine, state, route) : NE_OK;
}

static NEStatus ne_apply_deferred_piston(NEEngine *engine, NEStateView state, const NEFutureDeferredPiston *event) {
    if (event->piston >= engine->program->piston_count) return NE_ERR_INVALID;
    NEPistonState *current = &state.pistons[event->piston];
    const NEPiston *piston = &engine->program->pistons[event->piston];
    int actor_valid = current->revision == event->revision;
    engine->counters.deferred_publications++;
    NEStatus status = NE_OK;
    if (event->extending) {
        status = ne_apply_deferred_resource(engine, state, event, piston);
        if (status != NE_OK) return status;
        if (actor_valid) {
            current->availability = NE_PISTON_READY;
            current->head_present = 1;
            if (piston->routes[5] != NE_NONE) status = ne_deliver_route(engine, state, piston->routes[5]);
        }
    } else {
        if (actor_valid) {
            current->extended = 0;
            current->head_present = 0;
            current->availability = NE_PISTON_READY;
            if (piston->routes[4] != NE_NONE) status = ne_deliver_route(engine, state, piston->routes[4]);
        }
        if (status == NE_OK) status = ne_apply_deferred_resource(engine, state, event, piston);
    }
    return status;
}

static NEStatus ne_publish_deferred(NEEngine *engine, NEStateView state) {
    NEDeferredQueue *queue = &engine->deferred;
    while (queue->count != 0) {
        NEFutureDeferredPiston event = queue->items[queue->head];
        if (event.due_tick > engine->logical_tick) break;
        queue->head++;
        queue->count--;
        if (queue->count == 0) queue->head = 0;
        NEStatus status = ne_apply_deferred_piston(engine, state, &event);
        if (status != NE_OK) return status;
    }
    return NE_OK;
}

