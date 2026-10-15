const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

class Element {
  constructor() {
    this.listeners = new Map();
    this.children = [];
    this.hidden = false;
    this.value = "";
    this.textContent = "";
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
  const context = vm.createContext({ document, setTimeout, clearTimeout });
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
  assert.equal(viewer.cy.edges('.enabled').length, 168);
  assert.equal(viewer.cy.edges('.disabled').length, 7);
  assert.equal(viewer.cy.edges('.unknown').length, 30);
  viewer.elements.get('hide-inactive').checked = true;
  viewer.elements.get('hide-inactive').dispatch('change');
  assert.equal(viewer.cy.edges('.disabled.filtered').length, 7);
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
});

test('rejects malformed wire query fact masks while preserving the loaded graph', async (t) => {
  const viewer = makeViewer();
  t.after(() => viewer.cy?.destroy());
  await load(viewer, adderFixture());
  const previous = viewer.cy;
  const valid = { known: 0xffffff, solid: 1, transparent: 2, wire: 4, zero_no_dust: 0xfffffe };
  const invalid = [[], false, 'masks', {}, { ...valid, known: 0 }, { ...valid, zero_no_dust: 1 }];
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
