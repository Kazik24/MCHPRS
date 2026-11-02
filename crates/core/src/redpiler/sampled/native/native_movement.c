/* Included after the electrical executor so movement uses the same callbacks and accounting. */
static uint32_t bounded(uint32_t start, uint32_t count, uint32_t limit) { return (uint64_t)start + count <= limit; }
static uint32_t valid_source(uint32_t source, uint32_t nodes, uint32_t tokens) {
    return (source & 0x80000000u) ? (source & 0x7fffffffu) < tokens : source < nodes;
}
static uint32_t valid_movement(const MchprsNativeProgramDescriptor *p, const uint8_t *blob, const uint32_t **t, const uint32_t *n) {
    if (n[7] % 10u || n[11] % 3u || n[16] % 7u || n[19] % 6u || n[20] % 5u || n[21] % 2u
        || n[22] % 6u || n[23] % 5u || n[25] % 8u) return 0u;
    uint32_t pistons = n[7] / 10u, tokens = n[11] / 3u, cells = n[16] / 7u, nodes = p->component_count;
    if ((uint64_t)n[8] != (uint64_t)pistons * 2u || n[9] != n[8] || n[31] != n[8]
        || (uint64_t)n[12] != (uint64_t)tokens * 2u || n[14] != n[12] || n[18] != n[12]
        || n[28] != n[0] || n[30] != nodes || n[32] != nodes || n[25] / 8u != p->arrangement_link_count) return 0u;
    for (uint32_t i = 0; i < pistons; ++i) {
        const uint32_t *r = t[7] + i * 10u;
        if (r[0] >= nodes || blob[p->component_offset + (size_t)r[0] * 32u + 12u] != 8u
            || r[1] >= cells || r[2] >= cells || (r[3] != NONE && r[3] >= pistons)
            || r[4] > 1u || r[6] > 1u || r[7] > 1u || r[8] > 1u || r[9] > 1u
            || !bounded(t[8][i*2u], t[8][i*2u+1u], n[10]) || !bounded(t[9][i*2u], t[9][i*2u+1u], n[10])
            || !bounded(t[31][i*2u], t[31][i*2u+1u], n[4])) return 0u;
    }
    for (uint32_t i = 0; i < n[10]; ++i) if (t[10][i] >= pistons) return 0u;
    for (uint32_t i = 0; i < tokens; ++i) {
        if ((t[11][i*3u] != NONE && t[11][i*3u] >= cells) || t[11][i*3u+1u] > 15u
            || (t[11][i*3u+2u] != NONE && t[11][i*3u+2u] >= cells)
            || !bounded(t[12][i*2u], t[12][i*2u+1u], n[13]) || !bounded(t[14][i*2u], t[14][i*2u+1u], n[15])
            || !bounded(t[18][i*2u], t[18][i*2u+1u], n[19]/6u) || t[18][i*2u+1u] != t[12][i*2u+1u]) return 0u;
    }
    for (uint32_t i = 0; i < n[13]; ++i) if (t[13][i] >= cells) return 0u;
    for (uint32_t i = 0; i < n[15]; ++i) if (t[15][i] >= nodes) return 0u;
    for (uint32_t i = 0; i < cells; ++i) {
        const uint32_t *r = t[16] + i * 7u;
        if (r[0] != NONE && r[0] >= nodes) return 0u;
        for (uint32_t k = 1; k < 7u; k += 2u) {
            if (!bounded(r[k],r[k+1u],n[17])) return 0u;
            uint32_t limit = k == 1u ? tokens : k == 3u ? pistons : nodes;
            for (uint32_t j = 0; j < r[k+1u]; ++j) if (t[17][r[k]+j] >= limit) return 0u;
        }
    }
    for (uint32_t i = 0; i < n[19]; i += 6u) {
        if (!bounded(t[19][i],t[19][i+1u],n[20]/5u) || !bounded(t[19][i+2u],t[19][i+3u],n[23]/5u)
            || !bounded(t[19][i+4u],t[19][i+5u],n[23]/5u)) return 0u;
    }
    for (uint32_t i = 0; i < n[20]; i += 5u) {
        const uint32_t *r = t[20] + i;
        if (r[0] >= n[27] || !r[4] || r[1] >= r[4] || t[27][r[0]] >= r[4]
            || !bounded(r[2],r[4],n[21]/2u) || !bounded(r[3],r[4],n[21]/2u)) return 0u;
    }
    for (uint32_t i = 0; i < n[21]; i += 2u) if (!bounded(t[21][i],t[21][i+1u],n[22]/6u)) return 0u;
    for (uint32_t i = 0; i < n[22]; i += 6u) {
        const uint32_t *r = t[22] + i;
        if (r[0] >= nodes || !valid_source(r[1],nodes,tokens) || r[2] > 15u || r[3] >= p->arrangement_link_count || r[4] > 1u || r[5] > 1u) return 0u;
    }
    for (uint32_t i = 0; i < n[23]; i += 5u) {
        const uint32_t *r = t[23] + i;
        if (r[0] >= nodes || r[1] > 1u || r[2] > 1u || !bounded(r[3],r[4],n[24])) return 0u;
    }
    for (uint32_t i = 0; i < n[24]; ++i) if (t[24][i] >= nodes) return 0u;
    for (uint32_t i = 0; i < n[25]; i += 8u) {
        const uint32_t *r = t[25] + i;
        if (r[0] >= nodes || r[1] > 1u || r[2] > 1u || !valid_source(r[3],nodes,tokens) || r[4] > 15u
            || r[5] >= n[27] || !bounded(r[6],r[7],n[26]) || t[27][r[5]] >= r[7]) return 0u;
    }
    for (uint32_t i = 0; i < n[26]; ++i) if (t[26][i] > 1u) return 0u;
    for (uint32_t node = 0; node < nodes; ++node) {
        if (!bounded(t[28][node*2u],t[28][node*2u+1u],n[29]) || t[30][node] > 1u || t[32][node] >= cells) return 0u;
    }
    for (uint32_t i = 0; i < n[29]; ++i) if (t[29][i] >= p->arrangement_link_count) return 0u;
    if (!n[33]) {
        for (uint32_t i=34u;i<42u;++i) if (n[i]) return 0u;
        return 1u;
    }
    if ((n[33]-1u)%4u || n[34]%2u || n[35]%4u || n[36]%2u || n[37]%2u || n[38]%3u
        || n[40]!=(uint64_t)tokens*2u) return 0u;
    uint32_t locals=(n[33]-1u)/4u, wires=t[33][0], readers=n[38]/3u;
    if (wires>locals || locals-wires!=readers) return 0u;
    for (uint32_t i=0;i<locals;++i) {
        const uint32_t *r=t[33]+1u+i*4u;
        if (!bounded(r[0],r[1],n[34]/2u) || !bounded(r[2],r[3],n[35]/4u)) return 0u;
        uint64_t states=1u;
        for (uint32_t j=0;j<r[1];++j) {
            const uint32_t *a=t[34]+(r[0]+j)*2u;
            if (a[0]>=tokens || (j && a[0]<=a[-2]) || a[1]!=t[12][a[0]*2u+1u]+1u || states>4096u/a[1]) return 0u;
            states*=a[1];
        }
        if (states!=r[3]) return 0u;
    }
    for (uint32_t i=0;i<n[35];i+=4u) {
        const uint32_t *r=t[35]+i;
        if (r[0]==NONE) { if (r[1] || r[2] || r[3]) return 0u; }
        else if (!bounded(r[0],r[1],n[36]/2u) || !bounded(r[2],r[3],n[37]/2u)) return 0u;
    }
    for (uint32_t i=0;i<n[36];i+=2u) if (t[36][i]>=wires || t[36][i+1u]>1u) return 0u;
    for (uint32_t i=0;i<n[37];i+=2u) if (!valid_source(t[37][i],nodes,tokens) || t[37][i+1u]>1u) return 0u;
    for (uint32_t i=0;i<readers;++i) {
        const uint32_t *r=t[38]+i*3u;
        if (r[0]!=wires+i || !bounded(r[1],r[2],n[39])) return 0u;
        for (uint32_t j=0;j<r[2];++j) {
            uint32_t link=t[39][r[1]+j];
            if (link>=p->arrangement_link_count || (j && link<=t[39][r[1]+j-1u])) return 0u;
            if (j && t[25][link*8u]!=t[25][t[39][r[1]]*8u]) return 0u;
        }
    }
    for (uint32_t i=0;i<n[39];++i) if (t[39][i]>=p->arrangement_link_count) return 0u;
    for (uint32_t i=0;i<tokens;++i) {
        uint32_t start=t[40][i*2u], count=t[40][i*2u+1u];
        if (!bounded(start,count,n[41])) return 0u;
        for (uint32_t j=1u;j<count;++j) if (t[41][start+j]<=t[41][start+j-1u]) return 0u;
    }
    for (uint32_t i=0;i<n[41];++i) if (t[41][i]>=readers) return 0u;
    return 1u;
}

static size_t movement_storage_bytes(const NEEngine *e) {
    size_t n=e->program->component_count, p=table_words(e,7u)/10u, t=table_words(e,11u)/3u, c=table_words(e,16u)/7u;
    size_t m=p*2u+t;
    size_t wires=table_words(e,33u)?table(e,33u)[0]:0u;
    return 320u + (t+table_words(e,27u)+n+p+c+n+n+wires)*4u + p*2u + e->program->arrangement_link_count*3u + n*2u + wires
        + p*3u*2u*4u + m*sizeof(NEMotion);
}

static uint8_t *take_bytes(uint8_t **at, size_t count) {
    uint8_t *p=*at; *at += (count+7u)&~(size_t)7u; return p;
}

static uint8_t arrangement_output(const NEEngine *e, uint32_t source) {
    return source & 0x80000000u ? (uint8_t)table(e,11u)[(source&0x7fffffffu)*3u+1u] : output(e,source);
}
static void set_arranged(NEEngine *e, uint32_t index, uint32_t target, uint32_t side, uint8_t value) {
    uint8_t old=e->arrangement_contributions[index]; e->arrangement_contributions[index]=value;
    if (old == value) return;
    if (side) {
        if (old) { e->side_count[target]--; e->side_histograms[(size_t)target*16u+old]--; }
        if (value) { e->side_count[target]++; e->side_histograms[(size_t)target*16u+value]++; }
    }
    else {
        if (old) { e->input_count[target]--; e->histograms[(size_t)target*16u+old]--; }
        if (value) { e->input_count[target]++; e->histograms[(size_t)target*16u+value]++; }
    }
}
static void refresh_arranged(NEEngine *e, uint32_t index) {
    const uint32_t *r=table(e,25u)+index*8u;
    uint8_t value=0u, old=e->arrangement_contributions[index];
    if (table(e,26u)[r[6]+e->groups[r[5]]]) {
        uint32_t attenuation=e->route_attenuation[index]==254u?r[4]:e->route_attenuation[index];
        uint8_t strength=arrangement_output(e,r[3]); value=strength>attenuation?(uint8_t)(strength-attenuation):0u;
    }
    set_arranged(e,index,r[0],r[1],value);
    if ((old!=0u)!=(value!=0u) || (old!=value && (components(e)[r[0]].kind==9u || e->watched_by_component[r[0]]!=NONE))) e->updated[r[0]]=0u;
}

static uint32_t consume(NEEngine *e);
static const uint32_t *route_ports(NEEngine *e, uint32_t local) {
    const uint32_t *r=table(e,33u)+1u+local*4u;
    uint32_t state=0u, stride=1u;
    for (uint32_t i=0;i<r[1];++i) {
        const uint32_t *a=table(e,34u)+(r[0]+i)*2u;
        uint32_t token=a[0], digit=0u, cell=e->token_cell[token];
        if (cell!=NONE) {
            const uint32_t *row=table(e,12u)+token*2u;
            for (uint32_t j=0;j<row[1];++j) if (table(e,13u)[row[0]+j]==cell) { digit=j+1u; break; }
            if (!digit) { e->error=NE_INVALID; return NULL; }
        }
        state+=digit*stride; stride*=a[1];
    }
    const uint32_t *ports=table(e,35u)+(r[2]+state)*4u;
    if (ports[0]==NONE) { e->error=NE_INVALID; return NULL; }
    return ports;
}

static void route_source(NEEngine *e, const uint32_t *reader, uint32_t source, uint32_t side, uint8_t distance) {
    for (uint32_t i=0;i<reader[2];++i) {
        uint32_t link=table(e,39u)[reader[1]+i];
        const uint32_t *r=table(e,25u)+link*8u;
        if (r[3]==source && r[1]==side && distance<e->route_next[link]) e->route_next[link]=distance;
    }
}

static uint32_t route_reader(NEEngine *e, uint32_t reader_id, uint32_t account) {
    const uint32_t *reader=table(e,38u)+reader_id*3u;
    for (uint32_t i=0;i<reader[2];++i) e->route_next[table(e,39u)[reader[1]+i]]=255u;
    const uint32_t *ports=route_ports(e,reader[0]); if (!ports) return e->error;
    for (uint32_t i=0;i<ports[3];++i) {
        const uint32_t *source=table(e,37u)+(ports[2]+i)*2u;
        route_source(e,reader,source[0],source[1],0u);
    }
    uint32_t wires=table(e,33u)[0];
    for (uint32_t i=0;i<ports[1];++i) {
        const uint32_t *root=table(e,36u)+(ports[0]+i)*2u;
        for (uint32_t j=0;j<wires;++j) e->route_distance[j]=255u;
        uint32_t first=0u,last=0u; e->route_queue[last++]=root[0]; e->route_distance[root[0]]=0u;
        while (first<last) {
            uint32_t wire=e->route_queue[first++]; uint8_t distance=e->route_distance[wire];
            if (account && consume(e)) return e->error;
            const uint32_t *node=route_ports(e,wire); if (!node) return e->error;
            for (uint32_t j=0;j<node[3];++j) {
                const uint32_t *source=table(e,37u)+(node[2]+j)*2u;
                route_source(e,reader,source[0],root[1],distance);
            }
            if (distance==14u) continue;
            for (uint32_t j=0;j<node[1];++j) {
                uint32_t next=table(e,36u)[(node[0]+j)*2u];
                if (e->route_distance[next]==255u) { e->route_distance[next]=(uint8_t)(distance+1u); e->route_queue[last++]=next; }
            }
        }
    }
    return NE_OK;
}

static uint32_t route_refresh(NEEngine *e, uint32_t token, uint32_t *switched_count) {
    if (!table_words(e,33u)) return NE_OK;
    const uint32_t *row=table(e,40u)+token*2u;
    for (uint32_t i=0;i<row[1];++i) if (route_reader(e,table(e,41u)[row[0]+i],1u)) return e->error;
    for (uint32_t i=0;i<row[1];++i) {
        const uint32_t *reader=table(e,38u)+table(e,41u)[row[0]+i]*3u;
        for (uint32_t j=0;j<reader[2];++j) {
            uint32_t index=table(e,39u)[reader[1]+j];
            const uint32_t *link=table(e,25u)+index*8u;
            uint8_t old=e->route_attenuation[index], next=e->route_next[index], strength=arrangement_output(e,link[3]);
            if (old==next) continue;
            if (old!=255u) { if (consume(e)) return e->error; e->route_attenuation[index]=255u; set_arranged(e,index,link[0],link[1],0u); }
            if (next!=255u) { if (consume(e)) return e->error; e->route_attenuation[index]=next; set_arranged(e,index,link[0],link[1],strength>next?(uint8_t)(strength-next):0u); }
            if (strength>old || strength>next) e->updated[link[0]]=0u;
            if (components(e)[link[0]].kind!=8u && !e->switched[link[0]]) { e->switched[link[0]]=1u; e->switched_nodes[(*switched_count)++]=link[0]; }
        }
    }
    return NE_OK;
}

static uint32_t init_movement(NEEngine *e, uint8_t *base, size_t bytes) {
    if (bytes < movement_storage_bytes(e)) return NE_CAPACITY;
    for (size_t i=0;i<movement_storage_bytes(e);++i) base[i]=0u;
    uint8_t *at=base; uint32_t n=e->program->component_count;
    e->piston_count=table_words(e,7u)/10u; e->token_count=table_words(e,11u)/3u; e->cell_count=table_words(e,16u)/7u;
    e->motion_capacity=e->piston_count*2u+e->token_count;
    e->token_cell=(uint32_t *)take_bytes(&at,(size_t)e->token_count*4u);
    e->groups=(uint32_t *)take_bytes(&at,(size_t)table_words(e,27u)*4u);
    e->piston_by_component=(uint32_t *)take_bytes(&at,(size_t)n*4u);
    e->piston_motion=(uint32_t *)take_bytes(&at,(size_t)e->piston_count*4u);
    e->cell_motion=(uint32_t *)take_bytes(&at,(size_t)e->cell_count*4u);
    e->piston_flags=take_bytes(&at,e->piston_count); e->queued_pistons=take_bytes(&at,e->piston_count);
    e->arrangement_contributions=take_bytes(&at,e->program->arrangement_link_count);
    e->route_attenuation=take_bytes(&at,e->program->arrangement_link_count);
    e->route_next=take_bytes(&at,e->program->arrangement_link_count);
    uint32_t wires=table_words(e,33u)?table(e,33u)[0]:0u;
    e->route_distance=take_bytes(&at,wires);
    e->route_queue=(uint32_t *)take_bytes(&at,(size_t)wires*4u);
    e->switched=take_bytes(&at,n); e->changed=take_bytes(&at,n);
    e->switched_nodes=(uint32_t *)take_bytes(&at,(size_t)n*4u); e->changed_nodes=(uint32_t *)take_bytes(&at,(size_t)n*4u);
    e->piston_event_capacity=e->piston_count*3u;
    e->piston_events=(uint32_t *)take_bytes(&at,(size_t)e->piston_event_capacity*8u);
    e->motions=(NEMotion *)take_bytes(&at,(size_t)e->motion_capacity*sizeof(NEMotion));
    if ((size_t)(at-base)>bytes) return NE_CAPACITY;
    e->motion_first=e->motion_last=e->landing_cursor=NONE; e->motion_free=e->motion_capacity?0u:NONE;
    e->motion_phase=0u; e->piston_event_first=e->piston_event_count=0u;
    for (uint32_t i=0;i<n;++i) e->piston_by_component[i]=NONE;
    for (uint32_t i=0;i<e->piston_count;++i) {
        const uint32_t *r=table(e,7u)+i*10u;
        if (e->piston_by_component[r[0]]!=NONE) return NE_INVALID;
        e->piston_by_component[r[0]]=i; e->piston_motion[i]=NONE;
        e->piston_flags[i]=(uint8_t)(r[6]|(r[7]<<1u));
    }
    for (uint32_t i=0;i<e->cell_count;++i) e->cell_motion[i]=NONE;
    for (uint32_t i=0;i<e->token_count;++i) e->token_cell[i]=table(e,11u)[i*3u];
    for (uint32_t i=0;i<table_words(e,27u);++i) e->groups[i]=table(e,27u)[i];
    for (uint32_t i=0;i<e->motion_capacity;++i) e->motions[i].next=i+1u<e->motion_capacity?i+1u:NONE;
    for (uint32_t i=0;i<e->program->arrangement_link_count;++i) e->route_attenuation[i]=e->route_next[i]=254u;
    if (table_words(e,33u)) {
        for (uint32_t i=0;i<table_words(e,38u)/3u;++i) if (route_reader(e,i,0u)) return e->error;
        for (uint32_t i=0;i<e->program->arrangement_link_count;++i) e->route_attenuation[i]=e->route_next[i];
    }
    for (uint32_t i=0;i<e->program->arrangement_link_count;++i) refresh_arranged(e,i);
    return NE_OK;
}

static uint32_t consume(NEEngine *e) { e->work_count++; return ++e->tick_operations>NE_MAX_TICK_OPERATIONS?(e->error=NE_OPERATION_LIMIT):NE_OK; }
static uint32_t token_at(const NEEngine *e, uint32_t cell) {
    const uint32_t *r=table(e,16u)+cell*7u;
    for (uint32_t i=0;i<r[2];++i) { uint32_t t=table(e,17u)[r[1]+i]; if (e->token_cell[t]==cell) return t; }
    return NONE;
}
static uint32_t notify_shape(NEEngine *e,uint32_t cell) {
    const uint32_t *r=table(e,16u)+cell*7u;
    for (uint32_t i=0;i<r[6];++i) {
        uint32_t node=table(e,17u)[r[5]+i];
        if (!e->component_state[node]) { if (consume(e)) return e->error; if (schedule(e,node,2u,3u)) return e->error; }
    } return NE_OK;
}
static uint32_t notify_home(NEEngine *e,uint32_t piston) {
    const uint32_t *r=table(e,31u)+piston*2u;
    for (uint32_t i=0;i<r[1];++i) { if (consume(e)) return e->error; uint32_t result=update_once(e,table(e,4u)[r[0]+i]); if (result) return result; }
    return NE_OK;
}

static uint32_t mark_wire(NEEngine *e,uint32_t node) {
    uint32_t index=e->watched_by_component[node]; if (index==NONE) return 0u;
    uint8_t strength=input_strength(e,node); refresh_wire(e,node);
    if (strength==e->watched_strengths[index]) return 0u;
    e->watched_strengths[index]=strength; return 1u;
}
static uint32_t set_token_pos(NEEngine *e,uint32_t token,uint32_t position) {
    uint32_t old=e->token_cell[token]; if (old==position) return NE_OK;
    if (old!=NONE && position!=NONE) return e->error=NE_INVALID;
    uint32_t cell=position!=NONE?position:old, local=NONE;
    const uint32_t *cells=table(e,12u)+token*2u;
    for (uint32_t i=0;i<cells[1];++i) if (table(e,13u)[cells[0]+i]==cell) { local=i; break; }
    if (local==NONE) return e->error=NE_INVALID;
    e->token_cell[token]=position;
    uint32_t switched_count=0u,changed_count=0u;
    if (route_refresh(e,token,&switched_count)) return e->error;
    const uint32_t *affected=table(e,14u)+token*2u;
    for (uint32_t i=0;i<affected[1];++i) e->updated[table(e,15u)[affected[0]+i]]=0u;
    const uint32_t *row=table(e,19u)+(table(e,18u)[token*2u]+local)*6u;
    for (uint32_t i=0;i<row[1];++i) {
        const uint32_t *r=table(e,20u)+(row[0]+i)*5u;
        uint32_t before=e->groups[r[0]],after;
        if (position!=NONE) { if (before>UINT32_MAX-r[1]) return e->error=NE_INVALID; after=before+r[1]; }
        else { if (before<r[1]) return e->error=NE_INVALID; after=before-r[1]; }
        if (before>=r[4] || after>=r[4]) return e->error=NE_INVALID;
        uint32_t state=position!=NONE?after:before, base=position!=NONE?r[3]:r[2];
        const uint32_t *flips=table(e,21u)+(base+state)*2u; e->groups[r[0]]=after;
        for (uint32_t j=0;j<flips[1];++j) {
            const uint32_t *flip=table(e,22u)+(flips[0]+j)*6u;
            if (consume(e)) return e->error;
            uint8_t strength=arrangement_output(e,flip[1]);
            set_arranged(e,flip[3],flip[0],flip[5],flip[4] && strength>flip[2]?(uint8_t)(strength-flip[2]):0u);
            if (strength>flip[2]) e->updated[flip[0]]=0u;
            if (components(e)[flip[0]].kind!=8u && !e->switched[flip[0]]) { e->switched[flip[0]]=1u; e->switched_nodes[switched_count++]=flip[0]; }
        }
    }
    for (uint32_t i=0;i<switched_count;++i) { uint32_t node=e->switched_nodes[i]; if (components(e)[node].kind==5u) refresh_wire(e,node); }
    uint32_t start=position!=NONE?row[4]:row[2], count=position!=NONE?row[5]:row[3];
    for (uint32_t i=0;i<count;++i) {
        const uint32_t *r=table(e,23u)+(start+i)*5u; uint32_t node=r[0],deliver=e->switched[node]||r[1];
        for (uint32_t j=0;j<r[4] && !deliver;++j) deliver=e->changed[table(e,24u)[r[3]+j]];
        if (consume(e)) return e->error;
        if (r[2]) {
            if (e->switched[node]) { e->switched[node]=0u; if (mark_wire(e,node)) { e->changed[node]=1u; e->changed_nodes[changed_count++]=node; } }
        } else if (deliver) { e->switched[node]=0u; uint32_t result=components(e)[node].kind==7u && !r[1]?update_wire_watcher(e,node):update_once(e,node); if (result) return result; }
    }
    for (uint32_t i=0;i<switched_count;++i) {
        uint32_t node=e->switched_nodes[i]; if (!e->switched[node]) continue;
        if (consume(e)) return e->error;
        if (e->watched_by_component[node]!=NONE) { if (mark_wire(e,node)) { e->changed[node]=1u; e->changed_nodes[changed_count++]=node; } }
        else if (components(e)[node].kind==5u) refresh_wire(e,node);
        else { uint32_t result=update_once(e,node); if (result) return result; }
    }
    for (uint32_t i=0;i<changed_count;++i) {
        uint32_t record=e->watched_by_component[e->changed_nodes[i]];
        if (record!=NONE) {
            const uint32_t *r=table(e,3u)+record*3u;
            for (uint32_t j=0;j<r[2];++j) { if (consume(e)) return e->error; uint32_t result=update_wire_watcher(e,table(e,4u)[r[1]+j]); if (result) return result; }
        }
    }
    for (uint32_t i=0;i<switched_count;++i) e->switched[e->switched_nodes[i]]=0u;
    for (uint32_t i=0;i<changed_count;++i) e->changed[e->changed_nodes[i]]=0u;
    return NE_OK;
}

static uint32_t pushing_motion(const NEEngine *e,uint32_t piston) {
    const uint32_t *p=table(e,7u)+piston*10u;
    uint32_t slot=e->cell_motion[p[2]]; if (slot==NONE) return NONE;
    const NEMotion *m=&e->motions[slot];
    return (m->flags&1u) && table(e,7u)[m->piston*10u+5u]==p[5]?slot:NONE;
}
static uint32_t request_piston(NEEngine *e,uint32_t piston,uint32_t event) {
    uint8_t bit=(uint8_t)(1u<<event); if (e->queued_pistons[piston]&bit) return NE_OK;
    if (e->piston_event_count==e->piston_event_capacity) return e->error=NE_CAPACITY;
    uint32_t at=(e->piston_event_first+e->piston_event_count)%e->piston_event_capacity;
    e->piston_events[at*2u]=piston; e->piston_events[at*2u+1u]=event; e->piston_event_count++;
    e->queued_pistons[piston]|=bit; return NE_OK;
}
static uint32_t update_piston(NEEngine *e,uint32_t node) {
    uint32_t piston=e->piston_by_component[node]; if (piston==NONE || (e->piston_flags[piston]&8u)) return NE_OK;
    uint32_t powered=e->input_count[node]!=0u;
    if (powered==(e->piston_flags[piston]&1u)) return NE_OK;
    uint32_t pushing=pushing_motion(e,piston),event=powered?0u:1u;
    if (!powered && pushing!=NONE) {
        const NEMotion *m=&e->motions[pushing];
        uint32_t early=1u;
        if (e->motion_phase && e->tick_count-m->start>1u) early=e->landing_cursor!=NONE && m->order<e->landing_cursor;
        if (early) event=2u;
    }
    return request_piston(e,piston,event);
}
static uint32_t add_motion(NEEngine *e,uint32_t piston,uint32_t token,uint32_t destination,uint32_t extending,uint32_t piston_part) {
    uint32_t slot=e->motion_free; if (slot==NONE) return e->error=NE_CAPACITY;
    e->motion_free=e->motions[slot].next;
    NEMotion m={0}; m.piston=piston; m.token=token; m.destination=destination; m.start=e->tick_count; m.landing=e->tick_count+2u;
    m.order=e->motion_last!=NONE && e->motions[e->motion_last].landing==m.landing?e->motions[e->motion_last].order+1u:0u;
    m.flags=(extending?1u:0u)|(piston_part?0u:2u)|(token==NONE?4u:0u); m.next=NONE;
    e->motions[slot]=m;
    if (piston_part) e->piston_motion[piston]=slot;
    if (token!=NONE) { if (e->cell_motion[destination]!=NONE) return e->error=NE_INVALID; e->cell_motion[destination]=slot; }
    if (e->motion_last!=NONE) e->motions[e->motion_last].next=slot; else e->motion_first=slot;
    e->motion_last=slot; return NE_OK;
}
static uint32_t finish_token(NEEngine *e,uint32_t slot) {
    NEMotion m=e->motions[slot];
    if (m.token!=NONE && !(m.flags&4u)) {
        e->motions[slot].flags|=4u; e->cell_motion[m.destination]=NONE;
        return set_token_pos(e,m.token,m.destination);
    } return NE_OK;
}
static uint32_t finish_piston(NEEngine *e,uint32_t slot,uint32_t interrupted) {
    NEMotion m=e->motions[slot]; if (m.flags&2u) return NE_OK;
    e->motions[slot].flags|=2u;
    if (e->piston_motion[m.piston]==slot) e->piston_motion[m.piston]=NONE;
    const uint32_t *p=table(e,7u)+m.piston*10u; e->updated[p[0]]=0u;
    uint32_t status;
    if (m.flags&1u) {
        e->piston_flags[m.piston]&=(uint8_t)~(4u|2u);
        if (!interrupted) e->piston_flags[m.piston]|=2u;
        if ((status=notify_shape(e,p[1])) || (status=notify_home(e,m.piston))) return status;
    } else {
        e->piston_flags[m.piston]&=(uint8_t)~(8u|1u);
        set_component(e,p[0],0u);
        if ((status=notify(e,p[0],1u,1u))) return status;
    }
    return interrupted?NE_OK:update_piston(e,p[0]);
}
static uint32_t heads_blocked(const NEEngine *e,uint32_t piston,uint32_t table_index) {
    const uint32_t *r=table(e,table_index)+piston*2u;
    for (uint32_t i=0;i<r[1];++i) if (e->piston_flags[table(e,10u)[r[0]+i]]&(2u|4u)) return 1u;
    return 0u;
}
static uint32_t run_piston_events(NEEngine *e) {
    while (e->piston_event_count) {
        uint32_t at=e->piston_event_first,piston=e->piston_events[at*2u],event=e->piston_events[at*2u+1u];
        e->piston_event_first=(at+1u)%e->piston_event_capacity; e->piston_event_count--;
        const uint32_t *p=table(e,7u)+piston*10u; uint32_t extending=event==0u,status;
        if (consume(e)) return e->error;
        e->queued_pistons[piston]&=(uint8_t)~(1u<<event); e->updated[p[0]]=0u;
        if ((e->piston_flags[piston]&8u) || (e->input_count[p[0]]!=0u)!=extending) continue;
        if (!extending) {
            if ((e->piston_flags[piston]&4u) && e->piston_motion[piston]!=NONE)
                if ((status=finish_piston(e,e->piston_motion[piston],1u))) return status;
            e->piston_flags[piston]|=8u;
            uint32_t token=p[4] && event!=2u?token_at(e,p[2]):NONE;
            if ((status=add_motion(e,piston,token,p[1],0u,1u)) || (status=notify(e,p[0],1u,1u))) return status;
            if (token!=NONE && (status=set_token_pos(e,token,NONE))) return status;
            if (p[4]) { uint32_t pushing=pushing_motion(e,piston); if (pushing!=NONE) { if ((status=finish_token(e,pushing)) || (status=notify_home(e,piston))) return status; continue; } }
            if (e->piston_flags[piston]&2u) { e->piston_flags[piston]&=(uint8_t)~2u; if ((status=notify_shape(e,p[1]))) return status; }
            if ((status=notify_home(e,piston))) return status;
            continue;
        }
        if (e->piston_flags[piston]&(1u|2u|4u)) continue;
        if (p[8] || e->cell_motion[p[1]]!=NONE || heads_blocked(e,piston,8u)) continue;
        uint32_t token=token_at(e,p[1]),onward=token!=NONE?token_at(e,p[2]):NONE,mate=p[3],mate_open=0u;
        if (mate!=NONE) {
            const uint32_t *m=table(e,7u)+mate*10u;
            mate_open=!m[8] && token_at(e,m[1])==NONE && e->cell_motion[m[1]]==NONE && !(e->piston_flags[mate]&(2u|4u));
        }
        uint32_t far_blocked=p[9] || e->cell_motion[p[2]]!=NONE || heads_blocked(e,piston,9u) || (onward!=NONE && !mate_open);
        if (token!=NONE && far_blocked) continue;
        if (token!=NONE) {
            const uint32_t *r=table(e,12u)+token*2u; uint32_t legal=0u;
            for (uint32_t i=0;i<r[1];++i) if (table(e,13u)[r[0]+i]==p[2]) legal=1u;
            if (!legal) return e->error=NE_UNSUPPORTED;
        }
        if (onward!=NONE) {
            if (mate==NONE) return e->error=NE_INVALID;
            if ((status=set_token_pos(e,onward,NONE)) || (status=add_motion(e,mate,onward,table(e,7u)[mate*10u+1u],1u,0u))) return status;
        }
        if (token!=NONE && (status=set_token_pos(e,token,NONE))) return status;
        e->piston_flags[piston]|=1u; set_component(e,p[0],1u);
        if ((status=notify(e,p[0],1u,1u))) return status;
        e->piston_flags[piston]|=4u;
        if ((status=add_motion(e,piston,token,p[2],1u,1u)) || (status=notify_shape(e,p[1])) || (status=notify_home(e,piston))) return status;
        if (token!=NONE && (status=notify_shape(e,p[2]))) return status;
    } return NE_OK;
}
static uint32_t tick_motions(NEEngine *e) {
    e->motion_phase=1u;
    while (e->motion_first!=NONE) {
        uint32_t slot=e->motion_first; NEMotion m=e->motions[slot]; if (m.landing!=e->tick_count) break;
        e->landing_cursor=m.order;
        if ((m.flags&6u)!=6u) {
            if (consume(e)) return e->error;
            uint32_t status;
            if (m.flags&1u) { if ((status=finish_token(e,slot)) || (status=finish_piston(e,slot,0u))) return status; }
            else { if ((status=finish_piston(e,slot,0u)) || (status=finish_token(e,slot))) return status; }
        }
        e->motion_first=e->motions[slot].next; if (e->motion_first==NONE) e->motion_last=NONE;
        e->motions[slot].next=e->motion_free; e->motion_free=slot;
    }
    e->landing_cursor=NONE; e->motion_phase=0u; return NE_OK;
}

uint32_t ne_snapshot_words(const void *memory) {
    const NEEngine *e=(const NEEngine *)memory; if (!e) return 0u;
    uint32_t motions=0u; for (uint32_t slot=e->motion_first;slot!=NONE;slot=e->motions[slot].next) motions++;
    uint64_t size=8u+(uint64_t)e->program->component_count*4u+e->token_count+e->piston_count+(uint64_t)motions*9u+(uint64_t)e->piston_event_count*2u
        +e->program->link_count+e->program->arrangement_link_count+table_words(e,27u)+e->program->watched_wire_count;
    return size<=UINT32_MAX?(uint32_t)size:0u;
}
uint32_t ne_read_snapshot(const void *memory,uint32_t *out,uint32_t words) {
    const NEEngine *e=(const NEEngine *)memory;
    if (!e || !out || words!=ne_snapshot_words(memory)) return NE_INVALID;
    uint32_t motions=0u; for (uint32_t slot=e->motion_first;slot!=NONE;slot=e->motions[slot].next) motions++;
    out[0]=e->program->component_count; out[1]=e->token_count; out[2]=e->piston_count; out[3]=motions; out[4]=e->piston_event_count;
    out[5]=(uint32_t)e->tick_count; out[6]=(uint32_t)(e->tick_count>>32u); out[7]=3u;
    uint32_t cursor=8u;
    for (uint32_t i=0;i<e->program->component_count;++i) out[cursor++]=e->component_state[i];
    for (uint32_t i=0;i<e->token_count;++i) out[cursor++]=e->token_cell[i];
    for (uint32_t i=0;i<e->piston_count;++i) out[cursor++]=e->piston_flags[i];
    for (uint32_t slot=e->motion_first;slot!=NONE;slot=e->motions[slot].next) {
        NEMotion m=e->motions[slot]; out[cursor++]=m.piston; out[cursor++]=m.token; out[cursor++]=m.destination; out[cursor++]=m.order; out[cursor++]=m.flags;
        out[cursor++]=(uint32_t)m.start; out[cursor++]=(uint32_t)(m.start>>32u); out[cursor++]=(uint32_t)m.landing; out[cursor++]=(uint32_t)(m.landing>>32u);
    }
    for (uint32_t i=0;i<e->piston_event_count;++i) {
        uint32_t index=(e->piston_event_first+i)%e->piston_event_capacity;
        out[cursor++]=e->piston_events[index*2u]; out[cursor++]=e->piston_events[index*2u+1u];
    }
    for (uint32_t i=0;i<e->program->link_count;++i) out[cursor++]=e->contributions[i];
    for (uint32_t i=0;i<e->program->arrangement_link_count;++i) out[cursor++]=e->arrangement_contributions[i];
    for (uint32_t i=0;i<table_words(e,27u);++i) out[cursor++]=e->groups[i];
    for (uint32_t i=0;i<e->program->watched_wire_count;++i) out[cursor++]=e->watched_strengths[i];
    for (uint32_t i=0;i<e->program->component_count;++i) { out[cursor++]=e->input_count[i]; out[cursor++]=e->side_count[i]; }
    for (uint32_t i=0;i<e->program->component_count;++i) out[cursor++]=e->comparator_outputs[i];
    return cursor==words?NE_OK:NE_INVALID;
}
