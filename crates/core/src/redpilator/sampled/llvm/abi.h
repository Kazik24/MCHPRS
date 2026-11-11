#ifndef MCHPRS_SAMPLED_NATIVE_ABI_H
#define MCHPRS_SAMPLED_NATIVE_ABI_H

#include <stddef.h>
#include <stdint.h>

typedef struct {
    uint32_t abi_version;
    uint32_t descriptor_bytes;
    uint64_t semantic_version;
} MchprsNativeApiDescriptor;

typedef struct {
    uint32_t abi_version;
    uint32_t header_bytes;
    uint32_t total_bytes;
    uint32_t operation_offset;
    uint32_t operation_count;
    uint32_t link_offset;
    uint32_t link_count;
    uint32_t component_offset;
    uint32_t component_count;
    uint32_t source_offset;
    uint32_t source_count;
    uint32_t arrangement_link_count;
    uint32_t watched_wire_count;
} MchprsNativeProgramDescriptor;

typedef struct {
    int32_t x;
    int32_t y;
    int32_t z;
    uint8_t kind;
    uint8_t initial_state;
    /* Repeater: delay in ticks, then fixed-lock/diode-output flag bits. Zero for other kinds. */
    uint8_t reserved[2];
    uint32_t on_offset;
    uint32_t on_count;
    uint32_t off_offset;
    uint32_t off_count;
} MchprsNativeComponent;

typedef struct {
    uint32_t component;
    uint32_t ticks_left;
    uint8_t priority;
    uint8_t reserved[3];
} MchprsNativePendingTick;

#if defined(_WIN32)
#define MCHPRS_NATIVE_EXPORT __declspec(dllexport)
#else
#define MCHPRS_NATIVE_EXPORT
#endif

MCHPRS_NATIVE_EXPORT MchprsNativeApiDescriptor ne_api_descriptor(void);
MCHPRS_NATIVE_EXPORT size_t ne_state_size(const MchprsNativeProgramDescriptor *program, const uint8_t *blob, uint32_t blob_bytes, uint32_t initial_ticks);
MCHPRS_NATIVE_EXPORT uint32_t ne_create(const MchprsNativeProgramDescriptor *program, const uint8_t *blob, uint32_t blob_bytes, void *memory, size_t memory_bytes, uint32_t initial_ticks);
MCHPRS_NATIVE_EXPORT uint32_t ne_import_tick(void *memory, uint32_t component, uint32_t delay, uint8_t priority);
MCHPRS_NATIVE_EXPORT void ne_destroy(void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_tick(void *memory);
/* Status 5 requests more command effect capacity before the next tick; completed excludes failures. */
typedef uint64_t (*MchprsNativeClock)(const void *context);
MCHPRS_NATIVE_EXPORT uint32_t ne_tick_batch(void *memory, uint32_t count, MchprsNativeClock clock,
                                           const void *clock_context, uint64_t logical_base, uint32_t *completed);
MCHPRS_NATIVE_EXPORT uint32_t ne_use_control(void *memory, uint32_t source_index);
MCHPRS_NATIVE_EXPORT uint32_t ne_set_input(void *memory, uint32_t source_index, uint8_t powered);
MCHPRS_NATIVE_EXPORT uint32_t ne_read_state(const void *memory, uint8_t *out, uint32_t count);
MCHPRS_NATIVE_EXPORT uint32_t ne_read_components(const void *memory, const uint32_t *ids, uint8_t *out, uint32_t count);
/* Packed visual state: state, wire sides, comparator output, note instrument (one byte each). */
MCHPRS_NATIVE_EXPORT uint32_t ne_read_visual_components(const void *memory, const uint32_t *ids, uint32_t *out, uint32_t count);
MCHPRS_NATIVE_EXPORT uint32_t ne_note_count(const void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_read_notes(const void *memory, uint32_t *out, uint32_t count);
MCHPRS_NATIVE_EXPORT uint32_t ne_pending_count(const void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_read_pending(const void *memory, MchprsNativePendingTick *out, uint32_t count);
MCHPRS_NATIVE_EXPORT uint64_t ne_work_count(const void *memory);
MCHPRS_NATIVE_EXPORT uint64_t ne_tick_count(const void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_snapshot_words(const void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_read_snapshot(const void *memory, uint32_t *out, uint32_t words);
MCHPRS_NATIVE_EXPORT void ne_ack_effects(void *memory);
MCHPRS_NATIVE_EXPORT uint32_t ne_bind_commands(void *memory, void *commands, const uint32_t *notify_rows, const uint32_t *notify_targets, uint32_t target_count);
MCHPRS_NATIVE_EXPORT void ne_set_command_clock(void *memory, uint64_t now_ns, uint64_t logical_base);
MCHPRS_NATIVE_EXPORT uint64_t ne_command_effect_bound(const void *memory);

#if defined(__cplusplus)
static_assert(sizeof(MchprsNativeApiDescriptor) == 16, "native API descriptor ABI drift");
static_assert(sizeof(MchprsNativeProgramDescriptor) == 52, "native Program descriptor ABI drift");
static_assert(sizeof(MchprsNativeComponent) == 32, "native component ABI drift");
static_assert(sizeof(MchprsNativePendingTick) == 12, "native pending tick ABI drift");
static_assert(offsetof(MchprsNativeApiDescriptor, semantic_version) == 8, "native API offset drift");
static_assert(offsetof(MchprsNativeProgramDescriptor, operation_offset) == 12, "native Program offset drift");
static_assert(offsetof(MchprsNativeProgramDescriptor, component_offset) == 28, "native Program offset drift");
static_assert(offsetof(MchprsNativeProgramDescriptor, source_count) == 40, "native Program offset drift");
static_assert(offsetof(MchprsNativeProgramDescriptor, watched_wire_count) == 48, "native Program offset drift");
#else
_Static_assert(sizeof(MchprsNativeApiDescriptor) == 16, "native API descriptor ABI drift");
_Static_assert(sizeof(MchprsNativeProgramDescriptor) == 52, "native Program descriptor ABI drift");
_Static_assert(sizeof(MchprsNativeComponent) == 32, "native component ABI drift");
_Static_assert(sizeof(MchprsNativePendingTick) == 12, "native pending tick ABI drift");
_Static_assert(offsetof(MchprsNativeApiDescriptor, semantic_version) == 8, "native API offset drift");
_Static_assert(offsetof(MchprsNativeProgramDescriptor, operation_offset) == 12, "native Program offset drift");
_Static_assert(offsetof(MchprsNativeProgramDescriptor, component_offset) == 28, "native Program offset drift");
_Static_assert(offsetof(MchprsNativeProgramDescriptor, source_count) == 40, "native Program offset drift");
_Static_assert(offsetof(MchprsNativeProgramDescriptor, watched_wire_count) == 48, "native Program offset drift");
#endif

#endif
