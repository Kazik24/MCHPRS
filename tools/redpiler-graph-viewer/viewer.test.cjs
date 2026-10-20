const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

test('CPU bit masks preserve bit 63 and reject malformed programs', () => {
  const context = vm.createContext({document: {addEventListener() {}}});
  const source = fs.readFileSync(path.join(__dirname, 'viewer.js'), 'utf8');
  vm.runInContext(source.slice(source.indexOf('function validateCpuKernels'), source.indexOf('function validateGraph')), context);
  const p = {read_start: 0, read_len: 64, clause_start: 0, clause_len: 1};
  const program = {method: 'live_bitmask_dnf', effects: 'original_graph_fifo',
    reads: Array.from({length: 64}, () => ({kind:'guard', guard:'guard:0'})),
    entries: [{actor:'piston:0',main:{...p},quasi:{...p},valid:{...p}}],
    require_true:['8000000000000000'],require_false:['0000000000000001']};
  const ids = new Set(['guard:0','piston:0']);
  context.validateCpuKernels(program, ids);
  for (const [field,value] of [['read_len',65],['read_start',1],['clause_len',2],['clause_start',-1],['read_len',true]]) {
    const changed=structuredClone(program); changed.entries[0].main[field]=value;
    assert.throws(() => context.validateCpuKernels(changed, ids));
  }
  for (const value of [2 ** 63, '800000000000000', 'z000000000000000', '0000000000000001']) {
    const changed=structuredClone(program); changed.require_true=[value];
    assert.throws(() => context.validateCpuKernels(changed, ids));
  }
  ids.add('node:0');
  program.reads[0] = {kind:'wire_emission',node:'node:0',face:'north',threshold:3};
  context.validateCpuKernels(program, ids);
  for (const [field,value] of [['face','up'],['threshold',15],['threshold',true],['node','node:1']]) {
    const changed=structuredClone(program); changed.reads[0][field]=value;
    assert.throws(() => context.validateCpuKernels(changed, ids));
  }
});

class Element {
  constructor() {
    this.listeners = new Map();
    this.children = [];
    this.hidden = false;
    this.value = "";
    this.textContent = "";
    this.dataset = {};
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  dispatch(type, event) {
    return this.listeners.get(type)?.(event);
  }

  replaceChildren(...children) {
    this.children = children;
  }

  append(...children) {
    this.children.push(...children);
  }
}

function makeViewer() {
  const elements = new Map();
  const document = {
    getElementById(id) {
      if (!elements.has(id)) elements.set(id, new Element());
      return elements.get(id);
    },
    createElement() {
      return new Element();
    },
    createTextNode(text) {
      return { textContent: text };
    },
    addEventListener() {},
    querySelectorAll(selector) {
      const children = [...elements.values()].flatMap((element) => element.children).flatMap((element) => element.children || []);
      return children.filter((element) => element.className === selector.split(':')[0].slice(1) && (!selector.includes(':checked') || element.checked));
    },
  };
  let cytoscapeOptions;
  const context = vm.createContext({ document, setTimeout, clearTimeout, TextDecoder, TextEncoder });
  vm.runInContext(fs.readFileSync(path.join(__dirname, 'vendor/cytoscape.min.js'), 'utf8'), context);
  const realCytoscape = context.cytoscape;
  let cy;
  const cytoscape = (options) => {
    cytoscapeOptions = options;
    options.container = undefined; options.headless = true; options.styleEnabled = true;
    cy = realCytoscape(options);
    return cy;
  };
  const source = fs.readFileSync(path.join(__dirname, "viewer.js"), "utf8");
  context.cytoscape = cytoscape;
  vm.runInContext(source, context);
  vm.runInContext('const actualLayout = layoutGraph; layoutGraph = () => {};', context);
  elements.get('depth').value = '2';
  elements.get('layout').value = 'cose';
  return { elements, evaluate: (code) => vm.runInContext(code, context), get cy() { return cy; }, get cytoscapeOptions() { return cytoscapeOptions; } };
}

function instantCertificate(graph) {
  return {
    piston: graph.graph.pistons[0].id,
    resource: graph.graph.resources[0].id,
    observer: graph.graph.nodes.find((node) => node.kind.kind === 'observer').id,
    partner: graph.graph.wire_plans[1].node,
  };
}

test('classic nets display committed edges and reject corrupt masks, occurrences and ownership', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const fixture = path.join(__dirname, '../../test_data/graph-fixtures/simple_and_gate-classic.graph.json');
  const original = JSON.parse(fs.readFileSync(fixture, 'utf8'));
  await load(viewer, original);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate(`connections.has('classic_net:0:ingress')`), true);
  assert.equal(viewer.evaluate(`[...connections.values()].some((edge) => edge.record.execution === 'classic_provenance')`), false);
  assert.equal(viewer.cy.getElementById('classic_net:0').length, 1);
  const sampled = JSON.parse(JSON.stringify(original));
  const sampledOp = sampled.graph.classic_nets[0].entries.find((entry) => entry.ops.length).ops[0];
  sampledOp.sample = { kind: 'boolean', nonzero: sampledOp.nonzero };
  await load(viewer, sampled);
  assert.equal(viewer.elements.get('error').hidden, true);
  for (const mutation of ['mask', 'sample', 'attenuation', 'occurrence', 'overlap']) {
    const damaged = JSON.parse(JSON.stringify(original));
    const net = damaged.graph.classic_nets[0];
    if (mutation === 'mask') net.entries.find((entry) => entry.ops.length).ops[0].when[0] = 'invalid';
    else if (mutation === 'sample') net.entries.find((entry) => entry.ops.length).ops[0].sample = { kind: 'comparator', strength: 16, powered: true };
    else if (mutation === 'attenuation') net.wires[0].attenuation = 16;
    else if (mutation === 'occurrence') net.entries[0].ordinal = 999999;
    else damaged.graph.classic_nets.push({ ...net, id: 'classic_net:1' });
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false, mutation);
  }
});

test('weighted queries distinguish live reads from settled provenance and validate operands', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const node = graph.graph.nodes[0].id;
  graph.graph.fixed_signal_queries = ['live_state', 'settled_provenance'].map((semantics, ordinal) => ({
    id: `fixed_signal:${ordinal}`, semantics, constant: 0,
    to: { kind: 'input', node, port: 'main' }, inputs: [{ node, attenuation: 2, require_cross: false }],
  }));
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate(`connections.get('fixed_signal:0:input:0').type`), 'fixed_live_read');
  assert.equal(viewer.evaluate(`connections.get('fixed_signal:1:input:0').type`), 'settled_weighted_provenance');
  for (const mutation of ['weight', 'node', 'semantics', 'port']) {
    const damaged = structuredClone(graph);
    const query = damaged.graph.fixed_signal_queries[0];
    if (mutation === 'weight') query.inputs[0].attenuation = 15;
    else if (mutation === 'node') query.inputs[0].node = 'node:999999';
    else if (mutation === 'port') query.to.port = 'invalid';
    else query.semantics = 'compiled';
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false, mutation);
  }
});

test('compiled hop roots bind their wire and preserve older exports', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const wire = graph.graph.wire_plans[0];
  const root = graph.graph.sites.find((site) => site.source.kind === 'component' && site.source.node === wire.node);
  wire.hop_root = root.id;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  for (const invalid of ['site:999999', 'node:0', 7]) {
    wire.hop_root = invalid;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
  wire.hop_root = graph.graph.sites.find((site) => site.id !== root.id).id;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, false);
  delete wire.hop_root;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
});

test('loads built-in resets and rejects malformed clocks without replacing the graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const observer = graph.graph.nodes.find((node) => node.kind.kind === 'observer');
  observer.kind.builtin_reset = true;
  observer.output_route = null;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  assert.equal(viewer.evaluate(`recordLabel(recordIndex.get('${observer.id}'))`), 'built-in reset');
  const previous = viewer.cy;
  observer.kind.builtin_reset = 'true';
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, false);
  assert.equal(viewer.cy, previous);
  observer.kind.builtin_reset = true;
  observer.output_route = graph.graph.routes[0].id;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, false);
  assert.equal(viewer.cy, previous);
});

test('loads additive instant bindings and accepts older exports', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  graph.graph.wire_plans[0].instant_reset = instantCertificate(graph);
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  graph.graph.wire_plans[0].instant_reset = null;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  delete graph.graph.wire_plans[0].instant_reset;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
});

test('rejects malformed and overlapping instant bindings without replacing the graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  const previous = viewer.cy;
  const base = adderFixture();
  const valid = instantCertificate(base);
  const invalid = [[], false, 'reset', {}, { ...valid, piston: valid.resource },
    { ...valid, resource: valid.piston }, { ...valid, observer: valid.partner },
    { ...valid, partner: base.graph.wire_plans[0].node }, { ...valid, partner: 'node:9999999' }];
  for (const instant_reset of invalid) {
    const graph = adderFixture();
    graph.graph.wire_plans[0].instant_reset = instant_reset;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false, JSON.stringify(instant_reset));
    assert.equal(viewer.cy, previous);
  }
  const overlap = adderFixture();
  overlap.graph.wire_plans[0].instant_reset = valid;
  overlap.graph.wire_plans[1].instant_reset = { ...valid, partner: overlap.graph.wire_plans[0].node };
  await load(viewer, overlap);
  assert.equal(viewer.elements.get('error').hidden, false);
  assert.equal(viewer.cy, previous);
});

for (const count of [1, 20_001]) {
  test(`loads ${count} nodes and sends only a bounded view to the layout`, async (t) => {
    const viewer = makeViewer();
    t.after(() => viewer.cy?.destroy());
    const document = {
      format: "mchprs.redpiler.graph",
      version: { major: 1, minor: 0 },
      stage: "electrical_draft",
      capabilities: [],
      graph: {
        nodes: Array.from({ length: count }, (_, index) => ({
          id: `node:${index}`,
          kind: { kind: "electrical", component: { type: "lever" } },
          position: { x: index, y: 0, z: 0 },
          output_route: null,
        })),
        sites: [],
        contacts: [],
        resources: [],
        guards: [],
        pistons: [],
        power_links: [],
        wire_plans: [],
        wire_relations: [],
        comparator_plans: [],
        callback_links: [],
        routes: [],
        profiles: [],
      },
      snapshots: [],
      trace: null,
    };
    const fileText = JSON.stringify(document);
    await viewer.elements.get("file").dispatch("change", {
      target: {
        files: [{ size: fileText.length, text: async () => fileText }],
      },
    });

    assert.equal(viewer.elements.get("error").hidden, true, viewer.elements.get("error").textContent);
    assert.ok(viewer.cytoscapeOptions.elements.filter((element) => !('source' in element.data)).length <= 400);
    if (count > 1) {
      const last = `node:${count - 1}`;
      assert.ok(!viewer.cytoscapeOptions.elements.some((element) => element.data.id === last));
      viewer.elements.get("search").value = last;
      viewer.elements.get("find").dispatch("click");
      viewer.elements.get("matches").children[1].dispatch("click");
      assert.ok(viewer.cytoscapeOptions.elements.some((element) => element.data.id === last));
    }
  });
}

test('overview admits connected endpoints before filling the node budget', () => {
  const viewer = makeViewer();
  const view = viewer.evaluate(`(() => {
    activeNodeIds = Array.from({length: 450}, (_, i) => 'node:' + i);
    recordIndex = new Map(activeNodeIds.map(id => [id, {id, kind:{kind:'wire'}}]));
    connections = new Map([['edge:late', {id:'edge:late', source:'node:420', target:'node:421', type:'program_input'}]]);
    neighbors = new Map();
    const result = selectView(null, true, new Set(['program_input']), 1, activeNodeIds);
    return {nodes:Array.from(result.nodes), edges:Array.from(result.edges, edge => edge.id)};
  })()`);
  assert.ok(view.nodes.includes('node:420'));
  assert.ok(view.nodes.includes('node:421'));
  assert.deepEqual(Array.from(view.edges), ['edge:late']);
  assert.ok(view.nodes.length <= 400);
  assert.ok(view.edges.length <= 800);
});

test("a high-degree fixture keeps the layout bounded and every edge endpoint present", async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const document = JSON.parse(fs.readFileSync(path.join(__dirname, "../../test_data/graph-fixtures/adder-1bit/adder-1bit.graph.json"), "utf8"));
  const first = document.graph.nodes[0].id;
  const source = document.graph.power_links[0].from;
  const guard = document.graph.guards[0].id;
  const addedNodes = [];
  const addedSites = [];
  const addedLinks = [];
  for (let index = 0; index < 1_000; index++) {
    const id = `node:${document.graph.nodes.length + index}`;
    const site = `site:${document.graph.sites.length + index}`;
    addedNodes.push({ id, kind: { kind: "electrical", component: { type: "lamp" } }, position: { x: index, y: 0, z: 0 }, output_route: null });
    addedSites.push({ id: site, source: { kind: "component", node: id }, position: { x: index, y: 0, z: 0 } });
    addedLinks.push({ id: `power_link:${document.graph.power_links.length + index}`, from: { ...source, site }, to: { kind: "input", node: first, port: "main" }, face: "east", attenuation: 0, guard });
  }
  document.graph.nodes.push(...addedNodes);
  document.graph.sites.push(...addedSites);
  document.graph.power_links.push(...addedLinks);
  const fileText = JSON.stringify(document);
  await viewer.elements.get("file").dispatch("change", {
    target: { files: [{ size: fileText.length, text: async () => fileText }] },
  });
  assert.equal(viewer.elements.get("error").hidden, true, viewer.elements.get("error").textContent);
  const elements = viewer.cytoscapeOptions.elements;
  const nodes = new Set(elements.filter((element) => !("source" in element.data)).map((element) => element.data.id));
  const edges = elements.filter((element) => "source" in element.data);
  assert.ok(nodes.size <= 400);
  assert.ok(edges.length <= 800);
  for (const edge of edges) {
    assert.ok(nodes.has(edge.data.source));
    assert.ok(nodes.has(edge.data.target));
  }
});

test('fixed classic summary tolerates old exports and reports compile or decline inventory', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  assert.match(viewer.elements.get('availability').textContent, /fixed classic: not included/);

  graph.graph.fixed_classic = { status: 'declined', decline: 'dynamic contact geometry' };
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  assert.match(viewer.elements.get('availability').textContent, /fixed classic: declined · dynamic contact geometry/);

  graph.graph.fixed_classic = {
    status: 'compiled',
    inventory: { accounted_bytes: 4096, tables: [{ table: 'nodes', entries: 12 }, { table: 'wires', entries: 4 }, { table: 'route_ops', entries: 20 }] },
    program: {},
  };
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  assert.match(viewer.elements.get('availability').textContent, /fixed classic: compiled · 12 nodes · 4 wires · 20 route ops · 4096 accounted bytes/);
});

function compiledFixture() {
  const graph = adderFixture();
  const wire = graph.graph.wire_plans[0].node;
  const wireSite = graph.graph.sites.find((site) => site.source.kind === 'component' && site.source.node === wire).id;
  graph.graph.instant_transport = [{
    id: 'instant_transport:0', wires: [wire], sites: [wireSite], site_wire: [0],
    conditional_inputs: [0], ordered_receivers: false, canonical_repeaters: [],
  }];
  graph.graph.instant_programs = {
    instant_drives: {
      wires: graph.graph.nodes.filter((node) => node.kind?.kind === 'wire').map((node) => ({ node: node.id, query: node.id === wire ? 0 : null })),
      queries: [{ constant: 0, terms: [0, 1] }],
      terms: [{ source: { kind: 'output', node: graph.graph.nodes[0].id }, condition: 0, attenuation: 1 }],
      conditions: [{ kind: 'always' }],
    },
    signal_channels: [{ net: 0, wires: [wire], input: { source: { kind: 'output', node: graph.graph.nodes[0].id }, condition: 0 },
      profiles: [[0], [15]], discriminator: 0,
      transitions: [{ old_input: 0, root: 0, transcript: null }, { old_input: 1, root: 0, transcript: null }] }],
    stable_logic: [{ net: 0, wires: [wire], expressions: [{ constant: 0, terms: [0, 1] }],
      inputs: [{ source: { kind: 'output', node: graph.graph.nodes[0].id }, condition: 0 }],
      terms: [{ input: 0, attenuation: 1 }], input_users: [[0, 1]], input_user_expressions: [0],
      outputs: [{ site: wireSite, candidates: [{ wire: 0, slot: 0, admission: true }] }],
      wire_outputs: [[0, 1]], wire_output_indices: [0] }],
    value_regions: [{ net: 0, wires: [{ node: wire, compact_capture: true, region: 0, region_local: 0 }],
      regions: [{ component: 0, wires: [0, 1], drive_groups: [0, 1], conditional_arcs: [0, 1] }],
      region_wires: [0], drive_groups: [{ query: 0, row: 0 }], conditional_arcs: [{ source: 0, row: 1, selector: 0 }],
      selectors: [{ horizontal_site: wireSite, ascent_up_site: null }], distance_rows: [{ start: 0, len: 1 }, { start: 1, len: 1 }], distances: [0, 0] }],
    transition_eligibility: { semantics: 'eligibility only; runtime-learned transcripts are unavailable', nets: [{ net: 0, eligible: false }] },
  };
  return graph;
}

test('compiled program view collapses mapped wires and expands full typed IR lazily', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = compiledFixture(), wire = graph.graph.wire_plans[0].node;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.elements.get('program-view').value, 'compiled');
  assert.equal(viewer.cy.hasElementWithId(`compiled_net:0`), true);
  assert.equal(viewer.cy.hasElementWithId(wire), false);
  assert.equal(viewer.cy.hasElementWithId('compiled_drive:0'), true);
  assert.ok(viewer.evaluate('[...connections.values()].some((edge) => edge.type === "mapping_witness")'));
  assert.match(viewer.elements.get('program-status').textContent, /eligibility.*no exported learned transcript/);

  viewer.evaluate("focusRecord('compiled_net:0')");
  const expand = viewer.elements.get('dependencies').children.find((child) => child.textContent.startsWith('Expand this compiled'));
  assert.ok(expand, 'full IR starts collapsed');
  expand.dispatch('click');
  const expanded = viewer.elements.get('dependencies').children.find((child) => child.className === 'state-preview');
  assert.match(expanded.textContent, /physical_wires/);
  assert.match(expanded.textContent, /drive_expressions/);
  assert.match(expanded.textContent, /runtime_learned_transcript/);

  viewer.elements.get('program-view').value = 'physical';
  viewer.elements.get('program-view').dispatch('change');
  assert.equal(viewer.cy.hasElementWithId(wire), true);
  assert.equal(viewer.cy.hasElementWithId('compiled_net:0'), false);
});

test('compiled view rejects malformed local operands and keeps old graph loaded', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const valid = compiledFixture();
  await load(viewer, valid);
  const previous = viewer.cy;
  const malformed = [
    (g) => { g.graph.instant_programs.instant_drives.queries[0].terms = [0, 2]; },
    (g) => { g.graph.instant_programs.instant_drives.terms[0].condition = 1; },
    (g) => { g.graph.instant_programs.signal_channels[0].transitions[1].root = 1; },
    (g) => { g.graph.instant_programs.stable_logic[0].input_users = [[0, 0]]; },
    (g) => { g.graph.instant_programs.value_regions[0].conditional_arcs[0].source = 1; },
    (g) => { g.graph.instant_programs.value_regions[0].distance_rows[0].len = 2; },
    (g) => { g.graph.instant_programs.transition_eligibility.nets[0] = { net: 0, eligible: true, wire_queries: [{ wire: 1, node: g.graph.wire_plans[0].node, query: 0 }] }; },
    (g) => { g.graph.instant_transport[0].wires[0] = 'node:999999'; },
  ];
  for (const mutate of malformed) {
    const damaged = structuredClone(valid); mutate(damaged);
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});

test('older graph exports clearly retain the physical-only view', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  delete graph.graph.instant_programs;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.elements.get('program-view').disabled, true);
  assert.equal(viewer.elements.get('program-view').value, 'physical');
  assert.match(viewer.elements.get('program-status').textContent, /unavailable/);
  assert.equal(viewer.cy.nodes().length, 152);
});

function adderFixture() {
  return JSON.parse(fs.readFileSync(path.join(__dirname, '../../test_data/graph-fixtures/adder-1bit/adder-1bit.graph.json'), 'utf8'));
}

async function load(viewer, graph) {
  const text = JSON.stringify(graph);
  await viewer.elements.get('file').dispatch('change', { target: { files: [{ size: text.length, text: async () => text }] } });
}

test('adder overview includes all components and power links; inspection keeps their positions', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const fixture = adderFixture();
  await load(viewer, fixture);
  assert.equal(viewer.elements.get('scope').value, 'overview');
  assert.equal(viewer.cy.nodes().length, 152);
  assert.equal(viewer.cy.edges().length, fixture.graph.power_links.length);
  for (const row of fixture.graph.nodes) assert.ok(viewer.cy.hasElementWithId(row.id));
  assert.equal(viewer.cy.getElementById('node:49').data('kind'), 'repeater');
  assert.equal(viewer.cy.getElementById('node:50').data('kind'), 'wire');
  const previous = viewer.cy;
  viewer.evaluate('focusRecord("node:49")');
  assert.equal(viewer.cy, previous);
  assert.ok(viewer.elements.get('dependencies').children.some((child) => child.textContent.startsWith('Incoming (')));
  assert.equal(viewer.cy.getElementById('node:49').selected(), true);
  assert.equal(viewer.cy.getElementById('node:50').hasClass('neighbor'), true);
  const guard = fixture.graph.power_links[0].guard;
  viewer.evaluate('focusRecord("power_link:0")');
  assert.match(viewer.elements.get('details').textContent, new RegExp(guard));
  viewer.evaluate(`focusRecord("${guard}")`);
  assert.match(viewer.elements.get('details').textContent, new RegExp(`"id": "${guard}"`));
});

test('large graph files stream across UTF-8, escape, token and row boundaries', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  graph.stage = 'FPU 🌍 with \\"quoted\\" text';
  const bytes = new TextEncoder().encode(JSON.stringify(graph, null, 2));
  let reads = 0;
  const file = {
    size: 33 * 1024 * 1024,
    text() { throw new Error('large files must not use File.text()'); },
    stream() {
      let offset = 0;
      return { getReader() { return { async read() {
        reads++;
        if (offset >= bytes.length) return { done: true };
        const end = Math.min(offset + 7, bytes.length);
        const value = bytes.slice(offset, end); offset = end;
        return { done: false, value };
      } }; } };
    },
  };
  await viewer.elements.get('file').dispatch('change', { target: { files: [file] } });
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.cy.nodes().length, 152);
  assert.ok(reads > 10);
  assert.equal(viewer.evaluate('graphData.stage'), graph.stage);
});

test('streaming JSON rejects truncation and trailing content safely', async () => {
  const viewer = makeViewer();
  const parse = (text) => viewer.evaluate(`(async () => await new StreamingJsonParser({getReader(){let done=false;return {async read(){if(done)return {done:true};done=true;return {done:false,value:new TextEncoder().encode(${JSON.stringify(text)})}}}}}).parse())()`);
  await assert.rejects(parse('{"a":[1,2]} trailing'));
  await assert.rejects(parse('{"a":[1,2]'));
  const proto = await parse('{"__proto__":{"polluted":true}}');
  assert.equal(Object.prototype.hasOwnProperty.call(proto, '__proto__'), true);
  assert.equal(Object.getPrototypeOf(proto).polluted, undefined);
});

test('referenced-by details page through every result without duplicates', () => {
  const viewer = makeViewer();
  viewer.evaluate(`recordIndex = new Map([
    ['node:0', { source: 'guard:0' }], ['node:1', { source: 'guard:0' }],
    ['node:2', { source: 'guard:0' }], ['node:3', { source: 'guard:0' }]
  ])`);
  const first = viewer.evaluate(`reverseReferences('guard:0', 0, 2)`);
  const second = viewer.evaluate(`reverseReferences('guard:0', 2, 2)`);
  assert.deepEqual(Array.from(first.ids), ['node:0', 'node:1']);
  assert.deepEqual(Array.from(second.ids), ['node:2', 'node:3']);
  assert.equal(first.total, 4);
  assert.equal(second.total, 4);
});

test('following a filtered wire relation reveals it and both endpoints', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  assert.equal(viewer.cy.hasElementWithId('wire_relation:0'), false);
  viewer.evaluate('focusRecord("wire_relation:0", true)');
  const edge = viewer.cy.getElementById('wire_relation:0');
  assert.equal(edge.empty(), false);
  assert.equal(edge.source().id(), 'node:50');
  assert.equal(edge.target().id(), 'node:49');
  assert.equal(edge.selected(), true);
  assert.equal(edge.hasClass('filtered'), false);
});

test('world layout separates coincident records and filters Y layers without losing references', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  viewer.elements.get('layout').value = 'position';
  viewer.evaluate('actualLayout()');
  const piston = viewer.cy.getElementById('node:0');
  const contact = viewer.cy.getElementById('node:13');
  assert.notDeepEqual(piston.position(), contact.position());
  for (const node of viewer.cy.nodes()) {
    assert.ok(Number.isFinite(node.position('x')));
    assert.ok(Number.isFinite(node.position('y')));
  }
  viewer.elements.get('layer').value = '31';
  viewer.elements.get('layer').dispatch('change');
  assert.equal(viewer.cy.getElementById('node:1').hasClass('filtered'), true);
  assert.equal(piston.hasClass('filtered'), false);
  viewer.evaluate('focusRecord("node:1")');
  assert.equal(viewer.elements.get('layer').value, '');
  assert.equal(viewer.cy.getElementById('node:1').hasClass('filtered'), false);
});

test('snapshot guards preserve unknown site operands and hide only disabled links', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  viewer.elements.get('snapshot').value = 'snapshot:initial';
  viewer.elements.get('snapshot').dispatch('change');
  assert.equal(viewer.cy.edges('.enabled').length, 188);
  assert.equal(viewer.cy.edges('.disabled').length, 13);
  assert.equal(viewer.cy.edges('.unknown').length, 4);
  viewer.elements.get('hide-inactive').checked = true;
  viewer.elements.get('hide-inactive').dispatch('change');
  assert.equal(viewer.cy.edges('.disabled.filtered').length, 13);
  assert.equal(viewer.cy.edges('.unknown.filtered').length, 0);
  const states = viewer.evaluate(`evaluateGuards({graph:{guards:[
    {id:'guard:0',kind:'always'}, {id:'guard:1',kind:'site'},
    {id:'guard:2',kind:'not',guard:'guard:1'},
    {id:'guard:3',kind:'all',guards:['guard:0','guard:1']},
    {id:'guard:4',kind:'any',guards:['guard:0','guard:1']},
    {id:'guard:5',kind:'state',test:{kind:'piston_extended',piston:'piston:99'}}
  ]}}, {pistons:[],resources:[]})`);
  assert.equal(states.get('guard:2'), null);
  assert.equal(states.get('guard:3'), null);
  assert.equal(states.get('guard:4'), true);
  assert.equal(states.get('guard:5'), null);
});

test('resource inspection highlights all of its contacts and follows references in both directions', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  viewer.evaluate('focusRecord("node:49")');
  viewer.elements.get('scope').value = 'neighborhood';
  viewer.elements.get('scope').dispatch('change');
  viewer.evaluate('focusRecord("resource:1", true)');
  for (const id of ['node:14', 'node:15', 'node:42']) assert.equal(viewer.cy.getElementById(id).selected(), true);
  assert.ok(viewer.elements.get('dependencies').children.some((child) => child.textContent.startsWith('Referenced by')));
});

test('invalid imports report errors and preserve the previously loaded graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  const previous = viewer.cy;
  for (const mutate of [
    (graph) => { graph.version.major = 2; },
    (graph) => { graph.graph.nodes[0].output_route = 'route:999999'; },
    (graph) => { graph.graph.nodes[1].id = graph.graph.nodes[0].id; },
  ]) {
    const graph = adderFixture(); mutate(graph);
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});

test('searching an exported piston update reveals its direct callback edge', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  await load(viewer, graph);
  const nodes = new Map(graph.graph.nodes.map((node) => [node.id, node]));
  const update = graph.graph.callback_links.find((link) => link.kind === 'update'
    && nodes.get(link.from)?.kind.kind === 'piston'
    && nodes.get(link.to)?.kind.kind === 'observer');
  assert.ok(update, 'adder fixture includes piston-to-observer update links');
  viewer.evaluate(`focusRecord("${update.id}")`);
  const edge = viewer.cy.getElementById(update.id);
  assert.equal(edge.source().id(), update.from);
  assert.equal(edge.target().id(), update.to);
  assert.match(edge.data('label'), /^update/);
});

function addDustPath(graph) {
  const nodes = new Map(graph.graph.nodes.map((node) => [node.id, node]));
  const inputSite = graph.graph.sites.find((site) => site.source.kind === 'component'
    && nodes.get(site.source.node)?.kind.kind === 'electrical');
  assert.ok(inputSite, 'fixture has an electrical component site');
  const wires = graph.graph.wire_plans.slice(0, 2).map((wire) => wire.node);
  assert.equal(wires.length, 2);
  graph.graph.dust_paths = [{
    id: 'dust_path:0', input_node: inputSite.source.node, input_site: inputSite.id,
    input_face: 'east', wires,
  }];
  return graph;
}

test('accepts wire query fact masks and older exports without the optional field', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  graph.graph.wire_plans[0].query_facts = null;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  graph.graph.wire_plans[0].query_facts = { known: 0xffffff, solid: 1, transparent: 2, wire: 4, zero_no_dust: 0xfffffe };
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  for (const isolated of [true, false]) {
    graph.graph.wire_plans[0].query_facts.isolated = isolated;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  }
  for (const raw_sides of [0, 0x55, 0xaa, null]) {
    graph.graph.wire_plans[0].query_facts.raw_sides = raw_sides;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  }
});

test('rejects malformed wire query fact masks while preserving the loaded graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  const previous = viewer.cy;
  const valid = { known: 0xffffff, solid: 1, transparent: 2, wire: 4, zero_no_dust: 0xfffffe };
  const invalid = [[], false, 'masks', {}, { ...valid, known: 0 }, { ...valid, zero_no_dust: 1 }];
  for (const raw_sides of [-1, 256, 0.5, true, '1', 3, 12, 48, 192]) invalid.push({ ...valid, raw_sides });
  invalid.push({ ...valid, known: 0xfffffe, solid: 0, raw_sides: 0 });
  for (const isolated of [null, 0, 1, 'true', [], {}]) invalid.push({ ...valid, isolated });
  for (const key of Object.keys(valid)) {
    for (const value of [-1, 1 << 24, 0.5, true, null, '1']) invalid.push({ ...valid, [key]: value });
  }
  for (const query_facts of invalid) {
    const graph = adderFixture();
    graph.graph.wire_plans[0].query_facts = query_facts;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false, JSON.stringify(query_facts));
    assert.equal(viewer.cy, previous);
  }
});

test('loads and indexes a dust-path certificate and accepts older exports without the table', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = addDustPath(adderFixture());
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  viewer.evaluate('focusRecord("dust_path:0")');
  assert.match(viewer.elements.get('details').textContent, /dust_path:0/);

  const older = adderFixture();
  delete older.graph.dust_paths;
  await load(viewer, older);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
});

test('rejects dangling and duplicate dust-path references while preserving the loaded graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  const previous = viewer.cy;
  const malformed = [
    (graph) => { addDustPath(graph); graph.graph.dust_paths[0].wires[1] = 'node:999999'; },
    (graph) => { addDustPath(graph); graph.graph.dust_paths[0].wires[1] = graph.graph.dust_paths[0].wires[0]; },
    (graph) => { addDustPath(graph); graph.graph.dust_paths.push({ ...graph.graph.dust_paths[0] }); },
    (graph) => { addDustPath(graph); graph.graph.dust_paths[0].input_face = 'up'; },
  ];
  for (const mutate of malformed) {
    const graph = adderFixture();
    mutate(graph);
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});

test('validates compiled wire inputs while accepting older facts', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const facts = { known: 0xffffff, solid: 0, transparent: 0, wire: 0, zero_no_dust: 0 };
  graph.graph.wire_plans[0].query_facts = facts;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  facts.transfer = { seed: { direct: 4, conditional: 1 << 20 }, turbo: { direct: 1, conditional: 1 << 7 } };
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  const previous = viewer.cy;
  for (const invalid of [null, [], { direct: true, conditional: 0 }, { direct: 1 << 23, conditional: 0 },
    { direct: 1 << 7, conditional: 1 << 7 }, { direct: 0, conditional: 4 }, { direct: 4, conditional: 0 }]) {
    facts.transfer.turbo = invalid;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false, JSON.stringify(invalid));
    assert.equal(viewer.cy, previous);
  }
});


test('loads private instant proxies and rejects invalid route programs', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, '../../test_data/graph-fixtures/adder-1bit/adder-1bit-o3.graph.json'), 'utf8'));
  assert.equal(fixture.graph.instant_proxies.length, 4);
  await load(viewer, fixture);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const previous = viewer.cy;
  for (const mutation of [
    (graph) => { graph.instant_regions[0].incoming[0].ordinal = 999999; },
    (graph) => { graph.instant_proxies[0].region = 'instant_region:999999'; },
    (graph) => { graph.instant_proxies[0].routes[0].operations.push('dispatch'); },
    (graph) => { graph.instant_proxies[0].routes[0].operations[0] = 'skip'; },
    (graph) => { graph.instant_regions[0].stable_inputs = 'true'; },
    (graph) => { graph.instant_proxies[0].routes[0].operations[0] = {kind: 'sample', piston: 'node:0'}; },
    (graph) => { graph.instant_proxies[0].private_cells.push(graph.instant_proxies[0].private_cells[0]); },
  ]) {
    const invalid = JSON.parse(JSON.stringify(fixture));
    mutation(invalid.graph);
    await load(viewer, invalid);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});
const closedMachine = (graph) => ({
  encoding: 'closed-instant-machine-v1', controls: [graph.nodes[0].id],
  owners: {components: graph.nodes.map((n) => n.id), pistons: graph.pistons.map((p) => p.id), resources: graph.resources.map((r) => r.id)},
  states: [{components: graph.nodes.map(() => 0), pistons: graph.pistons.map(() => 0), resources: graph.resources.map(() => null)}],
  transitions: [[{next: 0}, null]],
});

const compactKernel = (graph) => {
  const state = () => ({
    components: graph.nodes.map(() => 0),
    pistons: graph.pistons.map(() => 0),
    resources: graph.resources.map(() => [0, 0]),
    future: {tick_len: 0, ticks: [], request_len: 0, requests: [], deferred_len: 0, deferred: []},
  });
  return {
    encoding: 'compact-instant-kernel-v1',
    trigger: graph.nodes[0].id,
    data_controls: [graph.nodes[1].id],
    owners: {
      components: graph.nodes.map((row) => row.id),
      pistons: graph.pistons.map((row) => row.id),
      resources: graph.resources.map((row) => row.id),
      contacts: graph.contacts.map((row) => row.id),
    },
    prefix: 0, period: 1, estimated_runtime_bytes: 8,
    expressions: [{kind: 'constant', value: '0'}],
    entry: state(),
    phases: [{components: [], dirty: [], piston_revisions: [], resource_revisions: [], effects: [],
      scheduled: 0, accepted: 0, published: 0, state: state()}],
  };
};

test('closed instant machines retain owner dimensions and bounded targets', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const fixture = adderFixture();
  fixture.graph.instant_machine = closedMachine(fixture.graph);
  await load(viewer, fixture);
  assert.equal(viewer.elements.get('error').hidden, true);
  for (const corrupt of [
    (m) => m.transitions[0][0].next = m.states.length,
    (m) => m.controls[0] = 'node:999999',
    (m) => m.states[0].components.pop(),
    (m) => m.owners.components.pop(),
  ]) {
    const invalid = JSON.parse(JSON.stringify(fixture));
    corrupt(invalid.graph.instant_machine);
    await load(viewer, invalid);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
});

test('compact instant kernels validate the bounded DAG, required roots, and typed owners', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const fixture = adderFixture();
  fixture.graph.instant_kernel = compactKernel(fixture.graph);
  await load(viewer, fixture);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const previous = viewer.cy;
  const mutations = [
    (kernel) => { delete kernel.owners.components; },
    (kernel) => { delete kernel.entry.future.ticks; },
    (kernel) => { kernel.phases[0].effects.push({enabled: 0, action: 'extend', extended: false}); },
    (kernel) => { kernel.trigger = fixture.graph.pistons[0].id; },
    (kernel) => { kernel.phases[0].components.push({node: fixture.graph.pistons[0].id, changed: 0, value: 0}); },
    (kernel) => { kernel.phases[0].effects.push({enabled: 0, piston: fixture.graph.nodes[0].id, action: 'extend', extended: false}); },
    (kernel) => { kernel.expressions[0].value = '00'; },
    (kernel) => { kernel.expressions[0].value = '9'.repeat(5000); },
    (kernel) => { kernel.period = 0; },
  ];
  for (const mutation of mutations) {
    const invalid = JSON.parse(JSON.stringify(fixture));
    mutation(invalid.graph.instant_kernel);
    await load(viewer, invalid);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
  const maxU64 = JSON.parse(JSON.stringify(fixture));
  maxU64.graph.instant_kernel.expressions[0].value = '18446744073709551615';
  await load(viewer, maxU64);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
});

function instantBoundaryFixture() {
  const graph = adderFixture();
  const observer = graph.graph.nodes.find((node) => node.kind.kind === 'observer');
  graph.graph.instant_cells = [{
    id: 'instant_cell:0', actors: [graph.graph.pistons[0].id],
    resources: [graph.graph.resources[0].id], resets: [observer.id],
  }];
  graph.graph.instant_corridors = [{ id: 'instant_corridor:0', wires: [], conductive_sites: [] }];
  graph.graph.instant_declines = [];
  const route = graph.graph.routes.find((row) => row.deliveries.length);
  const siteGuard = graph.graph.guards.find((row) => row.kind === 'site');
  const stateGuard = graph.graph.guards.find((row) => row.kind === 'state');
  const contact = graph.graph.contacts.find((row) => row.variants.length);
  const wireNode = graph.graph.nodes.find((node) => node.kind.kind === 'wire');
  const wirePlan = graph.graph.wire_plans.find((plan) => plan.id === wireNode.kind.plan);
  const capturedRoute = graph.graph.routes.find((candidate) => candidate.id === wirePlan.turbo_route);
  const capturedOrdinal = capturedRoute.deliveries.findIndex((delivery) => delivery.view === 'captured'
    && wirePlan.canonical_neighbors.includes(delivery.target));
  assert.ok(capturedOrdinal >= 0);
  const capturedNeighbor = capturedRoute.deliveries[capturedOrdinal].target;
  const comparatorPlan = {
    id: 'comparator_plan:0', node: graph.graph.nodes[0].id, rear: graph.graph.sites[0].id,
    direct_override: { kind: 'literal', value: 1 },
    far_override: { site: graph.graph.sites[0].id, guard: siteGuard.id, value: { kind: 'live_component_output', node: graph.graph.nodes[0].id } },
  };
  graph.graph.comparator_plans.push(comparatorPlan);
  graph.graph.instant_boundary_ports = [
    { id: 'instant_port:0', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'power', link: graph.graph.power_links[0].id } },
    { id: 'instant_port:1', owner: 'instant_cell:0', direction: 'output', binding: { kind: 'callback', occurrence: { route: route.id, ordinal: 0 } } },
    { id: 'instant_port:2', owner: 'instant_cell:0', direction: 'output', binding: { kind: 'callback', occurrence: { route: route.id, ordinal: 0 } } },
    ...['rear', 'direct_override', 'far_override_site', 'far_override_guard', 'far_override_output'].map((role, index) => ({
      id: `instant_port:${3 + index}`, owner: 'instant_cell:0', direction: 'input',
      binding: { kind: 'comparator', plan: comparatorPlan.id, role },
    })),
    { id: 'instant_port:8', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'guard_site', guard: siteGuard.id, site: siteGuard.site, view: siteGuard.view, test: siteGuard.test } },
    { id: 'instant_port:9', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'guard_state', guard: stateGuard.id, test: stateGuard.test } },
    { id: 'instant_port:10', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'contact_alternative', contact: contact.id, ordinal: 0 } },
    { id: 'instant_port:11', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'observer_watch', observer: observer.id, site: observer.kind.watched } },
    { id: 'instant_port:12', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'captured_wire', wire: wireNode.id, occurrence: { route: capturedRoute.id, ordinal: capturedOrdinal }, neighbor: capturedNeighbor } },
    { id: 'instant_port:13', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'wire_neighborhood', wire: wireNode.id, neighbor: wirePlan.canonical_neighbors[0] } },
    { id: 'instant_port:14', owner: 'instant_cell:0', direction: 'input', binding: { kind: 'fixed_read', node: observer.id, site: graph.graph.sites[0].id, role: 'note_support' } },
  ];
  return graph;
}

function instantLayerFixture() {
  const graph = instantBoundaryFixture();
  const catalogCell = graph.graph.instant_cells[0];
  catalogCell.actors.push(graph.graph.pistons[1].id);
  const route = graph.graph.routes.find((row) => row.deliveries.length >= 2);
  const occurrence = (ordinal) => ({ route: route.id, ordinal });
  const observer = catalogCell.resets[0];
  const actorRoutes = graph.graph.pistons[0].routes;
  const lifecycleOccurrence = (routeId) => ({ route: routeId, ordinal: 0 });
  const watchedSiteId = graph.graph.nodes.find((node) => node.id === observer).kind.watched;
  const watchedContactId = graph.graph.sites.find((site) => site.id === watchedSiteId).source.contact;
  const watchedActorId = graph.graph.contacts.find((contact) => contact.id === watchedContactId).variants
    .find((variant) => variant.receiver?.kind === 'piston_base').receiver.piston;
  const watchedRoutes = graph.graph.pistons.find((piston) => piston.id === watchedActorId).routes;
  const observerSite = graph.graph.sites.find((site) => site.source.kind === 'component' && site.source.node === observer);
  const resetLink = graph.graph.power_links.find((link) => link.from.site === observerSite.id
    && (link.from.port.kind === 'output' || (link.from.port.kind === 'strong' && link.from.port.face === 'bottom'))
    && link.guard === 'guard:0' && link.attenuation === 0);
  graph.graph.instant_boundary_ports.push({
    id: 'instant_port:15', owner: catalogCell.id, direction: 'input',
    binding: { kind: 'callback', occurrence: occurrence(0) },
  });
  graph.graph.instant_layer_regions = [{
    id: 'instant_layer_region:7', stationary_entry: 6, cells: [catalogCell.id], corridors: ['instant_corridor:0'],
    inputs: [
      { role: 'activation_event', source: { kind: 'boundary_event', port: 'instant_port:15', occurrence: occurrence(0) } },
      { role: 'request_sample', source: { kind: 'power_contribution', port: 'instant_port:0', link: graph.graph.power_links[0].id } },
      { role: 'acceptance_revalidation', source: { kind: 'state', test: { kind: 'piston_extended', piston: graph.graph.pistons[0].id } } },
      { role: 'request_sample', source: { kind: 'local_reset_output', observer, link: resetLink.id } },
      { role: 'acceptance_revalidation', source: { kind: 'state', test: { kind: 'resource_enabled', resource: catalogCell.resources[0] } } },
    ],
    outputs: [{ port: 'instant_port:1', emitted_when: 6 }],
    cold_lifecycle_occurrences: [
      { actor: graph.graph.pistons[0].id, phase: 'retraction_base_changed', occurrence: lifecycleOccurrence(actorRoutes.base_changed), reason: { kind: 'inert_cell_callback' } },
      { actor: graph.graph.pistons[0].id, phase: 'retraction_far_changed', occurrence: lifecycleOccurrence(actorRoutes.far_changed), reason: { kind: 'inert_cell_callback' } },
      { actor: graph.graph.pistons[0].id, phase: 'retraction_near_changed', occurrence: lifecycleOccurrence(actorRoutes.near_changed), reason: { kind: 'inert_cell_callback' } },
      { actor: graph.graph.pistons[0].id, phase: 'deferred_base_restoration', occurrence: lifecycleOccurrence(actorRoutes.base_restored), reason: { kind: 'ordinary_graph_lifecycle' } },
      { actor: graph.graph.pistons[0].id, phase: 'deferred_head_restoration', occurrence: lifecycleOccurrence(actorRoutes.head_restored), reason: { kind: 'ordinary_graph_lifecycle' } },
      { actor: graph.graph.pistons[0].id, phase: 'deferred_resource_restoration', occurrence: lifecycleOccurrence(actorRoutes.resource_restored_near), reason: { kind: 'ordinary_graph_lifecycle' } },
      { actor: graph.graph.pistons[0].id, phase: 'reset_observer_lifecycle', occurrence: lifecycleOccurrence(watchedRoutes.base_changed), reason: { kind: 'reset_observer_update', observer } },
      { actor: graph.graph.pistons[0].id, phase: 'reset_output', occurrence: lifecycleOccurrence(graph.graph.nodes.find((node) => node.id === observer).output_route), reason: { kind: 'ordinary_graph_lifecycle' } },
      { actor: graph.graph.pistons[0].id, phase: 'reset_far_shape_change', occurrence: lifecycleOccurrence(actorRoutes.far_shape_changed), reason: { kind: 'ordinary_graph_lifecycle' } },
    ],
    resource_outputs: [{ cell: catalogCell.id, resource: catalogCell.resources[0],
      source: graph.graph.resources[0].contacts[0], destination: graph.graph.resources[0].contacts[1],
      transfer_when: 7, source_absent_after: 8 }],
    expressions: [
      { kind: 'constant', value: true }, { kind: 'input', input: 0 }, { kind: 'input', input: 1 },
      { kind: 'input', input: 2 }, { kind: 'not', value: 0 }, { kind: 'all', values: [1, 4] },
      { kind: 'any', values: [2, 3] }, { kind: 'input', input: 3 }, { kind: 'input', input: 4 },
    ],
    pin_values: [{ cell: catalogCell.id, pin: 0, value: 5 }],
    layers: [{ order: 'fifo', uses: [
      { use_id: 0, actor: graph.graph.pistons[0].id, requested: 1, accepted: 2, consumed_before: 0, consumed_after: 5,
        cause: { kind: 'boundary', occurrences: [occurrence(0), occurrence(1)] } },
      { use_id: 1, actor: graph.graph.pistons[1].id, requested: 2, accepted: 3, consumed_before: 0, consumed_after: 6,
        cause: { kind: 'boundary', occurrences: [occurrence(0), occurrence(1)] } },
    ] }],
  }];
  graph.graph.instant_layer_declines = [];
  return graph;
}

test('loads and selects additive instant boundary records while accepting older exports', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = instantBoundaryFixture();
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate("recordIndex.has('instant_cell:0')"), true);
  assert.equal(viewer.evaluate("recordIndex.has('instant_corridor:0')"), true);
  assert.equal(viewer.evaluate("recordIndex.has('instant_port:2')"), true);
  viewer.evaluate('focusRecord("instant_port:2")');
  assert.match(viewer.elements.get('details').textContent, /instant_port:2/);
  const older = adderFixture();
  for (const table of ['instant_cells', 'instant_corridors', 'instant_boundary_ports', 'instant_declines']) delete older.graph[table];
  await load(viewer, older);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
});

test('layer boundary state requires its matching original input guard', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const data = instantLayerFixture();
  const region = data.graph.instant_layer_regions[0];
  const guard = data.graph.guards.find(guard => guard.kind === 'state');
  data.graph.instant_boundary_ports.push({
    id: 'instant_port:900', owner: region.cells[0], direction: 'input',
    binding: { kind: 'guard_state', guard: guard.id, test: guard.test },
  });
  region.inputs.push({ source: { kind: 'boundary_state', port: 'instant_port:900', test: guard.test }, role: 'request_sample' });
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const previous = viewer.cy;
  for (const port of ['instant_port:999', 'instant_port:0', 'instant_port:2']) {
    const damaged = structuredClone(data);
    damaged.graph.instant_layer_regions[0].inputs.at(-1).source.port = port;
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});

test('queued layer requests require the matching owned actor callback', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const data = instantLayerFixture();
  const actor = data.graph.pistons[0].id;
  const region = data.graph.instant_layer_regions[0];
  const route = data.graph.routes.find(route => route.deliveries.some(delivery =>
    delivery.kind === 'update' && delivery.receiver?.kind === 'piston_base' && delivery.receiver.piston === actor));
  const ordinal = route.deliveries.findIndex(delivery =>
    delivery.kind === 'update' && delivery.receiver?.kind === 'piston_base' && delivery.receiver.piston === actor);
  const event = { route: route.id, ordinal };
  const port = data.graph.instant_boundary_ports.find(port => port.id === region.inputs[0].source.port);
  port.binding.occurrence = event;
  region.inputs[0] = { role: 'activation_event', source: { kind: 'boundary_request', actor, port: port.id, occurrence: event } };
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const contact = data.graph.contacts.find(contact => contact.variants.some(variant =>
    variant.receiver?.kind === 'piston_base' && variant.receiver.piston === actor));
  const directReceiver = route.deliveries[ordinal].receiver;
  route.deliveries[ordinal].receiver = { kind: 'contact', contact: contact.id };
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  route.deliveries[ordinal].receiver = directReceiver;
  const previous = viewer.cy;
  for (const damage of [
    input => { input.source.actor = data.graph.pistons[1].id; },
    input => { input.source.actor = 'piston:999'; },
    input => { input.source.port = 'instant_port:0'; },
    input => { input.role = 'acceptance_revalidation'; },
  ]) {
    const damaged = structuredClone(data);
    damage(damaged.graph.instant_layer_regions[0].inputs[0]);
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
});

test('layer Graph observations validate boundary references and retain older exports', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const data = instantLayerFixture();
  const region = data.graph.instant_layer_regions[0];
  const wire = data.graph.nodes.find(node => node.kind.kind === 'wire');
  const plan = data.graph.wire_plans.find(plan => plan.id === wire.kind.plan);
  data.graph.instant_boundary_ports.push({
    id: 'instant_port:900', owner: region.cells[0], direction: 'output',
    binding: { kind: 'wire_neighborhood', wire: wire.id, neighbor: plan.canonical_neighbors[0] },
  });
  region.graph_outputs = ['instant_port:900'];
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const previous = viewer.cy;
  for (const invalid of [['instant_port:900', 'instant_port:900'], ['instant_port:999'],
    ['instant_port:0'], ['instant_port:2'], 'instant_port:900']) {
    const damaged = structuredClone(data);
    damaged.graph.instant_layer_regions[0].graph_outputs = invalid;
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous, 'malformed input must preserve the loaded graph');
  }
});

test('layer threshold inputs validate direction, strength and internal wire ownership', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const data = instantLayerFixture();
  const observer = data.graph.instant_cells[0].resets[0];
  const site = data.graph.sites.find(site => site.source.kind === 'component' && site.source.node === observer).id;
  const source = { kind: 'source_above', site, port: { kind: 'strong', face: 'bottom' }, threshold: 3 };
  data.graph.instant_layer_regions[0].inputs.push({ source, role: 'request_sample' });
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  const previous = viewer.cy;
  for (const replacement of [{ threshold: -1 }, { threshold: 15 }, { threshold: true }, { site: 'site:999999' },
    { port: { kind: 'weak', face: 'diagonal' } }, { port: { kind: 'unknown' } }]) {
    const damaged = structuredClone(data);
    Object.assign(damaged.graph.instant_layer_regions[0].inputs.at(-1).source, replacement);
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
  const wire = data.graph.nodes.find(node => node.kind.kind === 'wire').id;
  const wireSite = data.graph.sites.find(site => site.source.kind === 'component' && site.source.node === wire).id;
  data.graph.instant_corridors[0].wires = [wire];
  source.site = wireSite;
  await load(viewer, data);
  assert.equal(viewer.elements.get('error').hidden, false);
  assert.match(viewer.elements.get('error').textContent, /owned internal wire/);
});

test('loads causal-layer records and keeps local expressions and callback causes distinct', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = instantLayerFixture();
  const actorRoutes = graph.graph.pistons[0].routes;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate("recordIndex.has('instant_layer_region:7')"), true);
  viewer.evaluate('focusRecord("instant_layer_region:7")');
  assert.match(viewer.elements.get('details').textContent, /instant_layer_region:7/);

  const transfers = graph.graph.instant_layer_regions[0].resource_outputs;
  transfers.push({ ...transfers[0], transfer_when: 4 });
  const uses = graph.graph.instant_layer_regions[0].layers[0].uses;
  uses.push({ ...uses[0], accepted: 3 });
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);

  const previous = viewer.cy;
  for (const corrupt of [
    (row) => { row.cells = ['instant_cell:999']; },
    (row) => { row.expressions[4].value = 4; },
    (row) => { row.stationary_entry = 99; },
    (row) => { row.outputs[0].emitted_when = 99; },
    (row) => { row.inputs[0].source.port = 'instant_port:999'; },
    (row) => { row.inputs[1].source.link = 'power_link:999'; },
    (row) => { row.inputs[2].source.test.piston = 'resource:0'; },
    (row) => { row.inputs[3].source.observer = 'node:999'; },
    (row) => { row.inputs[3].source.link = 'power_link:0'; },
    (row) => { row.inputs[4].source.test.resource = 'resource:999'; },
    (row) => { row.resource_outputs[0].source = 'contact:999'; },
    (row) => { row.resource_outputs = []; },
    (row) => { row.resource_outputs[0].destination = row.resource_outputs[0].source; },
    (row) => { row.cold_lifecycle_occurrences[0].occurrence.ordinal = 99; },
    (row) => { row.cold_lifecycle_occurrences[0].reason.kind = 'base_pending_self_update'; },
    (row) => { row.cold_lifecycle_occurrences[6].reason.observer = 'node:999'; },
    (row) => { row.cold_lifecycle_occurrences[3].phase = 'deferred_resource_restoration'; },
    (row) => { row.cold_lifecycle_occurrences[7].occurrence.route = actorRoutes.base_changed; },
    (row) => { row.cold_lifecycle_occurrences[8].occurrence.route = actorRoutes.far_changed; },
    (row) => { row.layers[0].uses[0].actor = 'piston:999'; },
    (row) => { row.layers[0].uses.push(structuredClone(row.layers[0].uses[0])); },
  ]) {
    const damaged = structuredClone(graph);
    corrupt(damaged.graph.instant_layer_regions[0]);
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
  for (const [lifecycleIndex, wrongSource] of [
    [7, graph.graph.pistons[0].node],
    [8, graph.graph.instant_cells[0].resets[0]],
  ]) {
    const damaged = structuredClone(graph);
    const occurrence = damaged.graph.instant_layer_regions[0].cold_lifecycle_occurrences[lifecycleIndex].occurrence;
    damaged.graph.routes.find((route) => route.id === occurrence.route).source = wrongSource;
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }
  const declined = structuredClone(graph);
  declined.graph.instant_layer_regions = [];
  declined.graph.instant_layer_declines = [{
    id: 'instant_layer_decline:4', cells: ['instant_cell:0'], corridors: ['instant_corridor:0'],
    witness: { kind: 'actor', piston: graph.graph.pistons[0].id }, reason: 'shared_owner_order_unknown',
  }];
  await load(viewer, declined);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate("recordIndex.has('instant_layer_decline:4')"), true);
  viewer.evaluate('focusRecord("instant_layer_decline:4")');
  assert.match(viewer.elements.get('details').textContent, /instant_layer_decline:4/);
  declined.graph.instant_layer_declines[0].reason = 'proof_budget_exceeded';
  await load(viewer, declined);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
  assert.equal(viewer.evaluate("recordIndex.has('instant_layer_decline:4')"), true);
  const older = structuredClone(graph);
  delete older.graph.instant_layer_regions;
  delete older.graph.instant_layer_declines;
  await load(viewer, older);
  assert.equal(viewer.elements.get('error').hidden, true, viewer.elements.get('error').textContent);
});

test('rejects malformed instant boundary tables, ownership, and dangling bindings', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, instantBoundaryFixture());
  const previous = viewer.cy;
  const corruptions = [
    (graph) => { graph.graph.instant_cells.push({ ...graph.graph.instant_cells[0], id: 'instant_cell:1' }); },
    (graph) => { graph.graph.instant_cells[0].actors = ['node:0']; },
    (graph) => { graph.graph.instant_cells[0].resets = ['node:0']; },
    (graph) => { graph.graph.instant_corridors[0].wires = ['node:0']; },
    (graph) => { graph.graph.instant_corridors[0].wires = ['node:999']; },
    (graph) => { graph.graph.instant_corridors[0].conductive_sites = [graph.graph.sites[0].id]; graph.graph.instant_corridors.push({ id: 'instant_corridor:1', wires: [], conductive_sites: [graph.graph.sites[0].id] }); },
    (graph) => { graph.graph.instant_boundary_ports[0].owner = 'instant_cell:999'; },
    (graph) => { graph.graph.instant_boundary_ports[0].direction = 'sideways'; },
    (graph) => { graph.graph.instant_boundary_ports[0].binding.link = 'power_link:999'; },
    (graph) => { graph.graph.comparator_plans[0].far_override.value.kind = 'literal'; },
    (graph) => { graph.graph.instant_declines.push({ id: 'instant_decline:0', actors: graph.graph.instant_cells[0].actors, reason: { code: 'unsupported_case' } }); },
    (graph) => { graph.graph.instant_regions = [{ id: 'instant_region:0', wires: [], actors: [], resources: [], resets: [], contacts: [], readable_sites: [], power_inputs: [], power_readers: [], routes: [], wire_readers: [], observers: [], incoming: [], outgoing: [] }]; },
  ];
  for (const corrupt of corruptions) {
    const damaged = structuredClone(instantBoundaryFixture());
    corrupt(damaged);
    await load(viewer, damaged);
    assert.equal(viewer.elements.get('error').hidden, false);
    assert.equal(viewer.cy, previous);
  }

  const classicOwned = instantBoundaryFixture();
  const classicWire = classicOwned.graph.nodes.find((node) => node.kind.kind === 'wire').id;
  classicOwned.graph.classic_nets = [{ id: 'classic_net:0', source: classicOwned.graph.nodes[0].id, initial_signal: 0,
    wires: [{ node: classicWire, attenuation: 0 }], inputs: [], entries: [] }];
  classicOwned.graph.instant_corridors[0].wires = [classicWire];
  await load(viewer, classicOwned);
  assert.equal(viewer.elements.get('error').hidden, false);
  assert.equal(viewer.cy, previous);
});

test('instant wire node masks are optional and bounded', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const wire = graph.graph.wire_plans[0];
  wire.instant_node = { receivers: 1 };
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  for (const receivers of [0, -1, 0x1000000, true, '1']) {
    wire.instant_node = { receivers };
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
  delete wire.instant_node;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
});

test('BUD reads retain typed owners and original update scope', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture(), rows = graph.graph;
  rows.bud_memory_reads = [{id:'bud_memory_read:0',storage:rows.pistons[1].id,reader:rows.pistons[0].id,
    resource:rows.resources[0].id,source:rows.contacts[0].id,link:rows.power_links[0].id,updates:'original_graph_fifo',read:'enabled_at_source'}];
  await load(viewer,graph);
  assert.equal(viewer.elements.get('error').hidden,true);
  for (const [field,value] of [['storage',rows.pistons[0].id],['reader','node:0'],['resource','resource:99999'],['updates','erased'],['read','saved_pose']]) {
    const bad = structuredClone(graph);
    bad.graph.bud_memory_reads[0][field] = value;
    await load(viewer,bad);
    assert.equal(viewer.elements.get('error').hidden,false);
  }
});

test('event kernels validate ownership and ordered predicate operands', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const actor = graph.graph.pistons[0].id;
  graph.graph.instant_event_kernels = [{id:'instant_event_kernel:0',actors:[actor],wire_propagation_removed:false,
    entries:[{actor,valid:1,pins:[{kind:'state',test:{kind:'piston_extended',piston:actor}},{kind:'gate',gate:'invert',inputs:[0]}]}]}];
  await load(viewer,graph);
  assert.equal(viewer.elements.get('error').hidden,true);
  const mutations = [
    (kernel) => {kernel.wire_propagation_removed = true;},
    (kernel) => {kernel.actors.push(actor);},
    (kernel) => {kernel.entries[0].valid = 2;},
    (kernel) => {kernel.entries[0].pins[1].inputs = [1];},
    (kernel) => {kernel.entries[0].pins[0].kind = 'activation';},
    (kernel) => {kernel.entries[0].pins[0].test.piston = 'node:0';},
  ];
  for (const mutate of mutations) {
    const bad = structuredClone(graph);
    mutate(bad.graph.instant_event_kernels[0]);
    await load(viewer,bad);
    assert.equal(viewer.elements.get('error').hidden,false);
  }
});

test('wire capture operations are optional and typed', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  for (const value of [null, 'fixed', 'electrical', 'bulb', 'wire', 'piston', 'contact']) {
    graph.graph.sites[0].wire_capture = value;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, true);
  }
  for (const value of [false, 1, {}, [], 'saved_pose']) {
    graph.graph.sites[0].wire_capture = value;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
});

test('abstract instant net flags are explicit and typed', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture();
  const wire = graph.graph.wire_plans[0];
  wire.abstract_instant_net = true;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, true);
  for (const value of [0, 1, 'true', {}]) {
    wire.abstract_instant_net = value;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
});

test('mutable capture masks require 24 bits and a compiled root', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  const graph = adderFixture(), wire = graph.graph.wire_plans[0];
  wire.hop_root = graph.graph.sites.find((site) => site.source.kind === 'component' && site.source.node === wire.node).id;
  for (const value of [0, 0xffffff]) {
    wire.mutable_captures = value;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, true);
  }
  for (const value of [-1, 0x1000000, 0.5, true, '1', [], {}]) {
    wire.mutable_captures = value;
    await load(viewer, graph);
    assert.equal(viewer.elements.get('error').hidden, false);
  }
  wire.mutable_captures = 0;
  delete wire.hop_root;
  await load(viewer, graph);
  assert.equal(viewer.elements.get('error').hidden, false);
});
