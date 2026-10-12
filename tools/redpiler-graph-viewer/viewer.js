"use strict";

cytoscapeExpandCollapse(cytoscape);

const tables = {
  nodes: "node", sites: "site", contacts: "contact", resources: "resource", guards: "guard",
  pistons: "piston", power_links: "power_link", wire_plans: "wire", wire_relations: "wire_relation",
  comparator_plans: "comparator_plan", routes: "route", profiles: "profile",
};
const $ = (id) => document.getElementById(id);
let cy;
let recordIndex;
let graphData;
let connections;
let neighbors;
let groups = [];
const MAX_VIEW_NODES = 200;
const MAX_VIEW_EDGES = 400;

function typedIds(value, found = []) {
  if (typeof value === "string" && /^[a-z_]+:/.test(value)) {
    if (!value.startsWith("snapshot:") && !/^[a-z_]+:(0|[1-9][0-9]*)$/.test(value)) throw Error(`Malformed typed ID: ${value}`);
    found.push(value);
  }
  else if (Array.isArray(value)) value.forEach((child) => typedIds(child, found));
  else if (value && typeof value === "object") Object.values(value).forEach((child) => typedIds(child, found));
  return found;
}

function validateGraph(data) {
  if (data?.format !== "mchprs.redpiler.graph") throw Error("Unsupported graph format.");
  if (!Number.isInteger(data.version?.major) || data.version.major !== 1) throw Error(`Unsupported graph major version: ${data.version?.major}`);
  if (!data.graph || !Array.isArray(data.snapshots)) throw Error("Graph tables or snapshots are missing.");
  const ids = new Set();
  for (const [table, prefix] of Object.entries(tables)) {
    const rows = data.graph[table];
    if (table === "routes" && rows === null) continue;
    if (!Array.isArray(rows)) throw Error(`graph.${table} must be an array or null for routes.`);
    for (const row of rows) {
      if (!row || typeof row.id !== "string" || !new RegExp(`^${prefix}:(0|[1-9][0-9]*)$`).test(row.id)) throw Error(`Invalid ID in graph.${table}.`);
      if (ids.has(row.id)) throw Error(`Duplicate ID: ${row.id}`);
      ids.add(row.id);
    }
  }
  for (const snapshot of data.snapshots) {
    if (typeof snapshot.id !== "string" || !snapshot.id.startsWith("snapshot:") || ids.has(snapshot.id)) throw Error(`Invalid or duplicate snapshot ID: ${snapshot.id}`);
    ids.add(snapshot.id);
  }
  for (const ref of typedIds(data)) if (!ids.has(ref)) throw Error(`Dangling graph reference: ${ref}`);
  return data;
}

function indexRecords(data) {
  const index = new Map();
  for (const rows of Object.values(data.graph)) if (Array.isArray(rows)) for (const row of rows) index.set(row.id, row);
  for (const snapshot of data.snapshots) index.set(snapshot.id, snapshot);
  return index;
}

function category(node) {
  if (node.group === "sites") return "site";
  if (node.group === "routes") return "route";
  return (node.kind?.kind || "other").toLowerCase();
}

function sourceNode(siteId, index) {
  const site = index.get(siteId);
  return site?.source?.kind === "component" ? site.source.node : siteId;
}

function indexConnections(data, index) {
  const edges = new Map();
  const adjacent = new Map();
  function add(id, source, target, type, record) {
    const connection = { id, source, target, type, record };
    edges.set(id, connection);
    for (const node of new Set([source, target])) {
      if (!adjacent.has(node)) adjacent.set(node, []);
      adjacent.get(node).push(connection);
    }
  }
  for (const link of data.graph.power_links) {
    const target = link.to.kind === "input" ? link.to.node : link.to.site;
    add(link.id, sourceNode(link.from.site, index), target, "power_link", link);
  }
  for (const relation of data.graph.wire_relations) {
    add(relation.id, relation.from, relation.to, "wire_relation", relation);
  }
  for (const route of data.graph.routes || []) {
    route.deliveries.forEach((delivery, index) => {
      add(`${route.id}:delivery:${index}`, route.id, delivery.target, "route_delivery", delivery);
    });
  }
  return { edges, adjacent };
}

function selectNeighborhood(id) {
  const selectedNodes = new Set();
  const selectedEdges = [];
  const selectedConnection = connections.get(id);
  const center = selectedConnection ? selectedConnection.source : id;
  if (center) selectedNodes.add(center);

  function include(connection) {
    selectedNodes.add(connection.source);
    selectedNodes.add(connection.target);
    selectedEdges.push(connection);
  }
  if (selectedConnection) include(selectedConnection);
  for (const connection of neighbors.get(center) || []) {
    if (selectedNodes.size >= MAX_VIEW_NODES || selectedEdges.length >= MAX_VIEW_EDGES) break;
    if (connection !== selectedConnection) include(connection);
  }
  return { nodes: selectedNodes, edges: selectedEdges };
}

function connectionLabel(connection) {
  const record = connection.record;
  if (connection.type === "power_link") {
    return `${record.from.site} · ${record.face} · −${record.attenuation} · ${record.guard}`;
  }
  if (connection.type === "wire_relation") return `${record.kind} · ${record.direction}`;
  const direction = record.direction ? ` · ${record.direction}` : "";
  return `${record.ordinal} · ${record.kind}${direction}`;
}

function graphElements(view, index) {
  const elements = [];
  const groupIds = new Set();
  const addGroup = (id, label) => {
    if (!groupIds.has(id)) {
      elements.push({ data: { id, label, isGroup: true } });
      groupIds.add(id);
    }
  };
  const addRow = (row, type) => {
    const label = row.kind?.kind || row.kind?.type || row.name || type;
    const group = type === "site" ? "Sites" : type === "route" ? "Routes" : label;
    const parent = `group:${group.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
    addGroup(parent, group);
    elements.push({ data: { id: row.id, parent, label: row.id, type, kind: label, searchable: JSON.stringify(row).toLowerCase() } });
  };
  for (const id of view.nodes) addRow(index.get(id), id.split(":")[0]);
  for (const connection of view.edges) {
    elements.push({ data: {
      id: connection.id, source: connection.source, target: connection.target,
      type: connection.type, label: connectionLabel(connection),
      searchable: JSON.stringify(connection.record).toLowerCase(),
    } });
  }
  return elements;
}

function controls(data) {
  const nodeKinds = [...new Set(["site", "route", ...data.graph.nodes.map(category)])].sort();
  $("node-filters").replaceChildren(...nodeKinds.map((kind) => check(kind, kind[0].toUpperCase() + kind.slice(1), true, "node-filter")));
  const edgeTypes = ["power_link", "wire_relation", "route_delivery"];
  $("edge-filters").replaceChildren(...edgeTypes.map((kind) => check(kind, kind.replaceAll("_", " "), kind !== "route_delivery" || Array.isArray(data.graph.routes), "edge-filter")));
  document.querySelectorAll(".node-filter, .edge-filter").forEach((input) => input.addEventListener("change", applyFilters));
}

function check(value, label, checked, className) {
  const wrapper = document.createElement("label");
  const input = document.createElement("input");
  input.type = "checkbox"; input.value = value; input.checked = checked; input.className = className;
  wrapper.append(input, document.createTextNode(label));
  return wrapper;
}

function initGraph(data) {
  recordIndex = indexRecords(data);
  graphData = data;
  const topology = indexConnections(data, recordIndex);
  connections = topology.edges;
  neighbors = topology.adjacent;
  controls(data);
  const first = data.graph.nodes[0]?.id || data.graph.sites[0]?.id || data.graph.routes?.[0]?.id;
  renderNeighborhood(first);
  $("summary").textContent = `v${data.version.major}.${data.version.minor} · ${data.stage} · ${data.capabilities?.join(", ") || "no capabilities listed"}`;
  $("drop-hint").hidden = true;
  $("matches").replaceChildren();
  $("details").textContent = "Search for a record to center its neighborhood, or click a record to inspect it.";
}

function renderNeighborhood(id) {
  const view = selectNeighborhood(id);
  const elements = graphElements(view, recordIndex);
  if (cy) cy.destroy();
  cy = cytoscape({
    container: $("graph"), elements, layout: { name: "cose", animate: false, nodeRepulsion: 5000, idealEdgeLength: 75, padding: 40 },
    style: [
      { selector: "node", style: { label: "data(label)", color: "#e6edf3", "font-size": 9, "text-valign": "center", "text-halign": "center", "background-color": "#3d8eaa", width: 28, height: 28, "border-width": 1, "border-color": "#a8d8e8" } },
      { selector: "node[type = 'site']", style: { shape: "diamond", "background-color": "#9c78c6", "border-color": "#d1b9ed", width: 17, height: 17, "font-size": 7 } },
      { selector: "node[type = 'route']", style: { shape: "round-rectangle", "background-color": "#c28b43" } },
      { selector: "node[?isGroup]", style: { shape: "round-rectangle", "background-color": "#263746", "background-opacity": 0.45, "border-color": "#587185", "border-width": 2, "font-size": 12, padding: 16, "text-valign": "top", "text-halign": "center", color: "#b8cbd9", label: "data(label)" } },
      { selector: "edge", style: { width: 1, "line-color": "#4d6374", "target-arrow-color": "#8299aa", "target-arrow-shape": "triangle", "curve-style": "bezier", "arrow-scale": 0.7, label: "data(label)", color: "#9eb0bd", "font-size": 6, "text-background-color": "#111820", "text-background-opacity": 0.8, "text-background-padding": 1 } },
      { selector: "edge[type = 'wire_relation']", style: { "line-color": "#77ae72", "target-arrow-color": "#77ae72", "line-style": "dashed" } },
      { selector: "edge[type = 'route_delivery']", style: { "line-color": "#c28b43", "target-arrow-color": "#c28b43" } },
      { selector: ".search-hit", style: { "border-width": 4, "border-color": "#ffe16b", "background-color": "#db923f", "z-index": 10 } },
      { selector: ".filtered", style: { display: "none" } },
    ],
  });
  cy.expandCollapse({ layoutBy: { name: "cose", animate: false, randomize: false }, fisheye: false, animate: false, undoable: false });
  groups = cy.nodes("[?isGroup]");
  cy.on("tap", "node", (event) => inspect(event.target.id()));
  cy.on("tap", "edge", (event) => inspect(event.target.id()));
  applyFilters();
  const data = graphData;
  $("counts").textContent = `${data.graph.nodes.length} total nodes · ${data.graph.sites.length} sites · routes ${data.graph.routes === null ? "unavailable" : data.graph.routes.length} · trace ${data.trace === null ? "unavailable" : data.trace.length} · view ${view.nodes.size} nodes, ${view.edges.length} connections around ${id || "empty graph"}`;
}

function applyFilters() {
  if (!cy) return;
  const visibleNodes = new Set([...document.querySelectorAll(".node-filter:checked")].map((input) => input.value));
  const visibleEdges = new Set([...document.querySelectorAll(".edge-filter:checked")].map((input) => input.value));
  cy.nodes().forEach((node) => {
    const kind = node.data("isGroup") ? "group" : node.data("type") === "site" ? "site" : node.data("type") === "route" ? "route" : node.data("kind");
    node.toggleClass("filtered", !node.data("isGroup") && !visibleNodes.has(kind));
  });
  cy.edges().forEach((edge) => edge.toggleClass("filtered", !visibleEdges.has(edge.data("type")) || edge.source().hasClass("filtered") || edge.target().hasClass("filtered")));
}

function inspect(id) {
  const record = recordIndex.get(id) || connections.get(id)?.record;
  $("details").textContent = record ? `${id}\n${JSON.stringify(record, null, 2)}` : JSON.stringify(cy.getElementById(id).data(), null, 2);
}

function search() {
  if (!cy) return;
  const query = $("search").value.trim().toLowerCase();
  $("matches").replaceChildren();
  if (!query) return;
  const matches = [...recordIndex.entries()].filter(([id, row]) => id.toLowerCase().includes(query) || JSON.stringify(row).toLowerCase().includes(query)).slice(0, 30);
  for (const [id] of matches) {
    const button = document.createElement("button");
    button.className = "match"; button.textContent = id;
    button.addEventListener("click", () => focusRecord(id));
    $("matches").append(button);
  }
}

function focusRecord(id) {
  if (/^(node|site|route):/.test(id) || connections.has(id)) renderNeighborhood(id);
  const element = cy.getElementById(id);
  if (element.empty()) { inspect(id); return; }
  const api = cy.expandCollapse("get");
  api.expandAll();
  cy.elements().removeClass("search-hit");
  element.removeClass("filtered").addClass("search-hit");
  const connected = element.closedNeighborhood();
  connected.removeClass("filtered");
  cy.fit(connected, 70);
  inspect(id);
}

function relaxGraph() {
  if (!cy) return;
  cy.elements(":visible").layout({
    name: "cose", animate: true, animationDuration: 900, randomize: false, fit: false,
    numIter: 500, nodeRepulsion: 5000, idealEdgeLength: 75, componentSpacing: 50, padding: 40,
  }).run();
}

async function readFile(file) {
  if (!file) return;
  $("error").hidden = true;
  try {
    if (file.size > 64 * 1024 * 1024) throw Error("Graph file exceeds the 64 MiB limit.");
    const data = validateGraph(JSON.parse(await file.text()));
    initGraph(data);
  } catch (error) {
    $("error").textContent = error.message || String(error);
    $("error").hidden = false;
  }
}

$("file").addEventListener("change", (event) => readFile(event.target.files[0]));
$("find").addEventListener("click", search);
$("search").addEventListener("keydown", (event) => { if (event.key === "Enter") search(); });
$("collapse").addEventListener("click", () => cy?.expandCollapse("get").collapse(groups));
$("expand").addEventListener("click", () => cy?.expandCollapse("get").expandAll());
$("relax").addEventListener("click", relaxGraph);
$("graph").addEventListener("dragover", (event) => event.preventDefault());
$("graph").addEventListener("drop", (event) => { event.preventDefault(); readFile(event.dataTransfer.files[0]); });
