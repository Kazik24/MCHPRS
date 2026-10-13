"use strict";

const tables = {
  nodes: "node", sites: "site", contacts: "contact", resources: "resource", guards: "guard",
  pistons: "piston", power_links: "power_link", wire_plans: "wire", wire_relations: "wire_relation",
  comparator_plans: "comparator_plan", callback_links: "callback_link", routes: "route", profiles: "profile",
};
const $ = (id) => document.getElementById(id);
let cy;
let recordIndex;
let graphData;
let connections;
let neighbors;
let reverseRefs;
let selectedId;
let viewCenter;
let currentView;
let guardStates = new Map();
const MAX_VIEW_NODES = 400;
const MAX_VIEW_EDGES = 800;

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
    if (table === "callback_links" && rows === undefined) continue;
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
  return node.kind?.component?.type || node.kind?.kind || node.id.split(":")[0];
}

function sourceNode(siteId, index) {
  const site = index.get(siteId);
  if (site?.source?.kind === "component") return site.source.node;
  if (site?.source?.kind === "contact") return index.get(site.source.contact).node;
  return siteId;
}

function indexConnections(data, index) {
  const edges = new Map();
  const adjacent = new Map();
  const routeOwners = new Map();
  function ownRoute(route, node) {
    if (!route) return;
    if (!routeOwners.has(route)) routeOwners.set(route, new Set());
    routeOwners.get(route).add(node);
  }
  for (const node of data.graph.nodes) ownRoute(node.output_route, node.id);
  for (const piston of data.graph.pistons) {
    for (const route of Object.values(piston.routes || {})) ownRoute(route, piston.node);
  }
  function add(id, source, target, type, record) {
    const connection = { id, source, target, type, record };
    edges.set(id, connection);
    for (const node of new Set([source, target])) {
      if (!adjacent.has(node)) adjacent.set(node, []);
      adjacent.get(node).push(connection);
    }
  }
  for (const link of data.graph.power_links) {
    const target = link.to.kind === "input" ? link.to.node : sourceNode(link.to.site, index);
    add(link.id, sourceNode(link.from.site, index), target, "power_link", link);
  }
  for (const relation of data.graph.wire_relations) {
    add(relation.id, relation.from, sourceNode(relation.to, index), "wire_relation", relation);
  }
  for (const route of data.graph.routes || []) {
    route.deliveries.forEach((delivery, ordinal) => {
      const owners = [...(routeOwners.get(route.id) || [route.id])];
      if (delivery.receiver) return;
      owners.forEach((owner, ownerIndex) => {
        const suffix = owners.length > 1 ? `:owner:${ownerIndex}` : "";
        add(`${route.id}:delivery:${ordinal}${suffix}`, owner, sourceNode(delivery.target, index), "route_delivery", delivery);
      });
    });
  }
  for (const link of data.graph.callback_links || []) {
    add(link.id, link.from, link.to, "callback_link", link);
  }
  return { edges, adjacent };
}

function enabledEdgeTypes() {
  return new Set([...document.querySelectorAll('.edge-filter:checked')].map((input) => input.value));
}

function selectView(center, overview, edgeTypes, depth) {
  const nodes = new Set();
  const edges = new Map();
  let truncated = false;
  function include(connection) {
    if (!edgeTypes.has(connection.type)) return;
    const added = [connection.source, connection.target].filter((id) => !nodes.has(id));
    if (nodes.size + new Set(added).size > MAX_VIEW_NODES || (!edges.has(connection.id) && edges.size >= MAX_VIEW_EDGES)) {
      truncated = true;
      return;
    }
    added.forEach((id) => nodes.add(id));
    edges.set(connection.id, connection);
  }
  if (overview) {
    for (const row of graphData.graph.nodes) {
      if (nodes.size >= MAX_VIEW_NODES) { truncated = true; break; }
      nodes.add(row.id);
    }
    for (const connection of connections.values()) include(connection);
  } else {
    const selectedConnection = connections.get(center);
    if (selectedConnection) include(selectedConnection);
    else if (center?.startsWith('resource:')) {
      for (const id of recordIndex.get(center).contacts) {
        if (nodes.size >= MAX_VIEW_NODES) { truncated = true; break; }
        nodes.add(recordIndex.get(id).node);
      }
    }
    else if (center) nodes.add(center);
    let frontier = [...nodes];
    for (let hop = 0; hop < depth; hop++) {
      const before = new Set(nodes);
      for (const id of frontier) for (const connection of neighbors.get(id) || []) include(connection);
      frontier = [...nodes].filter((id) => !before.has(id));
    }
    // Include links between the discovered nodes, including duplicate connections.
    for (const id of nodes) for (const connection of neighbors.get(id) || []) {
      if (nodes.has(connection.source) && nodes.has(connection.target)) include(connection);
    }
  }
  return { nodes, edges: [...edges.values()], truncated };
}

function connectionLabel(connection) {
  const record = connection.record;
  if (connection.type === 'power_link') {
    const port = record.to.port || `conductor (dust ${record.to.include_dust ? 'included' : 'excluded'})`;
    const source = record.from.port;
    const sourcePort = `${source.kind}${source.kind === 'conducted' ? ` (dust ${source.include_dust ? 'included' : 'excluded'})` : source.face ? ` (${source.face})` : ''}`;
    return `${sourcePort} → ${port} · ${record.face} · −${record.attenuation}`;
  }
  if (connection.type === 'wire_relation') return `${record.kind} · ${record.direction}`;
  if (connection.type === 'callback_link') return `${record.kind}${record.direction ? ` · ${record.direction}` : ''} · ${record.route} #${record.sequence}`;
  return `#${record.ordinal} · ${record.kind}${record.direction ? ` · ${record.direction}` : ''} · ${record.view}`;
}

function recordLabel(row) {
  if (row.id.startsWith('site:')) {
    const source = row.source;
    return source.kind === 'constant' ? `${recordIndex.get(source.profile).name} site` : `${source.kind} site`;
  }
  return row.name || (typeof row.kind === 'string' ? row.kind : category(row));
}

function graphElements(view) {
  return [
    ...[...view.nodes].map((id) => {
      const row = recordIndex.get(id);
      const kind = recordLabel(row);
      const position = row.position;
      return { data: {
        id, type: id.split(':')[0], kind: category(row),
        label: `${kind}\n${id}${position ? ` · Y${position.y}` : ''}`,
      } };
    }),
    ...view.edges.map((connection) => ({ data: {
      id: connection.id, source: connection.source, target: connection.target,
      type: connection.type, label: connectionLabel(connection),
    } })),
  ];
}

function check(value, label, checked, className) {
  const wrapper = document.createElement('label');
  const input = document.createElement('input');
  input.type = 'checkbox'; input.value = value; input.checked = checked; input.className = className;
  wrapper.append(input, document.createTextNode(label));
  return wrapper;
}

function option(value, label) {
  const element = document.createElement('option');
  element.value = value; element.textContent = label;
  return element;
}

function controls(data) {
  const inputs = data.graph.nodes.filter((row) => ['lever', 'button', 'pressure_plate'].includes(category(row)));
  $('inputs').replaceChildren(...inputs.slice(0, 20).map((row) => referenceButton(row.id, `${category(row)} ${row.id}\n(${row.position?.x}, ${row.position?.y}, ${row.position?.z})`)));
  const kinds = [...new Set(['site', 'route', ...data.graph.nodes.map(category)])].sort();
  $('node-filters').replaceChildren(...kinds.map((kind) => check(kind, kind.replaceAll('_', ' '), true, 'node-filter')));
  const edgeTypes = ['power_link', 'wire_relation', 'route_delivery', 'callback_link'];
  $('edge-filters').replaceChildren(...edgeTypes.map((kind) => check(kind, kind.replaceAll('_', ' '), kind === 'power_link', 'edge-filter')));
  document.querySelectorAll('.node-filter').forEach((input) => input.addEventListener('change', applyFilters));
  document.querySelectorAll('.edge-filter').forEach((input) => input.addEventListener('change', () => renderView()));
  $('snapshot').replaceChildren(option('', 'All potential connections'), ...data.snapshots.map((snapshot) => option(snapshot.id, snapshot.id)));
  $('snapshot').value = '';
  const layers = [...new Set(data.graph.nodes.concat(data.graph.sites).map((row) => row.position?.y).filter(Number.isFinite))].sort((a, b) => a - b);
  $('layer').replaceChildren(option('', 'All layers'), ...layers.map((y) => option(String(y), `Y = ${y}`)));
  $('layer').value = '';
  $('availability').textContent = `Routes: ${data.graph.routes === null ? 'unavailable' : data.graph.routes.length} · trace: ${data.trace === null ? 'unavailable' : (data.trace || []).length} · ${data.snapshots.length} snapshots`;
  for (const diagnostic of data.diagnostics || []) {
    const message = document.createElement('p');
    message.textContent = `${diagnostic.code}: ${diagnostic.message}`;
    $('availability').append(message);
  }
}

function initGraph(data) {
  recordIndex = indexRecords(data);
  graphData = data;
  const topology = indexConnections(data, recordIndex);
  connections = topology.edges;
  neighbors = topology.adjacent;
  reverseRefs = new Map();
  for (const [id, row] of recordIndex) for (const ref of new Set(typedIds(row))) {
    if (ref === id) continue;
    if (!reverseRefs.has(ref)) reverseRefs.set(ref, []);
    reverseRefs.get(ref).push(id);
  }
  controls(data);
  guardStates = new Map();
  selectedId = undefined;
  viewCenter = data.graph.nodes[0]?.id || data.graph.sites[0]?.id || data.graph.routes?.[0]?.id;
  const overview = selectView(viewCenter, true, enabledEdgeTypes(), 1);
  $('scope').value = overview.truncated ? 'neighborhood' : 'overview';
  renderView();
  $('summary').textContent = `v${data.version.major}.${data.version.minor} · ${data.stage} · ${data.graph.nodes.length} circuit nodes · ${data.graph.power_links.length} power links`;
  $('drop-hint').hidden = true;
  $('matches').replaceChildren();
  $('dependencies').replaceChildren();
  $('selection-title').textContent = 'Click a component or connection. Double-click a component to open its neighborhood.';
  $('details').textContent = 'No record selected.';
}

function renderView() {
  if (!graphData) return;
  currentView = selectView(viewCenter, $('scope').value === 'overview', enabledEdgeTypes(), Number($('depth').value));
  if (cy) cy.destroy();
  cy = cytoscape({
    container: $('graph'), elements: graphElements(currentView), layout: { name: 'preset' },
    minZoom: 0.03, maxZoom: 4,
    style: [
      { selector: 'node', style: { label: 'data(label)', color: '#e6edf3', 'font-size': 12, 'text-valign': 'center', 'text-wrap': 'wrap', shape: 'round-rectangle', 'background-color': '#285064', width: 115, height: 48, 'border-width': 2, 'border-color': '#8ec6d6' } },
      { selector: 'node[kind = "wire"]', style: { 'background-color': '#632f35', 'border-color': '#e88383', width: 100, height: 44 } },
      { selector: 'node[kind = "piston"]', style: { shape: 'rectangle', 'background-color': '#5e4b27', 'border-color': '#d3b46e' } },
      { selector: 'node[kind = "contact"]', style: { shape: 'diamond', 'background-color': '#453355', 'border-color': '#b69bd7', width: 130, height: 80 } },
      { selector: 'node[kind = "lever"], node[kind = "button"]', style: { 'background-color': '#26533e', 'border-color': '#7dd5a2' } },
      { selector: 'node[type = "site"]', style: { 'background-color': '#25313f', 'border-color': '#657a91', width: 115, height: 44, 'font-size': 10 } },
      { selector: 'edge', style: { width: 1.5, 'line-color': '#6589a4', 'target-arrow-color': '#8bbbdc', 'target-arrow-shape': 'triangle', 'curve-style': 'bezier', 'arrow-scale': 1, label: '', color: '#d6e4ee', 'font-size': 10, 'text-rotation': 'autorotate', 'text-background-color': '#111820', 'text-background-opacity': 0.95, 'text-background-padding': 3 } },
      { selector: 'edge[type = "wire_relation"]', style: { 'line-color': '#77ae72', 'target-arrow-color': '#77ae72', 'line-style': 'dashed' } },
      { selector: 'edge[type = "route_delivery"]', style: { 'line-color': '#c28b43', 'target-arrow-color': '#c28b43' } },
      { selector: 'edge[type = "callback_link"]', style: { 'line-color': '#ee8fb0', 'target-arrow-color': '#ee8fb0', 'line-style': 'dashed' } },
      { selector: 'edge.enabled', style: { 'line-color': '#65c799', 'target-arrow-color': '#65c799' } },
      { selector: 'edge.disabled', style: { opacity: 0.15 } },
      { selector: 'edge.unknown', style: { 'line-style': 'dashed' } },
      { selector: '.dimmed', style: { opacity: 0.12, 'text-opacity': 0.15 } },
      { selector: 'node.neighbor', style: { 'border-color': '#ffe16b', 'border-width': 3 } },
      { selector: 'edge.neighbor', style: { width: 3, label: 'data(label)', 'z-index': 5 } },
      { selector: 'node:selected', style: { 'border-width': 5, 'border-color': '#ffe16b', 'z-index': 10 } },
      { selector: 'edge:selected', style: { width: 4, 'line-color': '#ffe16b', 'target-arrow-color': '#ffe16b', label: 'data(label)', 'z-index': 10 } },
      { selector: 'edge.labels', style: { label: 'data(label)' } },
      { selector: '.filtered', style: { display: 'none' } },
    ],
  });
  cy.on('tap', 'node, edge', (event) => focusRecord(event.target.id()));
  cy.on('dbltap', 'node', (event) => {
    viewCenter = event.target.id(); $('scope').value = 'neighborhood'; renderView(); focusRecord(viewCenter);
  });
  cy.on('tap', (event) => { if (event.target === cy) clearHighlight(); });
  applyFilters();
  layoutGraph();
  if (selectedId) highlight(selectedId);
}

function layoutGraph() {
  if (!cy) return;
  const elements = cy.elements(':visible');
  if ($('layout').value === 'position') {
    const positioned = cy.nodes(':visible').filter((node) => recordIndex.get(node.id())?.position);
    const rows = positioned.map((node) => recordIndex.get(node.id()).position);
    if (!rows.length) { elements.layout({ name: 'grid', padding: 50 }).run(); return; }
    const minX = Math.min(...rows.map((p) => p.x)), minZ = Math.min(...rows.map((p) => p.z));
    const layers = [...new Set(rows.map((p) => p.y))].sort((a, b) => a - b);
    const layerHeight = (Math.max(...rows.map((p) => p.z)) - minZ + 3) * 140;
    const occupied = new Map();
    cy.nodes(':visible').forEach((node, index) => {
      const p = recordIndex.get(node.id())?.position;
      if (!p) { node.position({ x: -160, y: index * 90 }); return; }
      const key = `${p.x},${p.y},${p.z}`;
      const slot = occupied.get(key) || 0;
      occupied.set(key, slot + 1);
      // Separate records sharing a block (for example a piston and its base contact).
      node.position({ x: (p.x - minX) * 280 + (slot % 2) * 135, y: (p.z - minZ) * 140 + layers.indexOf(p.y) * layerHeight + Math.floor(slot / 2) * 90 });
    });
    cy.fit(elements, 50);
    return;
  }
  const flow = $('layout').value === 'breadthfirst';
  elements.layout(flow ? { name: 'breadthfirst', directed: true, spacingFactor: 1.3, padding: 50, avoidOverlap: true, nodeDimensionsIncludeLabels: true } : {
    name: 'cose', animate: false, randomize: true, numIter: 700,
    nodeDimensionsIncludeLabels: true, nodeRepulsion: 18000, idealEdgeLength: 130, componentSpacing: 100, padding: 50,
  }).run();
}

// Only state tests can be resolved without a site query/captured operand context.
function evaluateGuards(data, snapshot) {
  const pistons = new Map(snapshot.pistons.map((row) => [row.piston, row]));
  const resources = new Map(snapshot.resources.map((row) => [row.resource, row]));
  const states = new Map();
  for (const guard of data.graph.guards) {
    let result = null;
    if (guard.kind === 'always') result = true;
    else if (guard.kind === 'state') {
      const test = guard.test;
      const piston = pistons.get(test.piston), resource = resources.get(test.resource);
      if (test.kind === 'piston_extended' && typeof piston?.extended === 'boolean') result = piston.extended;
      else if (test.kind === 'piston_head_present' && typeof piston?.head_present === 'boolean') result = piston.head_present;
      else if (test.kind === 'piston_availability' && piston?.availability !== undefined) result = piston.availability === test.availability;
      else if (test.kind === 'resource_at' && resource?.selected_contact !== undefined) result = resource.selected_contact === test.contact;
      else if (test.kind === 'resource_enabled' && typeof resource?.enabled === 'boolean') result = resource.enabled;
    } else if (guard.kind === 'not') {
      const child = states.get(guard.guard) ?? null;
      result = child === null ? null : !child;
    } else if (guard.kind === 'all' || guard.kind === 'any') {
      const children = guard.guards.map((id) => states.get(id) ?? null);
      result = guard.kind === 'all' ? (children.includes(false) ? false : children.includes(null) ? null : true)
        : (children.includes(true) ? true : children.includes(null) ? null : false);
    }
    states.set(guard.id, result);
  }
  return states;
}

function applySnapshot() {
  if (!graphData) return;
  const snapshot = graphData.snapshots.find((row) => row.id === $('snapshot').value);
  guardStates = snapshot ? evaluateGuards(graphData, snapshot) : new Map();
  applyFilters();
  if (selectedId) inspect(selectedId);
}

function applyFilters() {
  if (!cy) return;
  const visibleNodes = new Set([...document.querySelectorAll('.node-filter:checked')].map((input) => input.value));
  const layer = $('layer').value;
  cy.nodes().forEach((node) => {
    const position = recordIndex.get(node.id())?.position;
    node.toggleClass('filtered', !visibleNodes.has(node.data('kind')) || (layer !== '' && position?.y !== Number(layer)));
  });
  cy.edges().forEach((edge) => {
    const state = guardStates.get(connections.get(edge.id()).record.guard);
    edge.toggleClass('enabled', state === true);
    edge.toggleClass('disabled', state === false);
    edge.toggleClass('unknown', guardStates.size > 0 && (state === null || state === undefined));
    edge.toggleClass('labels', $('edge-labels').checked);
    edge.toggleClass('filtered', edge.source().hasClass('filtered') || edge.target().hasClass('filtered') || ($('hide-inactive').checked && state === false));
  });
  const total = graphData.graph.nodes.length;
  const visible = cy.nodes(':visible').length;
  $('counts').textContent = `${total} circuit nodes in export · view ${visible}/${currentView.nodes.size} records, ${cy.edges(':visible').length}/${currentView.edges.length} connections${currentView.truncated ? ' · LIMIT REACHED: refine the neighborhood or filters' : ''}`;
}

function clearHighlight() {
  if (!cy) return;
  cy.elements().removeClass('dimmed neighbor').unselect();
}

function highlight(id) {
  clearHighlight();
  const element = selectionElements(id);
  if (element.empty()) return;
  const related = element.isEdge() ? element.connectedNodes().closedNeighborhood() : element.closedNeighborhood();
  cy.elements().difference(related).addClass('dimmed');
  related.addClass('neighbor');
  element.select();
}

function referenceButton(id, label = id) {
  const button = document.createElement('button');
  button.className = 'reference'; button.textContent = label;
  button.addEventListener('click', () => focusRecord(id, true));
  return button;
}

function referenceSection(title, ids, label) {
  if (!ids.length) return;
  const heading = document.createElement('h3'); heading.textContent = `${title} (${ids.length})`;
  const list = document.createElement('div'); list.className = 'reference-list';
  list.append(...ids.map((id) => referenceButton(id, label ? label(id) : `${id} · ${recordLabel(recordIndex.get(id))}`)));
  $('dependencies').append(heading, list);
}

function inspect(id) {
  const record = recordIndex.get(id) || connections.get(id)?.record;
  if (!record) return;
  const position = record.position;
  $('selection-title').textContent = `${id} · ${recordIndex.has(id) ? recordLabel(record) : connections.get(id).type}${position ? ` · (${position.x}, ${position.y}, ${position.z})` : ''}`;
  $('details').textContent = JSON.stringify(record, null, 2);
  $('dependencies').replaceChildren();
  const snapshot = graphData.snapshots.find((row) => row.id === $('snapshot').value);
  if (snapshot) {
    const targets = new Set([id, record.kind?.piston].filter(Boolean));
    const state = [...snapshot.components, ...snapshot.pistons, ...snapshot.resources].filter((row) => [row.node, row.piston, row.resource].some((ref) => targets.has(ref)));
    if (state.length) {
      const heading = document.createElement('h3'); heading.textContent = `State · ${snapshot.id}`;
      const preview = document.createElement('pre'); preview.className = 'state-preview'; preview.textContent = JSON.stringify(state, null, 2);
      $('dependencies').append(heading, preview);
    }
  }
  const connection = connections.get(id);
  if (connection) {
    const description = document.createElement('p');
    description.textContent = connectionLabel(connection);
    $('dependencies').append(description, referenceButton(connection.source, `From: ${connection.source}`), referenceButton(connection.target, `To: ${connection.target}`));
  }
  if (record.guard) {
    const status = document.createElement('p');
    const value = guardStates.get(record.guard);
    status.textContent = `Guard ${record.guard}: ${value === true ? 'enabled' : value === false ? 'disabled' : 'unknown (requires snapshot or site context)'}`;
    $('dependencies').append(status);
  }
  const adjacent = neighbors.get(id) || [];
  const describeEdge = (edgeId) => {
    const edge = connections.get(edgeId);
    return `${edgeId} · ${edge.source} → ${edge.target}\n${connectionLabel(edge)}`;
  };
  referenceSection('Incoming', adjacent.filter((edge) => edge.target === id).map((edge) => edge.id), describeEdge);
  referenceSection('Outgoing', adjacent.filter((edge) => edge.source === id).map((edge) => edge.id), describeEdge);
  referenceSection('References', [...new Set(typedIds(record))].filter((ref) => ref !== id && recordIndex.has(ref)));
  referenceSection('Referenced by', reverseRefs.get(id) || []);
}

function search() {
  if (!recordIndex) return;
  const query = $('search').value.trim().toLowerCase();
  $('matches').replaceChildren();
  if (!query) return;
  const exact = recordIndex.has(query) ? [[query, recordIndex.get(query)]] : [];
  const matches = [...exact, ...[...recordIndex.entries()].filter(([id, row]) => id !== query && `${id} ${JSON.stringify(row)}`.toLowerCase().includes(query))];
  const count = document.createElement('p');
  count.className = 'muted'; count.textContent = `${matches.length} results${matches.length > 50 ? ' · showing first 50; refine search' : ''}`;
  $('matches').append(count);
  for (const [id, row] of matches.slice(0, 50)) $('matches').append(referenceButton(id, `${id} · ${recordLabel(row)}`));
}

function visualRecord(id) {
  if (connections.has(id) || id.startsWith('node:')) return id;
  if (id.startsWith('route:')) return recordIndex.get(id)?.source || id;
  if (id.startsWith('site:')) return sourceNode(id, recordIndex);
  if (id.startsWith('resource:')) return recordIndex.get(recordIndex.get(id)?.contacts[0])?.node;
  return recordIndex.get(id)?.node;
}

function selectionElements(id) {
  const ids = id?.startsWith('resource:') ? recordIndex.get(id).contacts.map((contact) => recordIndex.get(contact).node) : [visualRecord(id)];
  return ids.reduce((elements, node) => node ? elements.union(cy.getElementById(node)) : elements, cy.collection());
}

function focusRecord(id, center = false) {
  selectedId = id;
  const connection = connections.get(id);
  if (connection && !enabledEdgeTypes().has(connection.type)) {
    const input = [...document.querySelectorAll('.edge-filter')].find((input) => input.value === connection.type);
    input.checked = true;
    renderView();
  }
  const visual = visualRecord(id);
  const resource = id.startsWith('resource:') ? recordIndex.get(id) : null;
  const missingContact = resource?.contacts.some((contact) => cy.getElementById(recordIndex.get(contact).node).empty());
  if (visual && (cy.getElementById(visual).empty() || missingContact)) {
    viewCenter = resource ? id : visual; $('scope').value = 'neighborhood'; renderView();
  }
  const elements = selectionElements(id);
  const nodes = elements.isEdge() ? elements.connectedNodes() : elements;
  nodes.forEach((node) => {
    const input = [...document.querySelectorAll('.node-filter')].find((input) => input.value === node.data('kind'));
    if (input) input.checked = true;
  });
  if (nodes.some((node) => node.hasClass('filtered'))) $('layer').value = '';
  applyFilters();
  if (visual) highlight(id);
  else clearHighlight();
  inspect(id);
  if (center) centerSelection();
}

function centerSelection() {
  if (!cy || !selectedId) return;
  const element = selectionElements(selectedId);
  if (!element.empty()) {
    cy.fit(element.closedNeighborhood().filter(':visible'), 100);
    cy.zoom(Math.min(cy.zoom(), 1.5));
    cy.center(element);
  }
}

async function readFile(file) {
  if (!file) return;
  $('error').hidden = true;
  try {
    if (file.size > 64 * 1024 * 1024) throw Error('Graph file exceeds the 64 MiB limit.');
    const data = validateGraph(JSON.parse(await file.text()));
    initGraph(data);
  } catch (error) {
    $('error').textContent = error.message || String(error);
    $('error').hidden = false;
  }
}

$('file').addEventListener('change', (event) => readFile(event.target.files[0]));
$('find').addEventListener('click', search);
$('search').addEventListener('keydown', (event) => { if (event.key === 'Enter') search(); });
$('scope').addEventListener('change', () => { viewCenter = selectedId?.startsWith('resource:') ? selectedId : visualRecord(selectedId || viewCenter) || viewCenter; renderView(); });
$('depth').addEventListener('change', () => renderView());
$('layout').addEventListener('change', layoutGraph);
$('layer').addEventListener('change', () => { applyFilters(); layoutGraph(); });
$('snapshot').addEventListener('change', applySnapshot);
$('hide-inactive').addEventListener('change', applyFilters);
$('edge-labels').addEventListener('change', applyFilters);
$('fit').addEventListener('click', () => cy?.fit(cy.elements(':visible'), 50));
$('center').addEventListener('click', centerSelection);
$('relax').addEventListener('click', layoutGraph);
document.addEventListener('keydown', (event) => { if (event.key === 'Escape') clearHighlight(); });
$('graph').addEventListener('dragover', (event) => event.preventDefault());
$('graph').addEventListener('drop', (event) => { event.preventDefault(); readFile(event.dataTransfer.files[0]); });
