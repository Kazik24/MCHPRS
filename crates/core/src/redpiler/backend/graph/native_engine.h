#ifndef MCHPRS_NATIVE_ENGINE_H
#define MCHPRS_NATIVE_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NE_NONE UINT32_MAX
#define NE_FACE_NONE UINT8_MAX

/* NENode.flags */
#define NE_NODE_FLAG_REPEATER_FACING_DIODE (1u << 0)
#define NE_NODE_FLAG_COMPARATOR_FACING_DIODE (1u << 1)
#define NE_NODE_FLAG_COMMAND_REPEATING (1u << 2)
#define NE_NODE_FLAG_COMMAND_CHAIN (1u << 3)
#define NE_NODE_FLAG_COMMAND_AUTOMATIC (1u << 4)
#define NE_NODE_FLAG_COMMAND_INITIAL_TICK (1u << 5)
#define NE_NODE_FLAG_COMMAND_CONDITIONAL (1u << 6)
#define NE_NODE_FLAG_NOTE_POWERED (1u << 7)

/* NEProfile.flags */
#define NE_PROFILE_FLAG_IS_WIRE (1u << 0)
#define NE_PROFILE_FLAG_CONTAINER (1u << 1)
#define NE_PROFILE_FLAG_MOVING_PISTON (1u << 2)
#define NE_PROFILE_FLAG_PISTON_HEAD (1u << 3)
#define NE_PROFILE_FLAG_AIR (1u << 4)

/* RedstoneWireSide byte values used by NEComponentState. */
#define NE_WIRE_SIDE_UP 0u
#define NE_WIRE_SIDE_SIDE 1u
#define NE_WIRE_SIDE_NONE 2u

/* NEWire.flags */
#define NE_WIRE_FLAG_QUERY_FACTS (1u << 0)
#define NE_WIRE_FLAG_TRANSFER (1u << 1)
#define NE_WIRE_FLAG_ISOLATED (1u << 2)
#define NE_WIRE_FLAG_INSTANT_RESET (1u << 3)

typedef enum {
    NE_OK = 0,
    NE_ERR_INVALID = 1,
    NE_ERR_UNSUPPORTED = 2,
    NE_ERR_CAPACITY = 3,
    NE_ERR_ALLOC = 4,
    NE_ERR_BAD_STATE = 5
} NEStatus;

typedef enum {
    NE_NODE_ELECTRICAL = 0,
    NE_NODE_OBSERVER = 1,
    NE_NODE_BUILTIN_RESET = 2,
    NE_NODE_WIRE = 3,
    NE_NODE_PISTON = 4,
    NE_NODE_CONTACT = 5
} NENodeKind;

typedef enum {
    NE_ELECTRICAL_REPEATER = 0,
    NE_ELECTRICAL_TORCH = 1,
    NE_ELECTRICAL_COMPARATOR = 2,
    NE_ELECTRICAL_LAMP = 3,
    NE_ELECTRICAL_COPPER_BULB = 4,
    NE_ELECTRICAL_HOPPER = 5,
    NE_ELECTRICAL_BUTTON = 6,
    NE_ELECTRICAL_LEVER = 7,
    NE_ELECTRICAL_PRESSURE_PLATE = 8,
    NE_ELECTRICAL_TRAPDOOR = 9,
    NE_ELECTRICAL_CONSTANT = 10,
    NE_ELECTRICAL_COMMAND_BLOCK = 11,
    NE_ELECTRICAL_NOTE_BLOCK = 12
} NEElectricalKind;

typedef enum {
    NE_GUARD_ALWAYS = 0,
    NE_GUARD_STATE = 1,
    NE_GUARD_SITE = 2,
    NE_GUARD_NOT = 3,
    NE_GUARD_ALL = 4,
    NE_GUARD_ANY = 5
} NEGuardKind;

typedef enum {
    NE_STATE_PISTON_EXTENDED = 0,
    NE_STATE_PISTON_HEAD_PRESENT = 1,
    NE_STATE_PISTON_AVAILABILITY = 2,
    NE_STATE_RESOURCE_AT = 3,
    NE_STATE_RESOURCE_ENABLED = 4
} NEStateTestKind;

typedef enum {
    NE_SITE_SOLID = 0,
    NE_SITE_TRANSPARENT = 1,
    NE_SITE_IS_WIRE = 2,
    NE_SITE_IS_PROFILE = 3
} NESiteTestKind;

typedef enum {
    NE_SOURCE_COMPONENT = 0,
    NE_SOURCE_CONTACT = 1,
    NE_SOURCE_CONSTANT = 2
} NESiteSourceKind;

typedef enum {
    NE_PROFILE_NONE = 0,
    NE_PROFILE_REDSTONE_SOURCE = 1,
    NE_PROFILE_COMPONENT = 2
} NEProfileElectrical;

typedef enum {
    NE_DELIVERY_UPDATE = 0,
    NE_DELIVERY_SHAPE = 1
} NEDeliveryKind;

typedef enum {
    NE_INSTANT_DISPATCH = 0,
    NE_INSTANT_SAMPLE = 1,
    NE_INSTANT_ARM_RESET = 2
} NEInstantStepKind;

typedef enum {
    NE_RECEIVER_NONE = 0,
    NE_RECEIVER_COMPONENT = 1,
    NE_RECEIVER_PISTON_BASE = 2,
    NE_RECEIVER_PISTON_HEAD = 3,
    NE_RECEIVER_CONTACT = 4
} NEReceiverKind;

typedef enum {
    NE_PORT_OUTPUT = 0,
    NE_PORT_STORED = 1,
    NE_PORT_WEAK = 2,
    NE_PORT_STRONG = 3,
    NE_PORT_CONDUCTED = 4
} NEPowerPortKind;

typedef enum {
    NE_TARGET_INPUT = 0,
    NE_TARGET_CONDUCTOR = 1
} NEPowerTargetKind;

typedef enum {
    NE_PISTON_READY = 0,
    NE_PISTON_HEAD_PENDING = 1,
    NE_PISTON_PUBLISHING_HEAD_REMOVAL = 2,
    NE_PISTON_BASE_PENDING_HEAD_PRESENT = 3,
    NE_PISTON_BASE_PENDING = 4
} NEPistonAvailability;

typedef enum {
    NE_LANE_EMPTY = 0,
    NE_LANE_SINGLE_RESOURCE = 1
} NEPistonLaneKind;

typedef enum {
    NE_ACTION_EXTEND = 0,
    NE_ACTION_RETRACT = 1,
    NE_ACTION_RETRACT_WITHOUT_PULL = 2
} NEPistonAction;

typedef struct {
    int32_t x, y, z;
} NEPosition;

/* One immutable lowered node. `kind_data` is interpreted by its node kind:
   electrical: [delay/mode, facing, flags, watched-site]; observer/reset:
   [facing, watched-site]; wire/piston/contact: index into their table. */
typedef struct {
    /* STATE: a=test kind, b=piston/resource ID, child_start=contact ID for
       RESOURCE_AT and value=availability for PISTON_AVAILABILITY. SITE: a=site
       ID, b=profile ID for IS_PROFILE, test/view select predicate input.
       NOT: a=child guard. ALL/ANY: child_start/count index guard_children. */
    uint32_t kind;
    uint32_t electrical_kind;
    uint32_t flags;
    uint32_t kind_data[4];
    uint32_t output_route;
    uint32_t profile;
    uint32_t registry_type;
    uint32_t raw_state;
    uint32_t weak_faces;
    uint32_t strong_faces;
    uint32_t wire_connect_faces;
    NEPosition position;
} NENode;

typedef struct {
    uint8_t powered;
    uint8_t repeater_locked;
    uint8_t output_strength;
    uint8_t pending_tick;
    uint8_t wire_north, wire_south, wire_east, wire_west;
    uint8_t wire_power;
    uint8_t reserved[3];
} NEComponentState;

typedef struct {
    uint8_t extended;
    uint8_t head_present;
    uint8_t availability;
    uint8_t reserved[5];
    uint64_t revision;
} NEPistonState;

typedef struct {
    uint32_t at;
    uint8_t enabled;
    uint8_t reserved[3];
    uint64_t revision;
} NEResourceState;

typedef struct {
    NEComponentState *components;
    NEPistonState *pistons;
    NEResourceState *resources;
    uint8_t *classic_signals;
} NEStateView;

typedef struct {
    uint32_t kind;
    uint32_t a;
    uint32_t b;
    uint32_t child_start;
    uint32_t child_count;
    uint8_t view;
    uint8_t test;
    uint8_t value;
    uint8_t reserved;
} NEGuard;

typedef struct {
    uint32_t start;
    uint32_t count;
} NERange;

typedef struct {
    uint32_t target_site;
    uint32_t guard;
    uint32_t receiver_id;
    uint8_t kind;
    uint8_t direction;
    uint8_t view;
    uint8_t receiver_kind;
} NEDelivery;

typedef struct {
    uint32_t node;
    uint32_t first_delivery;
    uint32_t delivery_count;
    /* NE_NONE means ordinary guarded delivery; otherwise this route binds to
       the indicated flattened instant-step range. Original delivery rows stay
       present for validation and Turbo route offsets. */
    uint32_t first_instant_step;
    uint32_t instant_step_count;
} NERoute;

typedef struct {
    uint32_t delivery; /* original route delivery ordinal in NEProgram.deliveries */
    uint32_t operand;  /* piston ID for SAMPLE, reset node ID for ARM_RESET */
    uint8_t kind;
    uint8_t reserved[3];
} NEInstantStep;

typedef struct {
    uint32_t electrical;
    uint32_t registry_type;
    uint32_t weak_faces;
    uint32_t strong_faces;
    uint8_t solid;
    uint8_t transparent;
    uint8_t cube;
    uint8_t reserved;
    uint32_t flags;
    uint32_t wire_connect_faces;
    /* Constant/contact wire payload: power[3:0], N[5:4], S[7:6],
       E[9:8], W[11:10], each side using NE_WIRE_SIDE_* values. */
    uint32_t wire_bits;
} NEProfile;

typedef struct {
    uint32_t source_kind;
    uint32_t source_id;
    uint8_t source_strength;
    uint8_t reserved[3];
    NEPosition position;
} NESite;

typedef struct {
    uint32_t guard;
    uint32_t profile;
    uint32_t receiver_id;
    uint8_t receiver_kind;
    uint8_t reserved[3];
} NEContactVariant;

typedef struct {
    uint32_t node;
    uint32_t first_variant;
    uint32_t variant_count;
    NEPosition position;
} NEContact;

typedef struct {
    uint32_t from_site;
    uint32_t guard;
    uint32_t target_id;
    uint8_t from_port;
    uint8_t face;
    uint8_t attenuation;
    uint8_t target_kind;
    uint8_t target_port;
    uint8_t include_dust;
    uint8_t reserved[2];
} NEPowerLink;

typedef struct {
    uint32_t node;
    uint32_t neighbors[24];
    /* Optional 25-entry monotone offsets into turbo_route's delivery slice.
       All NE_NONE means use the validated linear fallback. */
    uint32_t turbo_offsets[25];
    uint32_t shape_route;
    uint32_t turbo_route;
    uint32_t query_known, query_solid, query_transparent, query_wire, query_zero_no_dust;
    uint32_t seed_direct, seed_conditional, turbo_direct, turbo_conditional;
    uint32_t dust_path, dust_path_position;
    uint32_t instant_partner, instant_piston, instant_resource, instant_observer;
    uint32_t flags;
} NEWire;

typedef struct {
    uint32_t source;
    uint32_t first_wire, wire_count;
    uint32_t first_input, input_count;
    uint32_t first_entry, entry_count;
} NEClassicNet;

typedef struct {
    uint32_t node, net;
    uint8_t attenuation;
    uint8_t reserved[3];
} NEClassicWire;

typedef struct {
    uint32_t receiver, net;
    uint8_t attenuation, source_attenuation;
    uint8_t reserved[2];
} NEClassicInput;

typedef struct {
    uint32_t net, first_op, op_count;
    uint8_t commit;
    uint8_t reserved[3];
} NEClassicEntry;

typedef struct {
    uint32_t receiver;
    uint8_t nonzero;
    uint8_t reserved[3];
    uint64_t when[4];
} NEClassicOp;

typedef struct {
    uint32_t ordinal, entry;
} NEClassicRouteEntry;

typedef struct {
    uint32_t net;
    uint8_t attenuation;
    uint8_t reserved[3];
} NEClassicWireBinding;

typedef struct {
    uint32_t net;
    uint8_t attenuation, source_attenuation;
    uint8_t reserved[2];
} NEClassicInputBinding;

typedef struct {
    uint32_t input_node;
    uint32_t input_site;
    uint32_t first_wire;
    uint32_t wire_count;
    uint8_t input_face;
    uint8_t reserved[3];
} NEDustPath;

typedef struct {
    uint32_t node;
    uint32_t rear_site;
    uint32_t side_port;
    uint32_t far_site;
    uint32_t direct_override_node;
    uint32_t far_override_node;
    uint32_t far_override_guard;
    uint8_t mode;
    uint8_t far_threshold;
    uint8_t direct_override_strength;
    uint8_t far_override_strength;
    uint8_t direct_override_live;
    uint8_t has_direct_override;
    uint8_t far_override_live;
    uint8_t has_far_override;
    uint8_t reserved[2];
} NEComparator;

typedef struct {
    uint32_t node;
    uint32_t base_contact;
    uint32_t near_contact;
    uint32_t far_contact;
    uint32_t lane_kind;
    uint32_t lane_far_contact;
    uint32_t facing;
    uint32_t flags;
    uint32_t routes[8];
    NEPosition position;
} NEPiston;

typedef struct {
    uint32_t material_profile;
    uint32_t first_contact;
    uint32_t contact_count;
} NEResource;

typedef struct {
    uint32_t site;
    uint32_t port;
    uint32_t target_id;
    uint32_t target_kind;
} NEPowerTarget;

typedef struct {
    uint32_t node;
    uint32_t delay;
    uint32_t priority;
    uint32_t registry_type;
    uint32_t current_registry_type;
    uint32_t block_type;
    uint8_t has_block_type;
    uint8_t reserved[3];
    NEPosition position;
} NEInitialTick;

typedef struct {
    void *context;
    void *(*allocate)(void *context, size_t size, size_t align);
    void (*deallocate)(void *context, void *ptr, size_t size, size_t align);
} NEAllocator;

typedef struct {
    uint32_t node_count, site_count, route_count, delivery_count, instant_step_count;
    uint32_t guard_count, guard_child_count, profile_count;
    uint32_t contact_count, contact_variant_count, power_link_count;
    uint32_t wire_count, wire_query_fact_count, wire_transfer_count, wire_isolated_count;
    uint32_t wire_instant_reset_count, instant_route_count, dust_path_count, dust_path_wire_count;
    uint32_t comparator_count, piston_count, resource_count;
    uint32_t resource_contact_count;
    uint32_t classic_net_count, classic_wire_count, classic_input_count;
    uint32_t classic_entry_count, classic_op_count, classic_route_entry_count;
    uint32_t resources_at_contact_count;
    uint32_t base_pistons_at_contact_count;
    uint32_t head_pistons_at_contact_count;
    uint32_t sound_button_off;
    uint32_t sound_bulb_on;
    uint32_t sound_bulb_off;
    const NENode *nodes;
    const NERoute *routes;
    const NEDelivery *deliveries;
    const NEInstantStep *instant_steps;
    const NEGuard *guards;
    const uint32_t *guard_children;
    const NEProfile *profiles;
    const NESite *sites;
    const NEContact *contacts;
    const NEContactVariant *contact_variants;
    const NEPowerLink *power_links;
    const NERange *input_power_ranges;      /* node_count * 3: Main, Side, Quasi */
    const NERange *conductor_power_ranges;  /* site_count * 2: no-dust, with-dust */
    const uint32_t *component_sites;       /* node_count; NE_NONE when no site represents node */
    const NEWire *wires;
    const NEDustPath *dust_paths;
    const uint32_t *dust_path_wires; /* wire-table IDs, ordered ingress-to-free-end */
    const NEComparator *comparators;        /* comparator_count, indexed by node->kind_data[0] */
    const NEPiston *pistons;
    const NEResource *resources;
    const uint32_t *resource_for_piston; /* piston_count; NE_NONE for empty lanes */
    const uint32_t *resource_contacts;
    const uint32_t *resources_at_contact_offsets; /* contact_count + 1 offsets */
    const uint32_t *resources_at_contact;
    const uint32_t *base_pistons_at_contact_offsets;
    const uint32_t *base_pistons_at_contact;
    const uint32_t *head_pistons_at_contact_offsets;
    const uint32_t *head_pistons_at_contact;
    const NEClassicNet *classic_nets;
    const NEClassicWire *classic_wires;
    const NEClassicInput *classic_inputs;
    const NEClassicEntry *classic_entries;
    const NEClassicOp *classic_ops;
    const NERange *classic_route_ranges; /* route_count */
    const NEClassicRouteEntry *classic_route_entries;
    const NEClassicWireBinding *classic_wire_by_node; /* node_count */
    const NEClassicInputBinding *classic_input_by_node; /* node_count */
} NEProgram;

typedef struct {
    uint32_t node;
    uint32_t priority;
    uint32_t delay;
    uint32_t registry_type;
    uint32_t current_registry_type;
    uint32_t block_type;
    uint8_t has_block_type;
    uint8_t reserved[3];
    NEPosition position;
} NEFutureTick;

typedef struct {
    uint32_t piston;
    uint32_t action;
} NEFuturePistonRequest;

typedef struct {
    uint32_t piston;
    uint32_t resource;
    uint32_t contact;
    uint64_t revision;
    uint64_t resource_revision;
    uint64_t due_tick;
    uint8_t has_resource;
    uint8_t extending;
    uint8_t reserved[6];
} NEFutureDeferredPiston;

typedef enum {
    NE_EFFECT_PISTON = 0,
    NE_EFFECT_SOUND = 1,
    NE_EFFECT_COMMAND_ENTITY = 2
} NEEffectKind;

typedef struct {
    uint32_t kind;
    NEPosition position;
    uint32_t action;
    uint32_t sound_id;
    uint32_t sound_category;
    uint8_t facing;
    uint8_t sticky;
    uint8_t extended;
    uint8_t reserved;
    float volume;
    float pitch;
} NEEffect;

typedef struct {
    uint64_t logical_tick;
    uint32_t scheduler_position;
    uint32_t tick_count;
    uint32_t request_count;
    uint32_t deferred_count;
    NEFutureTick *ticks;
    uint32_t tick_capacity;
    NEFuturePistonRequest *requests;
    uint32_t request_capacity;
    NEFutureDeferredPiston *deferred;
    uint32_t deferred_capacity;
} NEFutureSnapshot;

typedef struct {
    uint64_t ticks;
    uint64_t scheduled_ticks;
    uint64_t callbacks;
    uint64_t wire_walks;
    uint64_t piston_samples;
    uint64_t piston_actions;
    uint64_t deferred_publications;
    uint64_t wire_query_fact_evaluations;
    uint64_t wire_dust_path_evaluations;
    uint64_t wire_isolated_replays;
    uint64_t wire_instant_reset_replays;
    uint64_t instant_route_invocations;
    uint64_t instant_route_steps;
    uint64_t classic_net_invocations;
    uint64_t classic_receiver_ops;
    uint64_t compiled_power_queries;
    uint64_t compiled_guard_evaluations;
} NECounters;

typedef struct NEEngine NEEngine;

NEEngine *ne_create(const NEProgram *program, NEStateView state, NEAllocator allocator,
                    uint64_t logical_tick, uint32_t scheduler_position,
                    const NEInitialTick *initial_ticks, uint32_t initial_tick_count);
void ne_destroy(NEEngine *engine);
NEStatus ne_use_block(NEEngine *engine, NEStateView state, uint32_t node);
NEStatus ne_set_pressure_plate(NEEngine *engine, NEStateView state, uint32_t node, uint8_t powered);
NEStatus ne_tick(NEEngine *engine, NEStateView state);
NEStatus ne_snapshot_future(const NEEngine *engine, NEFutureSnapshot *snapshot);
const NECounters *ne_counters(const NEEngine *engine);
uint32_t ne_error_node(const NEEngine *engine);
void ne_verify_compiled_power(NEEngine *engine, uint8_t enabled);
/* Copies and clears the native dirty bitmap. Return value is number of set bits. */
uint32_t ne_copy_dirty(NEEngine *engine, uint8_t *destination, uint32_t capacity);
/* Copies presentation effects in order and clears them; capacity failure leaves them queued. */
uint32_t ne_copy_effects(NEEngine *engine, NEEffect *destination, uint32_t capacity);
/* Number of queued presentation effects; does not drain the queue. */
uint32_t ne_pending_effect_count(const NEEngine *engine);

#ifdef __cplusplus
}
#endif

#endif
