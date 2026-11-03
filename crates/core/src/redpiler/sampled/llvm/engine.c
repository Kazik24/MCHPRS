#include "abi.h"
#include "command_engine.c"

#define NE_OK 0u
#define NE_INVALID 1u
#define NE_UNSUPPORTED 2u
#define NE_CAPACITY 3u
#define NE_OPERATION_LIMIT 4u
#define NE_MAX_TICK_OPERATIONS 1000000u
#define NE_MAX_STATE_BYTES (256u * 1024u * 1024u)
#define NE_TABLES 61u
#define NONE UINT32_MAX
#define PASSIVE_TICK 0x80000000u

typedef struct {
    uint32_t piston, token, destination, order, flags, next;
    uint64_t start, landing;
} NEMotion;

typedef struct {
    uint32_t component;
    uint32_t next;
} NEEvent;

typedef struct { uint32_t first, last; } NEQueue;

typedef struct {
    const MchprsNativeProgramDescriptor *program;
    MchprsNativeProgramDescriptor program_copy;
    const uint8_t *blob;
    uint8_t *component_state;
    uint8_t *wire_sides;
    uint8_t *contributions;
    uint32_t *input_count;
    uint32_t *pending_count;
    uint32_t *side_count;
    uint32_t *histograms;
    uint32_t *side_histograms;
    uint8_t *comparator_outputs;
    uint8_t *reactive_strengths;
    uint8_t has_reactive;
    uint8_t *note_instruments, *note_queued;
    uint32_t *note_events, note_count;
    uint32_t *watched_by_component;
    uint8_t *watched_strengths;
    uint64_t *updated;
    uint64_t phase;
    NEEvent *events;
    NEQueue queues[32][4];
    uint32_t free_slot;
    uint32_t event_capacity;
    uint32_t initial_tick_count;
    uint32_t event_count;
    uint64_t half_tick;
    uint32_t tick_operations;
    uint64_t tick_count;
    uint64_t work_count;
    uint32_t error;
    uint32_t *token_cell, *token_digit, *groups, *piston_by_component, *piston_motion, *cell_motion;
    uint8_t *piston_flags, *queued_pistons, *arrangement_contributions, *switched, *changed;
    uint32_t *switched_nodes, *changed_nodes, *piston_events;
    uint32_t piston_event_first, piston_event_count, piston_event_capacity;
    uint32_t piston_count, token_count, cell_count, motion_capacity;
    NEMotion *motions;
    uint32_t motion_first, motion_last, motion_free, landing_cursor;
    uint8_t motion_phase;
    uint8_t *route_attenuation, *route_next, *route_distance;
    uint32_t *route_queue;
    NECommandEngine *commands;
    const uint32_t *command_notify_rows, *command_notify_targets;
    uint64_t command_now_ns, command_logical_base;
} NEEngine;

static const MchprsNativeComponent *components(const NEEngine *engine) {
    return (const MchprsNativeComponent *)(engine->blob + engine->program->component_offset);
}

static const uint32_t *sources(const NEEngine *engine) {
    return (const uint32_t *)(engine->blob + engine->program->source_offset);
}

static const uint8_t *operations(const NEEngine *engine) {
    return engine->blob + engine->program->operation_offset;
}

static const uint8_t *links(const NEEngine *engine) {
    return engine->blob + engine->program->link_offset;
}

/* Seven binary tables: side rows/data, source watchers, watched wires, watcher IDs, outgoing rows/IDs. */
static const uint32_t *aux_directory(const NEEngine *e) {
    return (const uint32_t *)(e->blob + e->program->source_offset + (size_t)e->program->source_count * 4u);
}
static const uint32_t *table(const NEEngine *e, uint32_t index) {
    return (const uint32_t *)(e->blob + aux_directory(e)[index * 2u]);
}
static uint32_t table_words(const NEEngine *e, uint32_t index) { return aux_directory(e)[index * 2u + 1u]; }
static uint32_t valid_movement(const MchprsNativeProgramDescriptor *, const uint8_t *, const uint32_t **, const uint32_t *);
static size_t movement_storage_bytes(const NEEngine *);
static uint32_t init_movement(NEEngine *, uint8_t *, size_t);
static uint32_t update_piston(NEEngine *, uint32_t);
static uint32_t run_piston_events(NEEngine *);
static uint32_t tick_motions(NEEngine *);
static uint32_t set_token_pos(NEEngine *, uint32_t, uint32_t);
static void refresh_arranged(NEEngine *, uint32_t);
static uint32_t update_note(NEEngine *, uint32_t);
static uint32_t dispatch_command_update(NEEngine *, uint32_t);
static uint32_t dispatch_command_tick(NEEngine *, uint32_t);

static uint32_t read_u32(const uint8_t *data) {
    return (uint32_t)data[0] | ((uint32_t)data[1] << 8) | ((uint32_t)data[2] << 16) | ((uint32_t)data[3] << 24);
}

static uint8_t output(const NEEngine *engine, uint32_t component) {
    const MchprsNativeComponent *record = &components(engine)[component];
    if (record->kind == 3u) return table(engine, 30u)[component] ? 0u : 15u;
    if (record->kind == 5u) return engine->component_state[component];
    if (record->kind == 9u) return engine->component_state[component] ? engine->comparator_outputs[component] : 0u;
    if (record->kind == 11u) return engine->component_state[component] ? 15u : 0u;
    if ((record->kind == 0u || record->kind == 1u || record->kind == 4u || record->kind == 6u || record->kind == 7u)
        && (engine->component_state[component] & 1u)) return 15u;
    return 0u;
}

static uint32_t has_pending(const NEEngine *engine, uint32_t component) {
    return engine->pending_count[component] != 0u;
}

static uint32_t enqueue(NEEngine *engine, uint32_t component, uint32_t delay, uint8_t priority) {
    if ((component & PASSIVE_TICK ? (component & ~PASSIVE_TICK) >= engine->initial_tick_count : component >= engine->program->component_count)
        || delay >= 32u || priority > 3u
        || (delay == 0u && priority != 3u)) return engine->error = NE_INVALID;
    uint32_t slot = engine->free_slot;
    if (slot == UINT32_MAX) return engine->error = NE_CAPACITY;
    engine->free_slot = engine->events[slot].next;
    engine->events[slot].component = component;
    engine->events[slot].next = UINT32_MAX;
    NEQueue *queue = &engine->queues[(engine->half_tick + delay) % 32u][priority];
    if (queue->last != UINT32_MAX) engine->events[queue->last].next = slot;
    else queue->first = slot;
    queue->last = slot;
    if (!(component & PASSIVE_TICK)) engine->pending_count[component]++;
    engine->event_count++;
    return NE_OK;
}

static uint32_t schedule(NEEngine *engine, uint32_t component, uint32_t delay, uint8_t priority) {
    if (has_pending(engine, component)) return NE_OK;
    return enqueue(engine, component, delay, priority);
}

static uint32_t execute_output(NEEngine *engine, uint32_t component, uint8_t powered);
static uint32_t update_once(NEEngine *engine, uint32_t component);

static uint32_t notify(NEEngine *e, uint32_t source, uint8_t shape, uint8_t update) {
    const uint32_t *row = table(e, 2u) + (size_t)source * 2u, *ids = table(e, 4u);
    for (uint32_t i = 0; i < row[1]; ++i) {
        uint32_t watcher = ids[row[0] + i];
        if (++e->tick_operations > NE_MAX_TICK_OPERATIONS) return e->error = NE_OPERATION_LIMIT;
        e->work_count++;
        if (components(e)[watcher].kind == 7u ? shape : update) {
            uint32_t result = update_once(e, watcher);
            if (result) return result;
        }
    }
    return NE_OK;
}

static void change_side_outputs(NEEngine *e, uint32_t node, uint8_t old, uint8_t value) {
    if (old == value) return;
    const uint32_t *row = table(e, 0u) + (size_t)node * 2u, *data = table(e, 1u);
    for (uint32_t i = 0; i < row[1]; ++i) {
        uint32_t target = data[(row[0] + i) * 2u], attenuation = data[(row[0] + i) * 2u + 1u];
        uint8_t before=old>attenuation?(uint8_t)(old-attenuation):0u;
        uint8_t after=value>attenuation?(uint8_t)(value-attenuation):0u;
        if (before) { e->side_count[target]--; e->side_histograms[(size_t)target*16u+before]--; }
        if (after) { e->side_count[target]++; e->side_histograms[(size_t)target*16u+after]++; }
    }
}
static void set_component(NEEngine *e, uint32_t node, uint8_t state) {
    uint8_t old = output(e, node);
    e->component_state[node] = state;
    change_side_outputs(e,node,old,output(e,node));
}

static void set_contribution(NEEngine *e, uint32_t index, uint8_t value) {
    const uint8_t *link = links(e) + (size_t)index * 12u;
    uint32_t target = read_u32(link + 4);
    uint8_t old = e->contributions[index];
    e->contributions[index] = value;
    if (link[9] && old != value) {
        if (old) { e->input_count[target]--; e->histograms[(size_t)target * 16u + old]--; }
        if (value) { e->input_count[target]++; e->histograms[(size_t)target * 16u + value]++; }
    }
    if ((old != 0u) != (value != 0u) || (old != value && (components(e)[target].kind==9u || e->watched_by_component[target] != UINT32_MAX))) e->updated[target] = 0u;
}

static uint8_t input_strength(const NEEngine *e, uint32_t node) {
    for (uint32_t level = 15u; level; --level) if (e->histograms[(size_t)node * 16u + level]) return (uint8_t)level;
    return 0u;
}

static uint8_t comparator_override(const NEEngine *e, uint32_t value) {
    if (value & 0x80000000u) return e->component_state[value & 0x7fffffffu] & 1u ? 15u : 0u;
    if (value & 0x40000000u) return (uint8_t)ne_command_success_count(e->commands, value & 0x3fffffffu, NULL);
    return (uint8_t)(value - 1u);
}

static void comparator_result(const NEEngine *e, uint32_t node, uint8_t *level, uint8_t *powered) {
    const uint32_t *r=table(e,42u)+node*4u;
    uint8_t rear=input_strength(e,node), side=0u;
    for (uint32_t i=15u;i;--i) if (e->side_histograms[(size_t)node*16u+i]) { side=(uint8_t)i; break; }
    if (r[1]) rear=comparator_override(e,r[1]);
    else if (rear<15u && r[2]) rear=comparator_override(e,r[2]);
    uint8_t subtract=(uint8_t)((r[0]>>1u)&1u);
    *powered=(uint8_t)(rear>0u && (rear>side || (rear==side && !subtract)));
    *level=side>rear?0u:subtract?(uint8_t)(rear-side):rear;
}

static uint32_t update_comparator(NEEngine *e, uint32_t node) {
    uint8_t level, powered; comparator_result(e,node,&level,&powered);
    if (level==e->comparator_outputs[node] && powered==e->component_state[node]) return NE_OK;
    return schedule(e,node,2u,(table(e,42u)[node*4u]&4u)?2u:3u);
}

static void refresh_wire(NEEngine *e, uint32_t wire) {
    uint8_t strength = input_strength(e, wire);
    if (e->component_state[wire] == strength) return;
    set_component(e, wire, strength);
    const uint32_t *row = table(e, 5u) + (size_t)wire * 2u, *ids = table(e, 6u);
    for (uint32_t i = 0; i < row[1]; ++i) {
        uint32_t index = ids[row[0] + i];
        uint8_t attenuation = links(e)[(size_t)index * 12u + 8u];
        set_contribution(e, index, strength > attenuation ? (uint8_t)(strength - attenuation) : 0u);
    }
    const uint32_t *arr=table(e,28u)+wire*2u;
    for (uint32_t i=0;i<arr[1];++i) refresh_arranged(e,table(e,29u)[arr[0]+i]);
}

static uint32_t update_repeater(NEEngine *engine, uint32_t component) {
    const MchprsNativeComponent *record = &components(engine)[component];
    if (record->reserved[1] & 4u) {
        uint8_t state = (engine->component_state[component] & 1u) | (engine->side_count[component] ? 2u : 0u);
        if (state != engine->component_state[component]) {
            set_component(engine, component, state);
            uint32_t status = notify(engine, component, 1u, 0u);
            if (status) return status;
        }
    }
    if (engine->component_state[component] & 2u) return NE_OK;
    uint8_t powered = engine->input_count[component] != 0u;
    if (powered == (engine->component_state[component] & 1u)) return NE_OK;
    uint8_t priority = (record->reserved[1] & 2u) ? 0u : powered ? 2u : 1u;
    return schedule(engine, component, record->reserved[0] * 2u, priority);
}

static uint32_t dispatch_lamp(NEEngine *engine, uint32_t component) {
    if (engine->input_count[component] != 0u) {
        if (!engine->component_state[component]) { set_component(engine, component, 1u); return notify(engine, component, 1u, 0u); }
        return NE_OK;
    }
    if (engine->component_state[component]) return schedule(engine, component, 4u, 3u);
    return NE_OK;
}

static uint32_t update_copper_bulb(NEEngine *e, uint32_t node) {
    uint8_t state=e->component_state[node], powered=e->input_count[node]!=0u;
    if (((state>>1u)&1u)==powered) return NE_OK;
    set_component(e,node,(uint8_t)(((state&1u)^powered)|(powered<<1u)));
    const uint32_t *row=table(e,59u)+node*2u;
    for (uint32_t i=0;i<row[1];++i) {
        uint32_t status=update_comparator(e,table(e,60u)[row[0]+i]);
        if (status) return status;
    }
    return notify(e,node,1u,1u);
}

static uint32_t update_component(NEEngine *e, uint32_t node) {
    uint8_t kind = components(e)[node].kind;
    if (kind == 2u) return dispatch_lamp(e, node);
    if (kind == 8u) return update_piston(e, node);
    if (kind == 4u) return update_repeater(e, node);
    if (kind == 9u) return update_comparator(e,node);
    if (kind == 10u) { set_component(e,node,(uint8_t)!e->input_count[node]); return NE_OK; }
    if (kind == 12u) return update_note(e,node);
    if (kind == 13u) return update_copper_bulb(e,node);
    if (kind == 14u) { set_component(e,node,(uint8_t)(e->input_count[node]!=0u)); return NE_OK; }
    if (kind == 15u) return dispatch_command_update(e,node);
    if (kind == 6u && e->component_state[node] == (e->input_count[node] != 0u)) return schedule(e, node, 2u, 3u);
    if (kind == 7u && !e->component_state[node]) return schedule(e, node, 2u, 3u);
    return NE_OK;
}
static uint32_t update_once(NEEngine *e, uint32_t node) {
    if (e->updated[node] == e->phase) return NE_OK;
    e->updated[node] = e->phase;
    return update_component(e, node);
}

static uint32_t update_wire_watcher(NEEngine *e, uint32_t node) {
    if (components(e)[node].kind == 7u) return e->component_state[node] ? schedule(e, node, 2u, 3u) : NE_OK;
    return update_once(e, node);
}

static uint32_t check_watched(NEEngine *e, uint32_t record) {
    const uint32_t *row = table(e, 3u) + (size_t)record * 3u, *ids = table(e, 4u);
    uint8_t strength = input_strength(e, row[0]);
    if (strength == e->watched_strengths[record]) return NE_OK;
    e->watched_strengths[record] = strength;
    for (uint32_t i = 0; i < row[2]; ++i) { uint32_t result = update_wire_watcher(e, ids[row[1] + i]); if (result) return result; }
    return NE_OK;
}

static uint32_t execute_operation(NEEngine *engine, const uint8_t *operation) {
    engine->work_count++;
    if (++engine->tick_operations > NE_MAX_TICK_OPERATIONS) return engine->error = NE_OPERATION_LIMIT;
    uint32_t operand = read_u32(operation + 4);
    if (operation[0] == 0u) {
        const uint8_t *link = links(engine) + (size_t)operand * 12u;
        uint32_t source = read_u32(link);
        uint32_t target = read_u32(link + 4);
        uint8_t value = output(engine, source);
        value = value > link[8] ? (uint8_t)(value - link[8]) : 0u;
        (void)target;
        set_contribution(engine, operand, value);
        return NE_OK;
    }
    if (operation[0] == 4u) return dispatch_lamp(engine, operand);
    if (operation[0] == 5u) return update_component(engine, operand);
    if (operation[0] == 6u) return update_component(engine, operand);
    if (operation[0] == 7u) return dispatch_command_update(engine, operand);
    if (operation[0] == 8u) return update_note(engine,operand);
    if (operation[0] == 9u) return update_copper_bulb(engine,operand);
    if (operation[0] == 10u) return update_repeater(engine, operand);
    if (operation[0] == 1u) { refresh_arranged(engine, operand); return NE_OK; }
    if (operation[0] == 2u) { refresh_wire(engine, operand); return NE_OK; }
    if (operation[0] == 3u) return check_watched(engine, operand);
    if (operation[0] == 12u) return update_component(engine, operand);
    if (operation[0] == 11u) return update_piston(engine, operand);
    if (operation[0] == 13u) return update_comparator(engine,operand);
    return engine->error = NE_UNSUPPORTED;
}

static uint32_t update_reactive(NEEngine *e, uint32_t source) {
    const uint32_t *row=table(e,43u)+source*5u;
    for (uint32_t i=0;i<row[2];++i) {
        uint32_t wire=table(e,44u)[row[1]+i];
        const uint32_t *record=table(e,45u)+wire*5u;
        uint8_t strength=0u;
        for (uint32_t j=0;j<record[2];++j) {
            const uint32_t *input=table(e,46u)+(record[1]+j)*2u;
            if (++e->tick_operations>NE_MAX_TICK_OPERATIONS) return e->error=NE_OPERATION_LIMIT;
            e->work_count++;
            uint8_t level=components(e)[input[0]].kind==3u?15u:output(e,input[0]);
            level=level>input[1]?(uint8_t)(level-input[1]):0u;
            if (level>strength) strength=level;
        }
        if (e->reactive_strengths[wire]==strength) continue;
        e->reactive_strengths[wire]=strength;
        for (uint32_t j=0;j<record[4];++j) {
            if (++e->tick_operations>NE_MAX_TICK_OPERATIONS) return e->error=NE_OPERATION_LIMIT;
            e->work_count++;
            uint32_t status=update_component(e,table(e,47u)[record[3]+j]); if (status) return status;
        }
    }
    for (uint32_t i=0;i<row[4];++i) {
        if (++e->tick_operations>NE_MAX_TICK_OPERATIONS) return e->error=NE_OPERATION_LIMIT;
        e->work_count++;
        uint32_t status=update_component(e,table(e,48u)[row[3]+i]); if (status) return status;
    }
    return NE_OK;
}

static uint32_t execute_output(NEEngine *engine, uint32_t component, uint8_t powered) {
    const MchprsNativeComponent *record = &components(engine)[component];
    uint32_t start = powered ? record->on_offset : record->off_offset;
    uint32_t count = powered ? record->on_count : record->off_count;
    if (engine->has_reactive) for (uint32_t i=0;i<count;++i) {
        const uint8_t *op=operations(engine)+(size_t)(start+i)*8u;
        if (op[0]>1u) continue;
        uint32_t operand=read_u32(op+4), target;
        uint8_t before;
        if (op[0]==0u) { target=read_u32(links(engine)+(size_t)operand*12u+4u); before=engine->contributions[operand]; }
        else { target=table(engine,25u)[operand*8u]; before=engine->arrangement_contributions[operand]; }
        uint32_t status=execute_operation(engine,op); if (status) return status;
        uint8_t after=op[0]==0u?engine->contributions[operand]:engine->arrangement_contributions[operand];
        if (before!=after) engine->updated[target]=0u;
    }
    for (uint32_t i = 0; i < count; ++i) {
        const uint8_t *op=operations(engine)+(size_t)(start+i)*8u;
        uint32_t operand=read_u32(op+4);
        if (engine->has_reactive && (op[0]<2u || ((op[0]==4u || op[0]==5u || (op[0]>=6u && op[0]<=10u) || op[0]==13u)
            && table(engine,43u)[operand*5u]))) continue;
        uint32_t status = execute_operation(engine, op);
        if (status != NE_OK) return status;
    }
    return engine->has_reactive?update_reactive(engine,component):NE_OK;
}

static uint32_t drain(NEEngine *engine) {
    for (;;) {
        NEQueue *queue = 0;
        for (uint32_t priority = 0; priority < 4u; ++priority) {
            NEQueue *candidate = &engine->queues[engine->half_tick % 32u][priority];
            if (candidate->first != UINT32_MAX) { queue = candidate; break; }
        }
        if (!queue) return NE_OK;
        uint32_t selected = queue->first;
        NEEvent event = engine->events[selected];
        queue->first = event.next;
        if (queue->first == UINT32_MAX) queue->last = UINT32_MAX;
        engine->events[selected].next = engine->free_slot;
        engine->free_slot = selected;
        if (!(event.component & PASSIVE_TICK)) engine->pending_count[event.component]--;
        engine->event_count--;
        if (++engine->tick_operations > NE_MAX_TICK_OPERATIONS) return engine->error = NE_OPERATION_LIMIT;
        if (event.component & PASSIVE_TICK) continue;
        engine->work_count++;
        const MchprsNativeComponent *component = &components(engine)[event.component];
        if (component->kind == 15u) {
            uint32_t status = dispatch_command_tick(engine, event.component);
            if (status) return status;
        } else if (component->kind == 1u && engine->component_state[event.component]) {
            set_component(engine, event.component, 0u);
            uint32_t status = execute_output(engine, event.component, 0u);
            if (status != NE_OK) return status;
            if ((status = notify(engine, event.component, 1u, 1u))) return status;
        } else if (component->kind == 2u && !engine->input_count[event.component]) {
            if (engine->component_state[event.component]) {
                set_component(engine, event.component, 0u);
                uint32_t status = notify(engine, event.component, 1u, 0u); if (status) return status;
            }
        } else if (component->kind == 4u && !(engine->component_state[event.component] & 2u)) {
            uint8_t powered = engine->component_state[event.component] & 1u;
            uint8_t rear = engine->input_count[event.component] != 0u;
            if (powered && rear) continue;
            if (!powered && !rear) {
                uint32_t status = enqueue(engine, event.component, component->reserved[0] * 2u, 1u);
                if (status != NE_OK) return status;
            }
            set_component(engine, event.component, (uint8_t)!powered);
            uint32_t status = execute_output(engine, event.component, (uint8_t)!powered);
            if (status != NE_OK) return status;
            if ((status = notify(engine, event.component, 1u, 1u))) return status;
        } else if (component->kind == 9u) {
            uint32_t node=event.component; uint8_t from=output(engine,node), level, powered;
            uint8_t previous=engine->comparator_outputs[node], old_powered=engine->component_state[node];
            comparator_result(engine,node,&level,&powered);
            engine->comparator_outputs[node]=level;
            change_side_outputs(engine,node,from,output(engine,node));
            if (previous==level && (table(engine,42u)[node*4u]&2u)) continue;
            set_component(engine,node,powered);
            uint8_t value=output(engine,node); uint32_t status=NE_OK;
            if (from!=value) status=execute_output(engine,node,(uint8_t)(value>from));
            else if (engine->has_reactive) status=update_reactive(engine,node);
            if (status || (status=notify(engine,node,(uint8_t)(old_powered!=powered),1u))) return status;
        } else if (component->kind == 6u && engine->component_state[event.component] == (engine->input_count[event.component] != 0u)) {
            uint8_t lit = (uint8_t)!engine->component_state[event.component];
            set_component(engine, event.component, lit);
            uint32_t status = execute_output(engine, event.component, lit);
            if (status || (status = notify(engine, event.component, 1u, 1u))) return status;
        } else if (component->kind == 7u) {
            uint8_t powered = (uint8_t)!engine->component_state[event.component];
            set_component(engine, event.component, powered);
            uint32_t status = notify(engine, event.component, 1u, 0u);
            if (status) return status;
            if (powered && (status = enqueue(engine, event.component, 2u, 3u))) return status;
            status = execute_output(engine, event.component, powered);
            if (status || (status = notify(engine, event.component, 0u, 1u))) return status;
        }
        if (engine->error != NE_OK) return engine->error;
    }
}

static uint32_t valid_aux(const MchprsNativeProgramDescriptor *p, const uint8_t *blob) {
    const uint32_t *directory = (const uint32_t *)(blob + p->source_offset + (size_t)p->source_count * 4u);
    uint64_t cursor = (uint64_t)p->source_offset + (uint64_t)p->source_count * 4u + NE_TABLES * 8u;
    const uint32_t *t[NE_TABLES]; uint32_t n[NE_TABLES];
    for (uint32_t i = 0; i < NE_TABLES; ++i) {
        n[i] = directory[i * 2u + 1u];
        if (directory[i * 2u] != cursor || cursor + (uint64_t)n[i] * 4u > p->total_bytes) return 0u;
        t[i] = (const uint32_t *)(blob + directory[i * 2u]);
        cursor += (uint64_t)n[i] * 4u;
    }
    if (cursor != p->total_bytes || (uint64_t)n[42] != (uint64_t)p->component_count*4u || (uint64_t)n[43] != (uint64_t)p->component_count*5u
        || (uint64_t)n[49] != (uint64_t)p->component_count*6u || n[16]%7u || n[11]%3u || n[50]!=n[16]/7u || n[51]!=n[11]/3u
        || n[45]%5u || n[46]%2u || (uint64_t)n[0] != (uint64_t)p->component_count * 2u
        || n[2] != n[0] || n[5] != n[0] || n[59] != n[0] || n[1] % 2u || (uint64_t)n[3] != (uint64_t)p->watched_wire_count * 3u) return 0u;
    for (uint32_t node = 0; node < p->component_count; ++node) {
        if ((uint64_t)t[0][node * 2u] + t[0][node * 2u + 1u] > n[1] / 2u
            || (uint64_t)t[2][node * 2u] + t[2][node * 2u + 1u] > n[4]
            || (uint64_t)t[5][node * 2u] + t[5][node * 2u + 1u] > n[6]
            || (uint64_t)t[59][node * 2u] + t[59][node * 2u + 1u] > n[60]) return 0u;
    }
    for (uint32_t i = 0; i < n[1]; i += 2u) if (t[1][i] >= p->component_count || t[1][i + 1u] > 15u) return 0u;
    for (uint32_t i = 0; i < n[3]; i += 3u) {
        if (t[3][i] >= p->component_count || blob[p->component_offset + (size_t)t[3][i] * 32u + 12u] != 5u
            || (uint64_t)t[3][i + 1u] + t[3][i + 2u] > n[4]) return 0u;
    }
    for (uint32_t i = 0; i < n[4]; ++i) if (t[4][i] >= p->component_count) return 0u;
    for (uint32_t i = 0; i < n[6]; ++i) if (t[6][i] >= p->link_count) return 0u;
    for (uint32_t i=0;i<p->component_count;++i) {
        const uint32_t *r=t[43]+i*5u;
        if (r[0]>1u || (uint64_t)r[1]+r[2]>n[44] || (uint64_t)r[3]+r[4]>n[48]) return 0u;
    }
    for (uint32_t i=0;i<n[44];++i) if (t[44][i]>=n[45]/5u) return 0u;
    for (uint32_t i=0;i<n[45];i+=5u) {
        const uint32_t *r=t[45]+i;
        if (r[0]>=p->component_count || blob[p->component_offset+(size_t)r[0]*32u+12u]!=5u
            || (uint64_t)r[1]+r[2]>n[46]/2u || (uint64_t)r[3]+r[4]>n[47]) return 0u;
    }
    for (uint32_t i=0;i<n[46];i+=2u) if (t[46][i]>=p->component_count || t[46][i+1u]>15u) return 0u;
    for (uint32_t i=0;i<n[47];++i) if (t[47][i]>=p->component_count) return 0u;
    for (uint32_t i=0;i<n[48];++i) if (t[48][i]>=p->component_count) return 0u;
    for (uint32_t i=0;i<p->component_count;++i) {
        const uint32_t *r=t[49]+i*6u;
        if ((r[0]!=NONE && r[0]>=n[50]) || (r[2]!=NONE && r[2]>=n[50]) || (r[1]&~287u) || (r[3]&~287u) || r[4]>24u || r[5]>15u) return 0u;
    }
    for (uint32_t i=0;i<n[50];++i) if (t[50][i]&~287u) return 0u;
    for (uint32_t i=0;i<n[51];++i) if (t[51][i]&~287u) return 0u;
    if (n[52] != p->component_count) return 0u;
    for (uint32_t i = 0; i < n[52]; ++i) {
        uint32_t sides = t[52][i];
        if (sides > 255u || (sides && blob[p->component_offset + (size_t)i * 32u + 12u] != 5u)) return 0u;
        for (uint32_t side = 0; side < 4u; ++side)
            if (((sides >> (side * 2u)) & 3u) == 3u) return 0u;
    }
    return valid_movement(p, blob, t, n);
}

static uint32_t valid_program(const MchprsNativeProgramDescriptor *program, const uint8_t *blob, uint32_t blob_bytes) {
    if (!program || !blob || (uintptr_t)blob % _Alignof(uint32_t) != 0u || blob_bytes < 16u || program->abi_version != 3u || program->header_bytes != 16u
        || program->total_bytes != blob_bytes || blob[0] != 'M' || blob[1] != 'C' || blob[2] != 'H' || blob[3] != 'N'
        || read_u32(blob + 4) != 3u || read_u32(blob + 8) != 16u || read_u32(blob + 12) != program->total_bytes) return 0u;
    if (program->operation_offset != 16u
        || (uint64_t)program->link_offset != (uint64_t)program->operation_offset + (uint64_t)program->operation_count * 8u
        || (uint64_t)program->component_offset != (uint64_t)program->link_offset + (uint64_t)program->link_count * 12u
        || (uint64_t)program->source_offset != (uint64_t)program->component_offset + (uint64_t)program->component_count * 32u
        || (uint64_t)program->total_bytes < (uint64_t)program->source_offset + (uint64_t)program->source_count * 4u + NE_TABLES * 8u) return 0u;
    if (!valid_aux(program, blob)) return 0u;
    const MchprsNativeComponent *component = (const MchprsNativeComponent *)(blob + program->component_offset);
    for (uint32_t i = 0; i < program->component_count; ++i) {
        if (component[i].kind > 15u || component[i].initial_state > (component[i].kind == 5u ? 15u : (component[i].kind == 4u || component[i].kind == 13u) ? 3u : 1u)
            || (component[i].kind == 4u ? (component[i].reserved[0] < 1u || component[i].reserved[0] > 4u || component[i].reserved[1] > 6u || (component[i].reserved[1] & 1u))
                : (component[i].reserved[0] || component[i].reserved[1]))
            || (uint64_t)component[i].on_offset + component[i].on_count > program->operation_count
            || (uint64_t)component[i].off_offset + component[i].off_count > program->operation_count) return 0u;
        if (component[i].kind == 2u && (component[i].on_count || component[i].off_count)) return 0u;
        const uint32_t *c=(const uint32_t *)(blob + ((const uint32_t *)(blob+program->source_offset+(size_t)program->source_count*4u))[42u*2u])+i*4u;
        if (component[i].kind==9u) {
            if (!(c[0]&1u) || c[0]>7u || c[3]>15u) return 0u;
            for (uint32_t j=1u;j<=2u;++j) {
                if (c[j]&0xc0000000u) {
                    uint32_t tag=c[j]&0xc0000000u, source=c[j]&0x3fffffffu;
                    if (tag==0xc0000000u || source>=program->component_count || component[source].kind!=(tag==0x80000000u?13u:15u)) return 0u;
                } else if (c[j]>16u) return 0u;
            }
        } else if (c[0] || c[1] || c[2] || c[3]) return 0u;
    }
    const uint32_t *override_ids=(const uint32_t *)(blob+((const uint32_t *)(blob+program->source_offset+(size_t)program->source_count*4u))[60u*2u]);
    uint32_t override_count=((const uint32_t *)(blob+program->source_offset+(size_t)program->source_count*4u))[60u*2u+1u];
    for (uint32_t i=0;i<override_count;++i)
        if (override_ids[i]>=program->component_count || component[override_ids[i]].kind!=9u) return 0u;
    const uint8_t *link = blob + program->link_offset;
    for (uint32_t i = 0; i < program->link_count; ++i, link += 12u) {
        uint32_t source = read_u32(link), target = read_u32(link + 4);
        if (source >= program->component_count || target >= program->component_count || link[8] > 15u || link[9] > 1u || link[10] || link[11]
            || component[source].kind == 2u || (component[target].kind != 2u && component[target].kind < 4u)) return 0u;
    }
    for (uint32_t i = 0; i < program->operation_count; ++i) {
        const uint8_t *op = blob + program->operation_offset + (size_t)i * 8u;
        uint32_t operand = read_u32(op + 4);
        if (op[1] || op[2] || op[3]) return 0u;
        if (op[0] == 0u) {
            if (operand >= program->link_count) return 0u;
        } else if (op[0] == 4u) {
            if (operand >= program->component_count || component[operand].kind != 2u) return 0u;
        } else if (op[0] == 5u) {
            if (operand >= program->component_count || component[operand].kind != 10u) return 0u;
        } else if (op[0] == 6u) {
            if (operand >= program->component_count || component[operand].kind != 14u) return 0u;
        } else if (op[0] == 7u) {
            if (operand >= program->component_count || component[operand].kind != 15u) return 0u;
        } else if (op[0] == 8u) {
            if (operand >= program->component_count || component[operand].kind != 12u) return 0u;
        } else if (op[0] == 9u) {
            if (operand >= program->component_count || component[operand].kind != 13u) return 0u;
        } else if (op[0] == 10u) {
            if (operand >= program->component_count || component[operand].kind != 4u) return 0u;
        } else if (op[0] == 2u || op[0] == 12u || op[0] == 11u || op[0] == 13u) {
            if (operand >= program->component_count || component[operand].kind != (op[0] == 2u ? 5u : op[0] == 11u ? 8u : op[0] == 13u ? 9u : 6u)) return 0u;
        } else if (op[0] == 1u) {
            if (operand >= program->arrangement_link_count) return 0u;
        } else if (op[0] == 3u) {
            if (operand >= program->watched_wire_count) return 0u;
        } else return 0u;
    }
    const uint32_t *source = (const uint32_t *)(blob + program->source_offset);
    for (uint32_t i = 0; i < program->source_count; ++i)
        if (source[i] >= program->component_count || (component[source[i]].kind != 0u && component[source[i]].kind != 1u)) return 0u;
    return 1u;
}

static size_t align8(size_t value) { return (value + 7u) & ~(size_t)7u; }

MchprsNativeApiDescriptor ne_api_descriptor(void) {
    MchprsNativeApiDescriptor value = {3u, 16u, 22u};
    return value;
}

size_t ne_state_size(const MchprsNativeProgramDescriptor *program, const uint8_t *blob, uint32_t blob_bytes, uint32_t initial_ticks) {
    if (!valid_program(program, blob, blob_bytes)) return 0u;
    if (program->component_count >= PASSIVE_TICK || initial_ticks >= PASSIVE_TICK) return 0u;
    size_t count = program->component_count, links_count = program->link_count;
    uint64_t requested = (uint64_t)count * 2u + initial_ticks;
    if (requested > UINT32_MAX) return 0u;
    size_t capacity = requested < 64u ? 64u : (size_t)requested;
    size_t bytes = align8(sizeof(NEEngine));
    NEEngine view = {0}; view.program = program; view.blob = blob;
    if (count > NE_MAX_STATE_BYTES || links_count > NE_MAX_STATE_BYTES || capacity > NE_MAX_STATE_BYTES / sizeof(NEEvent)) return 0u;
    bytes += count;
    bytes = align8(bytes) + count;
    bytes = align8(bytes) + links_count;
    bytes = align8(bytes) + count * sizeof(uint32_t);
    bytes = align8(bytes) + count * sizeof(uint32_t);
    bytes = align8(bytes) + count * sizeof(uint32_t);
    bytes = align8(bytes) + count * 16u * sizeof(uint32_t);
    bytes = align8(bytes) + count * 16u * sizeof(uint32_t);
    bytes = align8(bytes) + count;
    bytes = align8(bytes) + table_words(&view,45u)/5u;
    bytes = align8(bytes) + count;
    bytes = align8(bytes) + count;
    bytes = align8(bytes) + count*3u*sizeof(uint32_t);
    bytes = align8(bytes) + count * sizeof(uint32_t);
    bytes = align8(bytes) + program->watched_wire_count;
    bytes = align8(bytes) + count * sizeof(uint64_t);
    bytes = align8(bytes) + capacity * sizeof(NEEvent);
    bytes = align8(bytes) + movement_storage_bytes(&view);
    return bytes <= NE_MAX_STATE_BYTES ? bytes : 0u;
}

uint32_t ne_create(const MchprsNativeProgramDescriptor *program, const uint8_t *blob, uint32_t blob_bytes, void *memory, size_t memory_bytes, uint32_t initial_ticks) {
    size_t needed = ne_state_size(program, blob, blob_bytes, initial_ticks);
    if (!needed || !memory || (uintptr_t)memory % _Alignof(NEEngine) != 0u || memory_bytes < needed || !valid_program(program, blob, blob_bytes)) return NE_INVALID;
    uint8_t *base = (uint8_t *)memory;
    NEEngine *engine = (NEEngine *)base;
    size_t offset = align8(sizeof(NEEngine));
    engine->program_copy = *program;
    engine->program = &engine->program_copy;
    engine->blob = blob;
    engine->component_state = base + offset;
    offset = align8(offset + program->component_count);
    engine->wire_sides = base + offset;
    offset = align8(offset + program->component_count);
    engine->contributions = base + offset;
    offset = align8(offset + program->link_count);
    engine->input_count = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * sizeof(uint32_t));
    engine->pending_count = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * sizeof(uint32_t));
    engine->side_count = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * sizeof(uint32_t));
    engine->histograms = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * 16u * sizeof(uint32_t));
    engine->side_histograms = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * 16u * sizeof(uint32_t));
    engine->comparator_outputs = base + offset;
    offset = align8(offset + program->component_count);
    engine->reactive_strengths = base + offset;
    offset = align8(offset + table_words(engine,45u)/5u);
    engine->note_instruments = base + offset;
    offset = align8(offset + program->component_count);
    engine->note_queued = base + offset;
    offset = align8(offset + program->component_count);
    engine->note_events = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count*3u*sizeof(uint32_t));
    engine->watched_by_component = (uint32_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * sizeof(uint32_t));
    engine->watched_strengths = base + offset;
    offset = align8(offset + program->watched_wire_count);
    engine->updated = (uint64_t *)(base + offset);
    offset = align8(offset + (size_t)program->component_count * sizeof(uint64_t));
    engine->phase = 1u;
    engine->events = (NEEvent *)(base + offset);
    engine->event_capacity = program->component_count * 2u + initial_ticks;
    if (engine->event_capacity < 64u) engine->event_capacity = 64u;
    engine->initial_tick_count = initial_ticks;
    engine->event_count = 0u;
    engine->half_tick = 0u;
    engine->tick_operations = 0u;
    engine->tick_count = 0u;
    engine->work_count = 0u;
    engine->error = NE_OK;
    engine->commands = NULL;
    engine->command_notify_rows = NULL;
    engine->command_notify_targets = NULL;
    engine->command_now_ns = engine->command_logical_base = 0u;
    engine->has_reactive = 0u;
    engine->note_count = 0u;
    for (uint32_t i = 0; i < program->component_count; ++i) {
        engine->component_state[i] = components(engine)[i].initial_state;
        engine->wire_sides[i] = (uint8_t)table(engine, 52u)[i];
        engine->comparator_outputs[i] = (uint8_t)table(engine,42u)[i*4u+3u];
        engine->note_instruments[i]=(uint8_t)table(engine,49u)[i*6u+5u];
        engine->note_queued[i]=0u;
        if (table(engine,43u)[i*5u]) engine->has_reactive=1u;
        engine->input_count[i] = 0u;
        engine->pending_count[i] = 0u;
        engine->side_count[i] = 0u;
        engine->watched_by_component[i] = UINT32_MAX;
        engine->updated[i] = 0u;
        for (uint32_t level = 0; level < 16u; ++level) {
            engine->histograms[(size_t)i * 16u + level] = 0u;
            engine->side_histograms[(size_t)i * 16u + level] = 0u;
        }
    }
    for (uint32_t i = 0; i < program->watched_wire_count; ++i) {
        uint32_t wire = table(engine, 3u)[i * 3u];
        engine->watched_by_component[wire] = i;
        engine->watched_strengths[i] = engine->component_state[wire];
    }
    for (uint32_t i=0;i<table_words(engine,45u)/5u;++i)
        engine->reactive_strengths[i]=engine->component_state[table(engine,45u)[i*5u]];
    for (uint32_t i = 0; i < program->link_count; ++i) engine->contributions[i] = 0u;
    engine->free_slot = 0u;
    for (uint32_t i = 0; i < engine->event_capacity; ++i)
        engine->events[i].next = i + 1u < engine->event_capacity ? i + 1u : UINT32_MAX;
    for (uint32_t bucket = 0; bucket < 32u; ++bucket)
        for (uint32_t priority = 0; priority < 4u; ++priority)
            engine->queues[bucket][priority].first = engine->queues[bucket][priority].last = UINT32_MAX;
    offset = align8(offset + (size_t)engine->event_capacity * sizeof(NEEvent));
    if (init_movement(engine, base + offset, memory_bytes - offset)) return NE_INVALID;
    const uint8_t *link = links(engine);
    for (uint32_t i = 0; i < program->link_count; ++i, link += 12u) {
        uint8_t value = output(engine, read_u32(link));
        value = value > link[8] ? (uint8_t)(value - link[8]) : 0u;
        set_contribution(engine, i, value);
    }
    const uint32_t *side_data = table(engine, 1u);
    for (uint32_t source = 0; source < program->component_count; ++source) {
        const uint32_t *row = table(engine, 0u) + source * 2u;
        for (uint32_t i = 0; i < row[1]; ++i) {
            uint32_t index = (row[0] + i) * 2u;
            uint8_t strength=output(engine,source);
            if (strength > side_data[index + 1u]) {
                uint32_t target=side_data[index]; uint8_t level=(uint8_t)(strength-side_data[index+1u]);
                engine->side_count[target]++; engine->side_histograms[(size_t)target*16u+level]++;
            }
        }
    }
    return NE_OK;
}

void ne_destroy(void *memory) { (void)memory; }

uint32_t ne_import_tick(void *memory, uint32_t component, uint32_t delay, uint8_t priority) {
    NEEngine *engine = (NEEngine *)memory;
    if (!engine || engine->error) return NE_INVALID;
    return enqueue(engine, component, delay, priority);
}

uint32_t ne_tick(void *memory) {
    NEEngine *engine = (NEEngine *)memory;
    if (!engine || engine->error) return engine ? engine->error : NE_INVALID;
    engine->tick_count++;
    engine->phase++;
    if (!engine->phase) { engine->phase = 1u; for (uint32_t i = 0; i < engine->program->component_count; ++i) engine->updated[i] = 0u; }
    uint32_t result = drain(engine);
    if (result != NE_OK) return result;
    engine->half_tick++;
    result = drain(engine);
    if (!result && engine->piston_count) {
        engine->phase++;
        result = run_piston_events(engine);
        if (!result) { engine->phase++; result = tick_motions(engine); }
    }
    engine->tick_operations = 0u;
    return result;
}

uint32_t ne_use_control(void *memory, uint32_t source_index) {
    NEEngine *engine = (NEEngine *)memory;
    if (!engine || engine->error || source_index >= engine->program->source_count) return NE_INVALID;
    uint32_t component = sources(engine)[source_index];
    uint8_t kind = components(engine)[component].kind;
    uint8_t powered = kind == 0u ? (uint8_t)!engine->component_state[component] : 1u;
    if (engine->component_state[component] == powered) return NE_OK;
    engine->phase++;
    if (!engine->phase) { engine->phase = 1u; for (uint32_t i = 0; i < engine->program->component_count; ++i) engine->updated[i] = 0u; }
    set_component(engine, component, powered);
    if (kind == 1u && powered) {
        uint32_t result = enqueue(engine, component, 20u, 3u);
        if (result != NE_OK) return result;
    }
    uint32_t result = execute_output(engine, component, powered);
    return result ? result : notify(engine, component, 1u, 1u);
}

uint32_t ne_read_state(const void *memory, uint8_t *out, uint32_t count) {
    const NEEngine *engine = (const NEEngine *)memory;
    if (!engine || !out || count != engine->program->component_count) return NE_INVALID;
    for (uint32_t i = 0; i < count; ++i) out[i] = engine->component_state[i];
    return NE_OK;
}

uint64_t ne_tick_count(const void *memory) {
    const NEEngine *engine = (const NEEngine *)memory;
    return engine ? engine->tick_count : 0u;
}

uint32_t ne_pending_count(const void *memory) {
    const NEEngine *engine = (const NEEngine *)memory;
    return engine ? engine->event_count : 0u;
}

uint32_t ne_read_pending(const void *memory, MchprsNativePendingTick *out, uint32_t count) {
    const NEEngine *engine = (const NEEngine *)memory;
    if (!engine || (!out && count) || count != engine->event_count) return NE_INVALID;
    uint32_t written = 0u;
    for (uint32_t delay = 0; delay < 32u; ++delay) {
        for (uint32_t priority = 0; priority < 4u; ++priority) {
            uint32_t slot = engine->queues[(engine->half_tick + delay) % 32u][priority].first;
            while (slot != UINT32_MAX) {
                if (slot >= engine->event_capacity || written >= count) return NE_INVALID;
                const NEEvent *event = &engine->events[slot];
                out[written].component = event->component;
                out[written].ticks_left = delay;
                out[written].priority = (uint8_t)priority;
                out[written].reserved[0] = out[written].reserved[1] = out[written].reserved[2] = 0u;
                written++;
                slot = event->next;
            }
        }
    }
    return written == count ? NE_OK : NE_INVALID;
}

uint64_t ne_work_count(const void *memory) {
    const NEEngine *engine = (const NEEngine *)memory;
    return engine ? engine->work_count : 0u;
}

#include "llvm_movement.c"

static uint8_t command_powered(void *context, uint32_t component) {
    NEEngine *e = (NEEngine *)context;
    return (uint8_t)(e->input_count[component] != 0u);
}

static uint8_t command_pending(void *context, uint32_t component) {
    return (uint8_t)(has_pending((NEEngine *)context, component) != 0u);
}

static uint32_t command_schedule(void *context, uint32_t component, uint32_t delay, uint8_t priority) {
    return enqueue((NEEngine *)context, component, delay, priority);
}

static uint32_t command_notify(void *context, uint32_t component) {
    NEEngine *e = (NEEngine *)context;
    const uint32_t *row = e->command_notify_rows + (size_t)component * 2u;
    for (uint32_t i = 0; i < row[1]; ++i) {
        const uint32_t *target = e->command_notify_targets + (size_t)(row[0] + i) * 3u;
        if (!(note_meta(e, target[1], target[2]) & 0x100u)) continue;
        uint32_t node = target[0], status;
        if (components(e)[node].kind == 5u) {
            refresh_wire(e, node);
            uint32_t watched = e->watched_by_component[node];
            status = watched == NONE ? NE_OK : check_watched(e, watched);
        } else {
            status = update_component(e, node);
        }
        if (status) return status;
    }
    return NE_OK;
}

static NECommandHooks command_hooks(NEEngine *e) {
    NECommandHooks hooks = {e, command_powered, command_pending, command_schedule, command_notify};
    return hooks;
}

static uint32_t dispatch_command_update(NEEngine *e, uint32_t component) {
    if (!e->commands) return e->error = NE_INVALID;
    NECommandHooks hooks = command_hooks(e);
    uint32_t status = ne_command_update(e->commands, component, &hooks);
    return status ? (e->error = status) : NE_OK;
}

static uint32_t dispatch_command_tick(NEEngine *e, uint32_t component) {
    if (!e->commands) return e->error = NE_INVALID;
    NECommandHooks hooks = command_hooks(e);
    uint64_t tick = e->command_logical_base > UINT64_MAX - e->tick_count ? UINT64_MAX : e->command_logical_base + e->tick_count;
    uint32_t status = ne_command_tick(e->commands, component, tick, e->command_now_ns, &hooks);
    return status ? (e->error = status) : NE_OK;
}

uint32_t ne_bind_commands(void *memory, void *commands, const uint32_t *notify_rows, const uint32_t *notify_targets, uint32_t target_count) {
    NEEngine *e = (NEEngine *)memory;
    NECommandEngine *state = (NECommandEngine *)commands;
    if (!e || !state || state->component_count != e->program->component_count || target_count > UINT32_MAX / 3u
        || (!notify_rows && e->program->component_count) || (!notify_targets && target_count)) return NE_INVALID;
    for (uint32_t i = 0; i < e->program->component_count; ++i) {
        if (!bounded(notify_rows[(size_t)i * 2u], notify_rows[(size_t)i * 2u + 1u], target_count)
            || (components(e)[i].kind == 15u && command_index(state, i) == NE_CMD_NONE)) return NE_INVALID;
    }
    for (uint32_t i = 0; i < state->block_count; ++i)
        if (components(e)[state->block_rows[(size_t)i * NE_CMD_BLOCK_WORDS]].kind != 15u) return NE_INVALID;
    for (uint32_t i = 0; i < target_count; ++i) {
        const uint32_t *target = notify_targets + (size_t)i * 3u;
        if (target[0] >= e->program->component_count || (target[1] != NONE && target[1] >= e->cell_count)
            || (target[2] & ~0x100u)) return NE_INVALID;
    }
    e->commands = state;
    e->command_notify_rows = notify_rows;
    e->command_notify_targets = notify_targets;
    return NE_OK;
}

void ne_set_command_clock(void *memory, uint64_t now_ns, uint64_t logical_base) {
    NEEngine *e = (NEEngine *)memory;
    if (e) {
        e->command_now_ns = now_ns;
        e->command_logical_base = logical_base;
    }
}

uint64_t ne_command_effect_bound(const void *memory) {
    const NEEngine *e = (const NEEngine *)memory;
    if (!e || !e->commands) return 0u;
    uint64_t roots = (uint64_t)e->commands->block_count * 2u;
    for (uint32_t i = 0; i < e->commands->block_count; ++i)
        roots += e->pending_count[e->commands->block_rows[(size_t)i * NE_CMD_BLOCK_WORDS]];
    /* Each scheduled root can execute itself and the interpreter's 256 chain steps. */
    return roots * 257u;
}
