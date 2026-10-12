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
    querySelectorAll() {
      return [];
    },
  };
  let cytoscapeOptions;
  const cytoscape = (options) => {
    cytoscapeOptions = options;
    const wrap = (element) => ({
      data(key) {
        return element.data[key];
      },
      toggleClass() {},
      hasClass() {
        return false;
      },
      source() {
        return wrap(elementById.get(element.data.source));
      },
      target() {
        return wrap(elementById.get(element.data.target));
      },
    });
    const elementById = new Map(options.elements.map((element) => [element.data.id, element]));
    const collection = (items) => ({
      forEach(callback) {
        items.forEach((item) => callback(wrap(item)));
      },
      removeClass() {},
      addClass() {},
    });
    return {
      destroy() {},
      expandCollapse() {
        return { expandAll() {}, collapse() {} };
      },
      nodes() {
        return collection(options.elements.filter((element) => !("source" in element.data)));
      },
      edges() {
        return collection(options.elements.filter((element) => "source" in element.data));
      },
      elements() {
        return collection(options.elements);
      },
      on() {},
      getElementById() {
        return { empty: () => true };
      },
      fit() {},
    };
  };
  const source = fs.readFileSync(path.join(__dirname, "viewer.js"), "utf8");
  vm.runInNewContext(source, { document, cytoscape, cytoscapeExpandCollapse() {} });
  return { elements, get cytoscapeOptions() { return cytoscapeOptions; } };
}

for (const count of [1, 20_001]) {
  test(`loads ${count} nodes and sends only a bounded view to the layout`, async () => {
    const viewer = makeViewer();
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
    assert.ok(viewer.cytoscapeOptions.elements.length <= 20_000);
    if (count > 1) {
      const last = `node:${count - 1}`;
      assert.ok(!viewer.cytoscapeOptions.elements.some((element) => element.data.id === last));
      viewer.elements.get("search").value = last;
      viewer.elements.get("find").dispatch("click");
      viewer.elements.get("matches").children[0].dispatch("click");
      assert.ok(viewer.cytoscapeOptions.elements.some((element) => element.data.id === last));
    }
  });
}

test("a high-degree fixture keeps the layout bounded and every edge endpoint present", async () => {
  const viewer = makeViewer();
  const document = JSON.parse(fs.readFileSync(path.join(__dirname, "../../docs/notes/redpiler-pistons-v4/fixtures/adder-1bit.graph.json"), "utf8"));
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
  assert.ok(nodes.size <= 200 + Object.keys(document.graph).length);
  assert.ok(edges.length <= 400);
  for (const edge of edges) {
    assert.ok(nodes.has(edge.data.source));
    assert.ok(nodes.has(edge.data.target));
  }
});
