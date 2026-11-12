/* Libc-free input checks; compile beside a candidate's non-embedded engine.c. */
#include "abi.h"
#define NE_STATIC_WIRE_GRAPH 1
static const uint32_t ne_static_graph_rows[6] = {UINT32_MAX, UINT32_MAX, UINT32_MAX, 0, UINT32_MAX, UINT32_MAX};
static const uint32_t ne_static_graph_values[1] = {15u | (2u << 8u)};
static const uint32_t ne_static_graph_prefix[2] = {0, 0};
static const uint32_t ne_static_graph_flips[4] = {0};
#include "engine.c"

MCHPRS_NATIVE_EXPORT uint32_t input_checks(void) {
    static NEEngine e;
    static MchprsNativeComponent nodes[3] = {{.kind=2}, {.kind=5}, {.kind=9}};
    static MchprsNativeProgramDescriptor program = {.component_count=3};
    static uint8_t links_data[36], contributions[3], arranged[3];
    static uint32_t counts[3], side_counts[3], histograms[48], side_histograms[48];
    static uint32_t watched[3] = {NONE,NONE,NONE};
    static uint64_t updated[3];
    e.program=&program; e.component_data=nodes; e.link_data=links_data;
    e.input_count=counts; e.side_count=side_counts; e.histograms=histograms; e.side_histograms=side_histograms;
    e.contributions=contributions; e.arrangement_contributions=arranged;
    e.watched_by_component=watched; e.updated=updated;
    for (uint32_t ordinary=0; ordinary<2; ++ordinary)
    for (uint32_t target=0; target<3; ++target)
    for (uint32_t side=0; side<(ordinary ? 1u : 2u); ++side)
    for (uint8_t old=0; old<=15; ++old)
    for (uint8_t next=0; next<=15; ++next) {
        for (uint32_t i=0;i<3;++i) { counts[i]=side_counts[i]=0; contributions[i]=arranged[i]=0; }
        for (uint32_t i=0;i<48;++i) histograms[i]=side_histograms[i]=0;
        for (uint32_t i=0;i<3;++i) { links_data[i*12+4]=(uint8_t)target; links_data[i*12+9]=1; }
#define CHANGE(index, value) do { \
    if (ordinary) set_contribution(&e,index,value); \
    else set_arranged(&e,index,target,side,value,nodes[target].kind==9u || (!side && nodes[target].kind==5u)); \
} while(0)
        CHANGE(0,8); CHANGE(1,8); CHANGE(2,old); CHANGE(2,next);
        uint32_t active=(side ? side_counts : counts)[target];
        if (active!=2u+(next!=0u)) return 1;
        const uint32_t *hist=(side ? side_histograms : histograms)+target*16u;
        uint32_t analog=nodes[target].kind==9u || (!side && nodes[target].kind==5u);
        uint32_t mask=0;
        for (uint32_t level=1;level<16;++level) {
            uint32_t expected=analog ? 2u*(level==8u)+(level==next) : 0u;
            if (hist[level]!=expected) return 2;
            if (expected) mask|=1u<<level;
        }
        if (hist[0]!=mask) return 3;
        if (analog && histogram_strength(hist)!=(next>8u ? next : 8u)) return 4;
        CHANGE(0,0); CHANGE(1,0); CHANGE(2,0);
        if ((side ? side_counts : counts)[target]) return 5;
        for (uint32_t level=0;level<16;++level) if(hist[level]) return 6;
#undef CHANGE
    }
    return 0;
}

MCHPRS_NATIVE_EXPORT uint32_t graph_checks(void) {
    static NEEngine e;
    static MchprsNativeComponent nodes[3] = {{.kind=3}, {.kind=5}, {.kind=9}};
    static MchprsNativeProgramDescriptor program = {.component_count=3, .link_count=2, .arrangement_link_count=1};
    static uint8_t links_data[24] = {0,0,0,0,1,0,0,0,7,1,0,0, 0,0,0,0,1,0,0,0,0,0,0,0};
    static uint8_t contributions[2] = {8,15}, arranged[1] = {0};
    static uint32_t counts[3] = {0,99,0}, histograms[48], groups[1] = {0};
    static uint32_t arrangement[8] = {1,0,0,0,0,0,0,1}, active[1] = {1}, hidden[3] = {0};
    e.program=&program; e.component_data=nodes; e.link_data=links_data;
    e.contributions=contributions; e.arrangement_contributions=arranged;
    e.input_count=counts; e.histograms=histograms; e.groups=groups;
    e.table_data[25]=arrangement; e.table_data[26]=active; e.table_data[30]=hidden;
    e.graph_active=1;
    if (!graph_node(&e,1) || graph_value(&e,1)!=(15u | (2u << 8u))) return 1;
    if (graph_contribution(&e,0)!=15u) return 2;
    graph_materialize(&e);
    if (e.graph_active || counts[1]!=2u || arranged[0]!=15u) return 3;
    if (histograms[16u] != ((1u<<8u) | (1u<<15u)) || histograms[24u]!=1u || histograms[31u]!=1u) return 4;
    graph_materialize(&e);
    if (counts[1]!=2u || graph_node(&e,1)) return 5;
    return 0;
}

