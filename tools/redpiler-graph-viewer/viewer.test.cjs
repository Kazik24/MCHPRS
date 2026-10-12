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
  });
}
