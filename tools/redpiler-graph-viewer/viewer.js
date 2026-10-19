"use strict";

const tables = {
  nodes: "node", sites: "site", contacts: "contact", resources: "resource", guards: "guard",
  pistons: "piston", power_links: "power_link", wire_plans: "wire", wire_relations: "wire_relation",
  instant_regions: "instant_region", instant_proxies: "instant_proxy",
  instant_cells: "instant_cell", instant_corridors: "instant_corridor",
  instant_boundary_ports: "instant_port", instant_declines: "instant_decline",
  instant_layer_regions: "instant_layer_region", instant_layer_declines: "instant_layer_decline",
  classic_nets: "classic_net",
  fixed_signal_queries: "fixed_signal",
  dust_paths: "dust_path", comparator_plans: "comparator_plan", callback_links: "callback_link", routes: "route", profiles: "profile",
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
    if (["dust_paths", "instant_regions", "instant_proxies", "classic_nets", "fixed_signal_queries",
      "instant_cells", "instant_corridors", "instant_boundary_ports", "instant_declines",
      "instant_layer_regions", "instant_layer_declines"].includes(table) && rows == null) continue;
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
  const nodes = new Map(data.graph.nodes.map((node) => [node.id, node]));
  for (const node of nodes.values()) {
    if (node.kind?.builtin_reset !== undefined && (typeof node.kind.builtin_reset !== "boolean" || node.kind.kind !== "observer")) throw Error(`${node.id}.builtin_reset must be an observer boolean.`);
    if (node.kind?.builtin_reset && (node.kind.kind !== "observer" || node.output_route != null)) throw Error(`${node.id} has an invalid built-in reset.`);
  }
  const sites = new Map(data.graph.sites.map((site) => [site.id, site]));
  const regionRows = new Map((data.graph.instant_regions || []).map((region) => [region.id, region]));
  const routeRows = new Map((data.graph.routes || []).map((route) => [route.id, route]));
  for (const region of regionRows.values()) {
    if (region.stable_inputs !== undefined && typeof region.stable_inputs !== "boolean") throw Error(`${region.id} has invalid stable_inputs.`);
    if (!Array.isArray(region.wires ?? []) || (region.wires ?? []).some((wire) => nodes.get(wire)?.kind?.kind !== "wire")) throw Error(`${region.id} has invalid internal wires.`);
    const fields = {actors: "piston", resources: "resource", resets: "node", contacts: "contact", readable_sites: "site",
      power_inputs: "power_link", power_readers: "power_link", routes: "route", wire_readers: "node", observers: "node"};
    for (const [field, prefix] of Object.entries(fields)) {
      if (!Array.isArray(region[field]) || region[field].some((id) => typeof id !== "string" || !id.startsWith(`${prefix}:`) || !ids.has(id))) throw Error(`${region.id} has invalid ${field}.`);
    }
    for (const field of ["incoming", "outgoing"]) {
      if (!Array.isArray(region[field])) throw Error(`${region.id} has invalid ${field}.`);
      for (const event of region[field]) {
        if (!Number.isInteger(event.ordinal) || event.ordinal < 0 || event.ordinal >= (routeRows.get(event.route)?.deliveries.length ?? -1)) throw Error(`${region.id} has an invalid route occurrence.`);
      }
    }
  }
  const proxyRoutes = new Set();
  for (const proxy of data.graph.instant_proxies || []) {
    const region = regionRows.get(proxy.region);
    if (!region || !Array.isArray(proxy.private_cells) || !Array.isArray(proxy.routes)) throw Error(`${proxy.id} has an invalid region.`);
    const privateResources = new Set();
    for (const cell of proxy.private_cells) {
      if (!cell || !region.actors.includes(cell.actor) || !region.resources.includes(cell.resource)
        || !region.resets.includes(cell.reset) || privateResources.has(cell.resource)) throw Error(`${proxy.id} has an invalid private cell.`);
      privateResources.add(cell.resource);
    }
    for (const plan of proxy.routes) {
      if (!region.routes.includes(plan.route) || proxyRoutes.has(plan.route) || !Array.isArray(plan.operations)
        || plan.operations.length !== routeRows.get(plan.route)?.deliveries.length
        || plan.operations.some((operation) => !operation || typeof operation !== "object"
          || !(operation.kind === "dispatch" || (operation.kind === "sample" && region.actors.includes(operation.piston))
            || (operation.kind === "arm_reset" && region.resets.includes(operation.node))))) throw Error(`${proxy.id} has an invalid proxy route.`);
      proxyRoutes.add(plan.route);
    }
  }
  validateInstantMachine(data.graph.instant_machine, data.graph);
  validateInstantKernel(data.graph.instant_kernel, data.graph);
  const instantWires = new Set();
  for (const wire of data.graph.wire_plans) {
    if (wire.hop_root != null) {
      const root = sites.get(wire.hop_root);
      if (root?.source?.kind !== "component" || root.source.node !== wire.node) throw Error(`${wire.id} hop root must bind its component.`);
    }
    const instant = wire.instant_reset;
    if (instant != null) {
      if (typeof instant !== "object" || Array.isArray(instant)
        || typeof instant.piston !== "string" || !instant.piston.startsWith("piston:") || !ids.has(instant.piston)
        || typeof instant.resource !== "string" || !instant.resource.startsWith("resource:") || !ids.has(instant.resource)
        || nodes.get(wire.node)?.kind?.kind !== "wire" || nodes.get(instant.partner)?.kind?.kind !== "wire"
        || nodes.get(instant.observer)?.kind?.kind !== "observer" || wire.node === instant.partner
        || instantWires.has(wire.node) || instantWires.has(instant.partner)) {
        throw Error(`${wire.id} has invalid or overlapping instant reset bindings.`);
      }
      instantWires.add(wire.node);
      instantWires.add(instant.partner);
    }
    const callbacks = wire.callback_selection;
    if (callbacks != null && (!Number.isInteger(callbacks.known) || callbacks.known < 0 || callbacks.known > 0xffffff || !Number.isInteger(callbacks.admitted) || callbacks.admitted < 0 || callbacks.admitted > 0xffffff || (callbacks.admitted & ~callbacks.known) !== 0)) throw Error(`Invalid compiled wire callback masks: ${wire.id}`);
    const facts = wire.query_facts;
    if (facts == null) continue;
    if (typeof facts !== "object" || Array.isArray(facts)) throw Error(`${wire.id} has invalid wire query fact masks.`);
    if ("isolated" in facts && typeof facts.isolated !== "boolean") throw Error(`${wire.id}.query_facts.isolated must be a boolean.`);
    const masks = [facts.known, facts.solid, facts.transparent, facts.wire, facts.zero_no_dust];
    if (masks.some((mask) => !Number.isInteger(mask) || mask < 0 || mask > 0xffffff)
      || ((facts.solid | facts.transparent | facts.wire | facts.zero_no_dust) & ~facts.known)
      || (facts.zero_no_dust & facts.solid)) throw Error(`${wire.id} has invalid wire query fact masks.`);
    if (facts.raw_sides != null && (!Number.isInteger(facts.raw_sides) || facts.raw_sides < 0 || facts.raw_sides > 255
      || facts.known !== 0xffffff || [0, 2, 4, 6].some((shift) => ((facts.raw_sides >> shift) & 3) === 3))) {
      throw Error(`${wire.id} has invalid compiled raw wire sides.`);
    }
    if (facts.transfer != null) {
      const diagonals = [7, 8, 12, 13, 17, 18, 20, 21].reduce((mask, slot) => mask | (1 << slot), 0);
      for (const phase of ["seed", "turbo"]) {
        const inputs = facts.transfer?.[phase];
        if (!inputs || typeof inputs !== "object" || Array.isArray(inputs)
          || [inputs.direct, inputs.conditional].some((mask) => !Number.isInteger(mask) || mask < 0 || mask > 0xffffff)
          || (inputs.direct & ~(diagonals | 0x3f)) || (inputs.conditional & ~diagonals)
          || (inputs.direct & inputs.conditional) || (phase === "turbo" && (inputs.direct & 0xc))) {
          throw Error(`${wire.id} has invalid wire transfer inputs.`);
        }
      }
    }
  }
  const assignedWires = new Set();
  for (const query of data.graph.fixed_signal_queries || []) {
    if (!["live_state", "settled_provenance"].includes(query.semantics) || !Number.isInteger(query.constant) || query.constant < 0 || query.constant > 15 || !Array.isArray(query.inputs)) throw Error(`Invalid fixed signal query: ${query.id}`);
    if (query.to?.kind === "input") {
      if (!nodes.has(query.to.node) || !["main", "side", "quasi"].includes(query.to.port)) throw Error(`Invalid fixed signal target: ${query.id}`);
    } else if (query.to?.kind !== "conductor" || !sites.has(query.to.site) || typeof query.to.include_dust !== "boolean") throw Error(`Invalid fixed signal conductor: ${query.id}`);
    for (const input of query.inputs) if (!nodes.has(input.node) || !Number.isInteger(input.attenuation) || input.attenuation < 0 || input.attenuation >= 15 || typeof input.require_cross !== "boolean") throw Error(`Invalid fixed signal input: ${query.id}`);
  }
  for (const net of data.graph.classic_nets || []) {
    if (!nodes.has(net.source) || !Number.isInteger(net.initial_signal) || net.initial_signal < 0 || net.initial_signal > 15 || !net.wires?.length) throw Error(`Invalid classic net: ${net.id}`);
    for (const guard of net.guarded_wires || []) {
      if (nodes.get(guard)?.kind?.kind !== 'wire' || net.wires.some((wire) => wire.node === guard)) throw Error(`Invalid classic guarded wire: ${guard}`);
    }
    for (const wire of net.wires) {
      if (nodes.get(wire.node)?.kind?.kind !== 'wire' || assignedWires.has(wire.node) || !Number.isInteger(wire.attenuation) || wire.attenuation < 0 || wire.attenuation > 15) throw Error(`Invalid classic wire: ${wire.node}`);
      assignedWires.add(wire.node);
    }
    for (const term of net.outside_main || []) {
      if (!nodes.has(term.node) || !net.inputs.some((input) => input.receiver === term.receiver) || net.wires.some((wire) => wire.node === term.node) || !Number.isInteger(term.attenuation) || term.attenuation < 0 || term.attenuation >= 15) throw Error(`Invalid classic outside input: ${net.id}`);
    }
    const guardedMain = new Set((net.outside_main || []).map((term) => term.receiver));
    for (const input of net.inputs) {
      if ((input.guarded != null && typeof input.guarded !== 'boolean') || Boolean(input.guarded) !== guardedMain.has(input.receiver)) throw Error(`Invalid classic guarded input: ${net.id}`);
      if (!nodes.has(input.receiver) || [input.attenuation, input.source_attenuation].some((value) => !Number.isInteger(value) || value < 0 || value > 15)) throw Error(`Invalid classic input: ${net.id}`);
    }
    for (const entry of net.entries) {
      const route = data.graph.routes?.find((route) => route.id === entry.route);
      if (!route?.deliveries[entry.ordinal] || typeof entry.commit !== 'boolean') throw Error(`Invalid classic entry: ${net.id}`);
      for (const op of entry.ops) if (!nodes.has(op.receiver) || !(op.sample?.kind === 'comparator' ? Number.isInteger(op.sample.strength) && op.sample.strength >= 0 && op.sample.strength <= 15 && typeof op.sample.powered === 'boolean' : typeof op.nonzero === 'boolean' && (!op.sample || op.sample.kind === 'boolean' && op.sample.nonzero === op.nonzero)) || op.when?.length !== 4 || op.when.some((mask) => !/^[0-9a-f]{16}$/.test(mask))) throw Error(`Invalid classic operation: ${net.id}`);
    }
  }
  for (const path of data.graph.dust_paths || []) {
    const input = nodes.get(path.input_node);
    if (input?.kind?.kind !== "electrical") throw Error(`${path.id} input must be an electrical node.`);
    if (sites.get(path.input_site)?.source?.kind !== "component" || sites.get(path.input_site).source.node !== path.input_node) {
      throw Error(`${path.id} input site must be owned by its input node.`);
    }
    if (!["north", "south", "east", "west"].includes(path.input_face)) throw Error(`${path.id} input face must be horizontal.`);
    if (!Array.isArray(path.wires) || path.wires.length < 2) throw Error(`${path.id} must contain at least two wire nodes.`);
    const localWires = new Set();
    for (const wireId of path.wires) {
      if (nodes.get(wireId)?.kind?.kind !== "wire") throw Error(`${path.id} references non-wire node ${wireId}.`);
      if (localWires.has(wireId) || assignedWires.has(wireId)) throw Error(`${path.id} repeats or shares wire node ${wireId}.`);
      localWires.add(wireId);
      assignedWires.add(wireId);
    }
  }
  validateInstantBoundaries(data, ids, nodes);
  validateInstantLayers(data, ids, nodes);
  return data;
}

function validateInstantBoundaries(data, ids, nodes) {
  const graph = data.graph;
  const cells = graph.instant_cells || [];
  const corridors = graph.instant_corridors || [];
  const ports = graph.instant_boundary_ports || [];
  const declines = graph.instant_declines || [];
  if ((cells.length || corridors.length || ports.length || declines.length)
    && ((graph.instant_regions || []).length || (graph.instant_proxies || []).length)) {
    throw Error("Instant boundary tables cannot coexist with legacy instant regions or proxies.");
  }
  const ref = (id, prefix, owner) => {
    if (typeof id !== "string" || !id.startsWith(`${prefix}:`) || !ids.has(id)) throw Error(`${owner} has an invalid ${prefix} reference.`);
  };
  const actors = new Set(), resources = new Set(), resets = new Set();
  const declinedActors = new Set();
  for (const cell of cells) {
    for (const [field, prefix, owned] of [["actors", "piston", actors], ["resources", "resource", resources], ["resets", "node", resets]]) {
      if (!Array.isArray(cell[field])) throw Error(`${cell.id} has invalid ${field}.`);
      const local = new Set();
      for (const id of cell[field]) {
        ref(id, prefix, cell.id);
        if (field === "resets" && nodes.get(id)?.kind?.kind !== "observer") throw Error(`${cell.id} reset must be an observer node.`);
        if (local.has(id) || owned.has(id)) throw Error(`${cell.id} repeats or shares ${field} ownership.`);
        local.add(id);
        owned.add(id);
      }
    }
  }
  for (const decline of declines) {
    if (!Array.isArray(decline.actors)) throw Error(`${decline.id} has invalid actors.`);
    for (const actor of decline.actors) {
      ref(actor, "piston", decline.id);
      if (declinedActors.has(actor)) throw Error(`${decline.id} repeats declined actor ownership.`);
      declinedActors.add(actor);
    }
    if (!decline.reason || typeof decline.reason !== "object" || Array.isArray(decline.reason)
      || typeof decline.reason.code !== "string" || !/^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/.test(decline.reason.code)) {
      throw Error(`${decline.id} has an invalid decline reason.`);
    }
    for (const prefix of ["piston", "resource", "contact"]) {
      if (decline.reason[prefix] != null) ref(decline.reason[prefix], prefix, decline.id);
    }
  }
  for (const actor of actors) if (declinedActors.has(actor)) throw Error("An instant actor cannot be both admitted and declined.");

  const corridorWires = new Set(), corridorSites = new Set();
  for (const corridor of corridors) {
    for (const [field, prefix, owned] of [["wires", "node", corridorWires], ["conductive_sites", "site", corridorSites]]) {
      if (!Array.isArray(corridor[field])) throw Error(`${corridor.id} has invalid ${field}.`);
      const local = new Set();
      for (const id of corridor[field]) {
        ref(id, prefix, corridor.id);
        if (field === "wires" && nodes.get(id)?.kind?.kind !== "wire") throw Error(`${corridor.id} references a non-wire corridor node.`);
        if (local.has(id) || owned.has(id)) throw Error(`${corridor.id} repeats or shares ${field} ownership.`);
        local.add(id);
        owned.add(id);
      }
    }
  }
  const classicOwnedWires = new Set((graph.classic_nets || []).flatMap((net) => net.wires.map((wire) => wire.node)));
  for (const wire of corridorWires) if (classicOwnedWires.has(wire)) throw Error("Instant corridors cannot own classic-owned wires.");

  const cellIds = new Set(cells.map((cell) => cell.id));
  const corridorIds = new Set(corridors.map((corridor) => corridor.id));
  const context = {
    nodes,
    routes: new Map((graph.routes || []).map((route) => [route.id, route])),
    guards: new Map(graph.guards.map((guard) => [guard.id, guard])),
    contacts: new Map(graph.contacts.map((contact) => [contact.id, contact])),
    plans: new Map(graph.comparator_plans.map((plan) => [plan.id, plan])),
    wirePlans: new Map(graph.wire_plans.map((plan) => [plan.id, plan])),
  };
  for (const port of ports) {
    if (cellIds.has(port.owner)) ref(port.owner, "instant_cell", port.id);
    else if (corridorIds.has(port.owner)) ref(port.owner, "instant_corridor", port.id);
    else throw Error(`${port.id} has an invalid instant boundary owner.`);
    if (!['input', 'output'].includes(port.direction)) throw Error(`${port.id} has an invalid boundary direction.`);
    if (!port.binding || typeof port.binding !== "object" || Array.isArray(port.binding)) throw Error(`${port.id} has an invalid binding.`);
    validateBoundaryBinding(port, ids, context);
  }
}

function validateInstantLayers(data, ids, nodes) {
  const graph = data.graph;
  const regions = graph.instant_layer_regions || [];
  const declines = graph.instant_layer_declines || [];
  if (!regions.length && !declines.length) return;
  if ((graph.instant_regions || []).length || (graph.instant_proxies || []).length) throw Error("Instant layer tables cannot coexist with legacy instant regions or proxies.");
  const cells = new Map((graph.instant_cells || []).map((row) => [row.id, row]));
  const corridors = new Map((graph.instant_corridors || []).map((row) => [row.id, row]));
  const ports = new Map((graph.instant_boundary_ports || []).map((row) => [row.id, row]));
  const pistons = new Map(graph.pistons.map((row) => [row.id, row]));
  const resources = new Map(graph.resources.map((row) => [row.id, row]));
  const contacts = new Map(graph.contacts.map((row) => [row.id, row]));
  const powerLinks = new Map(graph.power_links.map((row) => [row.id, row]));
  const sites = new Map(graph.sites.map((row) => [row.id, row]));
  const guards = new Map(graph.guards.map((row) => [row.id, row]));
  const routes = new Map((graph.routes || []).map((row) => [row.id, row]));
  if (!cells.size && !corridors.size) throw Error("Instant layer tables require the boundary catalog.");
  const ref = (id, prefix, owner) => {
    if (typeof id !== "string" || !id.startsWith(`${prefix}:`) || !ids.has(id)) throw Error(`${owner} has an invalid ${prefix} reference.`);
    return id;
  };
  const index = (value, length, owner) => {
    if (!Number.isInteger(value) || value < 0 || value >= length || value > 0xffffffff) throw Error(`${owner} has an invalid local index.`);
    return value;
  };
  const root = (value, length, owner) => index(value, length, owner);
  const occurrence = (value, owner) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${owner} has an invalid route occurrence.`);
    ref(value.route, "route", owner);
    const deliveries = routes.get(value.route)?.deliveries;
    if (!Number.isInteger(value.ordinal) || value.ordinal < 0 || value.ordinal >= (deliveries?.length ?? 0)) throw Error(`${owner} has an invalid route occurrence.`);
    return [value.route, value.ordinal];
  };
  const stateTest = (test, owner) => {
    if (!test || typeof test !== "object" || Array.isArray(test)) throw Error(`${owner} has an invalid state test.`);
    if (["piston_extended", "piston_head_present"].includes(test.kind)) ref(test.piston, "piston", owner);
    else if (test.kind === "piston_availability") {
      ref(test.piston, "piston", owner);
      if (!["ready", "head_pending", "publishing_head_removal", "base_pending_head_present", "base_pending"].includes(test.availability)) throw Error(`${owner} has an invalid piston availability.`);
    } else if (test.kind === "resource_at") {
      ref(test.resource, "resource", owner); ref(test.contact, "contact", owner);
    } else if (test.kind === "resource_enabled") ref(test.resource, "resource", owner);
    else throw Error(`${owner} has an unknown state test.`);
    return test;
  };

  const ownedCells = new Set(), ownedCorridors = new Set();
  for (const region of regions) {
    for (const field of ["inputs", "outputs", "resource_outputs", "cold_lifecycle_occurrences", "expressions", "pin_values", "layers"]) {
      if (!Array.isArray(region[field])) throw Error(`${region.id} has invalid ${field}.`);
    }
    const memberCells = new Set(), memberCorridors = new Set();
    for (const [field, prefix, available, all, local] of [
      ["cells", "instant_cell", cells, ownedCells, memberCells],
      ["corridors", "instant_corridor", corridors, ownedCorridors, memberCorridors],
    ]) {
      if (!Array.isArray(region[field])) throw Error(`${region.id} has invalid ${field}.`);
      for (const id of region[field]) {
        ref(id, prefix, region.id);
        if (!available.has(id) || local.has(id) || all.has(id)) throw Error(`${region.id} repeats or shares ${field} ownership.`);
        local.add(id); all.add(id);
      }
    }
    const actors = new Set([...memberCells].flatMap((cell) => cells.get(cell).actors));
    const resourcesOwned = new Set([...memberCells].flatMap((cell) => cells.get(cell).resources));
    const resetObservers = new Set([...memberCells].flatMap((cell) => cells.get(cell).resets));
    const actorNodes = new Set([...actors].map((actor) => pistons.get(actor)?.node));
    for (const input of region.inputs) {
      if (!input || typeof input !== "object" || Array.isArray(input)
        || !["request_sample", "activation_event", "acceptance_revalidation"].includes(input.role)) throw Error(`${region.id} has an invalid input.`);
      const source = input.source;
      if (!source || typeof source !== "object" || Array.isArray(source)) throw Error(`${region.id} has an invalid input source.`);
      if (["boundary_event", "power_contribution"].includes(source.kind)) {
        ref(source.port, "instant_port", region.id);
        const port = ports.get(source.port);
        if (!port || port.direction !== "input" || !(memberCells.has(port.owner) || memberCorridors.has(port.owner))) throw Error(`${region.id} input port is outside its boundary.`);
        if (source.kind === "boundary_event") {
          occurrence(source.occurrence, region.id);
          if (port.binding?.kind !== "callback" || port.binding.occurrence.route !== source.occurrence.route
            || port.binding.occurrence.ordinal !== source.occurrence.ordinal) throw Error(`${region.id} event source does not match its callback port.`);
        } else {
          ref(source.link, "power_link", region.id);
          if (port.binding?.kind !== "power" || port.binding.link !== source.link) throw Error(`${region.id} power source does not match its power port.`);
        }
      } else if (source.kind === "source_above") {
        ref(source.site, "site", region.id);
        const port = source.port;
        const validPort = ["output", "stored"].includes(port?.kind) ||
          (["weak", "strong"].includes(port?.kind) && ["top", "bottom", "north", "south", "east", "west"].includes(port?.face));
        if (!Number.isInteger(source.threshold) || source.threshold < 0 || source.threshold >= 15 || !validPort) throw Error(`${region.id} has an invalid threshold source.`);
        const ownedWires = new Set([...memberCorridors].flatMap(id => corridors.get(id).wires));
        if (ownedWires.has(sites.get(source.site)?.source?.node)) throw Error(`${region.id} exposes an owned internal wire as input.`);
      } else if (source.kind === "boundary_state") {
        const test = stateTest(source.test, region.id);
        ref(source.port, "instant_port", region.id);
        const port = ports.get(source.port);
        if (port.direction !== "input" || !(memberCells.has(port.owner) || memberCorridors.has(port.owner))
          || port.binding?.kind !== "guard_state"
          || !["kind", "piston", "resource", "contact", "availability"].every(field => port.binding.test?.[field] === test[field])) {
          throw Error(`${region.id} boundary state does not match its input guard.`);
        }
      } else if (source.kind === "state") {
        const test = stateTest(source.test, region.id);
        if (test.kind.startsWith("piston_")) {
          if (!actors.has(test.piston)) throw Error(`${region.id} reads state for an actor outside its cells.`);
        } else {
          if (!resourcesOwned.has(test.resource)) throw Error(`${region.id} reads state for a resource outside its cells.`);
          if (test.kind === "resource_at" && !resources.get(test.resource).contacts.includes(test.contact)) throw Error(`${region.id} reads a resource at a contact it does not own.`);
        }
      } else if (source.kind === "local_reset_output") {
        ref(source.observer, "node", region.id);
        const observer = nodes.get(source.observer);
        if (!resetObservers.has(source.observer) || observer?.kind?.kind !== "observer") throw Error(`${region.id} reset source is not an owned observer.`);
        ref(source.link, "power_link", region.id);
        const link = powerLinks.get(source.link), site = sites.get(link.from.site), guard = guards.get(link.guard);
        const port = link.from.port, target = link.to, siteSource = site?.source;
        const validPort = port?.kind === "output" || (port?.kind === "strong" && port.face === "bottom");
        const targetIsQuasi = target?.kind === "input" && target.port === "quasi" && actorNodes.has(target.node);
        let targetIsStaticConductor = false;
        if (target?.kind === "conductor") {
          ref(target.site, "site", region.id);
          targetIsStaticConductor = port?.kind === "strong" && port.face === "bottom"
            && link.face === "bottom" && sites.get(target.site)?.source?.kind === "constant";
        }
        if (siteSource?.kind !== "component" || siteSource.node !== source.observer || !validPort
          || link.attenuation !== 0 || guard?.kind !== "always" || !(targetIsQuasi || targetIsStaticConductor)) throw Error(`${region.id} reset source does not match its original quasi link.`);
      } else throw Error(`${region.id} has an unknown input source.`);
    }
    const expressions = region.expressions;
    if (!Array.isArray(expressions)) throw Error(`${region.id} has invalid expressions.`);
    for (let i = 0; i < expressions.length; i++) {
      const expression = expressions[i];
      if (!expression || typeof expression !== "object" || Array.isArray(expression)) throw Error(`${region.id} has a malformed expression.`);
      if (expression.kind === "constant") {
        if (typeof expression.value !== "boolean") throw Error(`${region.id} has an invalid Boolean constant.`);
      } else if (expression.kind === "input") index(expression.input, region.inputs?.length ?? 0, region.id);
      else if (expression.kind === "not") index(expression.value, i, region.id);
      else if (["all", "any"].includes(expression.kind)) {
        if (!Array.isArray(expression.values)) throw Error(`${region.id} has invalid ${expression.kind} operands.`);
        for (const operand of expression.values) index(operand, i, region.id);
      } else throw Error(`${region.id} has an unknown expression kind.`);
    }
    root(region.stationary_entry, expressions.length, region.id);
    if (region.resource_outputs.length === 0) throw Error(`${region.id} must expose resource transfer obligations.`);
    for (const output of region.resource_outputs) {
      ref(output.cell, "instant_cell", region.id); ref(output.resource, "resource", region.id);
      if (!memberCells.has(output.cell) || !cells.get(output.cell).resources.includes(output.resource)) throw Error(`${region.id} resource output is outside its cell.`);
      const contacts = resources.get(output.resource).contacts;
      for (const field of ["source", "destination"]) {
        ref(output[field], "contact", region.id);
        if (!contacts.includes(output[field])) throw Error(`${region.id} resource output contact is outside its resource.`);
      }
      if (output.source === output.destination) throw Error(`${region.id} resource output must move between distinct contacts.`);
      root(output.transfer_when, expressions.length, region.id);
      root(output.source_absent_after, expressions.length, region.id);
    }
    for (const output of region.outputs) {
      ref(output.port, "instant_port", region.id);
      const port = ports.get(output.port);
      if (!port || port.direction !== "output" || !(memberCells.has(port.owner) || memberCorridors.has(port.owner))) throw Error(`${region.id} output port is outside its boundary.`);
      root(output.emitted_when, expressions.length, region.id);
    }
    const graphOutputs = region.graph_outputs ?? [];
    if (!Array.isArray(graphOutputs)) throw Error(`${region.id} has invalid Graph outputs.`);
    const seenGraphOutputs = new Set();
    for (const id of graphOutputs) {
      ref(id, "instant_port", region.id);
      const port = ports.get(id);
      if (seenGraphOutputs.has(id) || port.direction !== "output"
        || !(memberCells.has(port.owner) || memberCorridors.has(port.owner))
        || !["guard_site", "wire_relation", "wire_neighborhood", "captured_wire"].includes(port.binding?.kind)) {
        throw Error(`${region.id} Graph output is not a unique geometry observation on its boundary.`);
      }
      seenGraphOutputs.add(id);
    }
    for (const pin of region.pin_values) {
      ref(pin.cell, "instant_cell", region.id);
      if (!memberCells.has(pin.cell) || !Number.isInteger(pin.pin) || pin.pin < 0 || pin.pin > 0xffffffff) throw Error(`${region.id} has a pin value outside its cell.`);
      root(pin.value, expressions.length, region.id);
    }
    const useActors = new Map(), useOccurrences = new Set();
    for (const layer of region.layers) {
      if (!layer || layer.order !== "fifo" || !Array.isArray(layer.uses)) throw Error(`${region.id} has an invalid causal layer.`);
      for (const use of layer.uses) {
        index(use.use_id, 0x100000000, region.id);
        ref(use.actor, "piston", region.id);
        if (!actors.has(use.actor)) throw Error(`${region.id} use actor is outside its region.`);
        if (useActors.has(use.use_id) && useActors.get(use.use_id) !== use.actor) throw Error(`${region.id} changes the actor for a use ID.`);
        useActors.set(use.use_id, use.actor);
        for (const field of ["requested", "accepted", "consumed_before", "consumed_after"]) root(use[field], expressions.length, region.id);
        let causeKey;
        if (use.cause?.kind === "boundary") {
          if (!Array.isArray(use.cause.occurrences)) throw Error(`${region.id} has invalid boundary cause occurrences.`);
          if (!use.cause.occurrences.length) throw Error(`${region.id} has a boundary cause without occurrences.`);
          causeKey = ["boundary", use.cause.occurrences.map((item) => occurrence(item, region.id))];
        } else throw Error(`${region.id} has an invalid use cause.`);
        const prefix = [use.requested, use.accepted, use.consumed_before, use.consumed_after];
        const signature = JSON.stringify([use.use_id, use.actor, causeKey, prefix]);
        if (useOccurrences.has(signature)) throw Error(`${region.id} repeats an exact use occurrence.`);
        useOccurrences.add(signature);
      }
    }
    for (const item of region.cold_lifecycle_occurrences) {
      ref(item.actor, "piston", region.id);
      if (!actors.has(item.actor)) throw Error(`${region.id} lifecycle actor is outside its cells.`);
      const [routeId] = occurrence(item.occurrence, region.id);
      const phase = item.phase, reason = item.reason?.kind;
      const retraction = ["retraction_base_changed", "retraction_far_changed", "retraction_near_changed"].includes(phase);
      const deferred = ["deferred_base_restoration", "deferred_head_restoration", "deferred_resource_restoration"].includes(phase);
      const validReason = (retraction && reason === "inert_cell_callback")
        || (phase === "reset_observer_lifecycle" && reason === "reset_observer_update")
        || (["reset_output", "reset_far_shape_change"].includes(phase) && reason === "ordinary_graph_lifecycle")
        || (deferred && reason === "ordinary_graph_lifecycle");
      if (!validReason) throw Error(`${region.id} has an invalid lifecycle phase and reason.`);
      if (phase === "reset_observer_lifecycle" && reason === "reset_observer_update") {
        ref(item.reason.observer, "node", region.id);
        if (!resetObservers.has(item.reason.observer) || nodes.get(item.reason.observer)?.kind?.kind !== "observer") throw Error(`${region.id} lifecycle observer is outside its resets.`);
      }
      const routeBindings = pistons.get(item.actor)?.routes || {};
      const resetRoutes = [];
      if (reason === "reset_observer_update") {
        const watched = nodes.get(item.reason.observer)?.kind?.watched;
        const watchedContact = contacts.get(sites.get(watched)?.source?.contact);
        for (const variant of watchedContact?.variants || []) {
          const receiver = variant.receiver;
          if (receiver?.kind !== "piston_base") continue;
          const watchedRoutes = pistons.get(receiver.piston)?.routes || {};
          resetRoutes.push(watchedRoutes.base_changed, watchedRoutes.base_restored);
        }
      }
      const expectedRoutes = {
        retraction_base_changed: [routeBindings.base_changed],
        retraction_far_changed: [routeBindings.far_changed],
        retraction_near_changed: [routeBindings.near_changed],
        reset_far_shape_change: [routeBindings.far_shape_changed],
        deferred_base_restoration: [routeBindings.base_restored],
        deferred_head_restoration: [routeBindings.head_restored],
        deferred_resource_restoration: [routeBindings.resource_restored_near, routeBindings.resource_restored_far],
        reset_observer_lifecycle: resetRoutes,
      };
      if (phase === "reset_output") {
        const recognizedObserver = [...resetObservers].some((observerId) => {
          const observer = nodes.get(observerId);
          return observer?.kind?.kind === "observer" && observer.output_route === routeId && routes.get(routeId)?.source === observerId;
        });
        if (!recognizedObserver) throw Error(`${region.id} reset output route is not owned by a recognized observer.`);
      } else if (phase === "reset_far_shape_change") {
        if (!expectedRoutes[phase]?.includes(routeId) || routes.get(routeId)?.source !== pistons.get(item.actor)?.node) {
          throw Error(`${region.id} far-shape route is not owned by its actor.`);
        }
      } else if (!expectedRoutes[phase]?.includes(routeId)) throw Error(`${region.id} lifecycle occurrence does not match its declared route phase.`);
    }
  }
  const reasons = new Set(["signal_arrival_unproven", "callback_semantics_unsupported",
    "shared_owner_order_unknown", "boundary_effect_unsupported"]);
  for (const decline of declines) {
    const memberCells = new Set(), memberCorridors = new Set();
    for (const [field, prefix, available, all, local] of [
      ["cells", "instant_cell", cells, ownedCells, memberCells],
      ["corridors", "instant_corridor", corridors, ownedCorridors, memberCorridors],
    ]) {
      if (!Array.isArray(decline[field])) throw Error(`${decline.id} has invalid ${field}.`);
      for (const id of decline[field]) {
        ref(id, prefix, decline.id);
        if (!available.has(id) || local.has(id) || all.has(id)) throw Error(`${decline.id} repeats or shares ${field} ownership.`);
        local.add(id); all.add(id);
      }
    }
    if (!reasons.has(decline.reason)) throw Error(`${decline.id} has an invalid decline reason.`);
    const actors = new Set([...memberCells].flatMap((cell) => cells.get(cell).actors));
    const wires = new Set([...memberCorridors].flatMap((corridor) => corridors.get(corridor).wires));
    if (decline.witness?.kind === "actor") {
      ref(decline.witness.piston, "piston", decline.id);
      if (!actors.has(decline.witness.piston)) throw Error(`${decline.id} actor witness is outside its cells.`);
    } else if (decline.witness?.kind === "callback") occurrence(decline.witness.occurrence, decline.id);
    else if (decline.witness?.kind === "power") ref(decline.witness.link, "power_link", decline.id);
    else if (decline.witness?.kind === "power_at_cut") {
      ref(decline.witness.actor, "piston", decline.id); ref(decline.witness.link, "power_link", decline.id);
      if (!actors.has(decline.witness.actor) || !["request_sample", "activation_event", "acceptance_revalidation"].includes(decline.witness.phase)) throw Error(`${decline.id} has an invalid power-at-cut witness.`);
    }
    else if (decline.witness?.kind === "wire") {
      ref(decline.witness.node, "node", decline.id);
      if (!wires.has(decline.witness.node) || nodes.get(decline.witness.node)?.kind?.kind !== "wire") throw Error(`${decline.id} wire witness is outside its corridors.`);
    } else throw Error(`${decline.id} has an invalid witness.`);
  }
}

function validateBoundaryBinding(port, ids, context) {
  const { nodes, routes, guards, contacts, plans, wirePlans } = context;
  const binding = port.binding;
  const ref = (id, prefix) => {
    if (typeof id !== "string" || !id.startsWith(`${prefix}:`) || !ids.has(id)) throw Error(`${port.id} has an invalid ${prefix} reference.`);
  };
  const routeOccurrence = (value) => {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw Error(`${port.id} has an invalid route occurrence.`);
    ref(value.route, "route");
    const deliveries = routes.get(value.route).deliveries;
    if (!Number.isInteger(value.ordinal) || value.ordinal < 0 || value.ordinal >= deliveries.length) throw Error(`${port.id} has an invalid route occurrence.`);
    return { route: routes.get(value.route), delivery: deliveries[value.ordinal] };
  };
  const sameFields = (left, right) => left && right && typeof left === "object" && typeof right === "object"
    && !Array.isArray(left) && !Array.isArray(right)
    && Object.keys(left).length === Object.keys(right).length
    && Object.keys(left).every((key) => key in right && left[key] === right[key]);
  const siteTest = (test) => {
    if (!test || typeof test !== "object" || Array.isArray(test)) throw Error(`${port.id} has an invalid site test.`);
    if (!['solid', 'transparent', 'is_wire', 'profile'].includes(test.kind)) throw Error(`${port.id} has an invalid site test.`);
    if (test.kind === "profile") ref(test.profile, "profile");
  };
  const stateTest = (test) => {
    if (!test || typeof test !== "object" || Array.isArray(test)) throw Error(`${port.id} has an invalid state test.`);
    if (["piston_extended", "piston_head_present"].includes(test.kind)) ref(test.piston, "piston");
    else if (test.kind === "piston_availability") {
      ref(test.piston, "piston");
      if (!["ready", "head_pending", "publishing_head_removal", "base_pending_head_present", "base_pending"].includes(test.availability)) throw Error(`${port.id} has an invalid piston availability.`);
    } else if (test.kind === "resource_at") {
      ref(test.resource, "resource"); ref(test.contact, "contact");
    } else if (test.kind === "resource_enabled") ref(test.resource, "resource");
    else throw Error(`${port.id} has an invalid state test.`);
  };

  if (binding.kind === "power") ref(binding.link, "power_link");
  else if (binding.kind === "callback") routeOccurrence(binding.occurrence);
  else if (binding.kind === "wire_relation") ref(binding.relation, "wire_relation");
  else if (binding.kind === "comparator") {
    ref(binding.plan, "comparator_plan");
    if (!["rear", "direct_override", "far_override_site", "far_override_guard", "far_override_output"].includes(binding.role)) throw Error(`${port.id} has an invalid comparator role.`);
    const plan = plans.get(binding.plan);
    if (binding.role === "direct_override" && plan.direct_override == null) throw Error(`${port.id} references an absent direct override.`);
    if (binding.role.startsWith("far_override_")) {
      if (binding.role === "far_override_output") {
        if (plan.far_override?.value?.kind !== "live_component_output") throw Error(`${port.id} references an absent far override output.`);
        ref(plan.far_override.value.node, "node");
      } else {
        const field = binding.role === "far_override_site" ? "site" : "guard";
        ref(plan.far_override?.[field], field);
      }
    }
  } else if (binding.kind === "guard_site") {
    ref(binding.guard, "guard"); ref(binding.site, "site");
    siteTest(binding.test);
    const guard = guards.get(binding.guard);
    if (guard.kind !== "site" || guard.site !== binding.site || guard.view !== binding.view
      || !["live", "captured"].includes(binding.view) || !sameFields(guard.test, binding.test)) {
      throw Error(`${port.id} does not match its guard site atom.`);
    }
  } else if (binding.kind === "guard_state") {
    ref(binding.guard, "guard");
    stateTest(binding.test);
    const guard = guards.get(binding.guard);
    if (guard.kind !== "state" || !sameFields(guard.test, binding.test)) throw Error(`${port.id} does not match its guard state atom.`);
  } else if (binding.kind === "contact_alternative") {
    ref(binding.contact, "contact");
    const variants = contacts.get(binding.contact).variants;
    if (!Number.isInteger(binding.ordinal) || binding.ordinal < 0 || binding.ordinal >= variants.length) throw Error(`${port.id} has an invalid contact alternative.`);
  } else if (binding.kind === "observer_watch") {
    ref(binding.observer, "node"); ref(binding.site, "site");
    const observer = nodes.get(binding.observer);
    if (observer?.kind?.kind !== "observer" || observer.kind.watched !== binding.site) throw Error(`${port.id} does not match its observer watch.`);
  } else if (["captured_wire", "wire_neighborhood"].includes(binding.kind)) {
    ref(binding.wire, "node"); ref(binding.neighbor, "site");
    const wire = nodes.get(binding.wire);
    const plan = wire?.kind?.kind === "wire" ? wirePlans.get(wire.kind.plan) : null;
    if (!plan || !plan.canonical_neighbors.includes(binding.neighbor)) throw Error(`${port.id} has a noncanonical wire neighbor.`);
    if (binding.kind === "captured_wire") {
      const occurrence = routeOccurrence(binding.occurrence);
      if (plan.turbo_route !== binding.occurrence.route || occurrence.delivery.view !== "captured"
        || occurrence.delivery.target !== binding.neighbor) throw Error(`${port.id} does not match its captured wire occurrence.`);
    }
  } else if (binding.kind === "fixed_read") {
    ref(binding.node, "node");
    ref(binding.site, "site");
    if (!["note_support", "note_obstruction"].includes(binding.role)) throw Error(`${port.id} has an invalid fixed-read role.`);
    const node = nodes.get(binding.node), component = node?.kind?.component?.type;
    if (component != null && component !== "note_block") throw Error(`${port.id} fixed-read role does not match its node kind.`);
  } else throw Error(`${port.id} has an unknown boundary binding kind.`);
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
    if ((data.graph.classic_nets || []).some((net) => net.wires.some((wire) => wire.node === sourceNode(link.from.site, index)) || (link.to.kind === 'input' && net.wires.some((wire) => wire.node === link.to.node)))) continue;
    const target = link.to.kind === "input" ? link.to.node : sourceNode(link.to.site, index);
    add(link.id, sourceNode(link.from.site, index), target, "power_link", link);
  }
  for (const relation of data.graph.wire_relations) {
    if ((data.graph.classic_nets || []).some((net) => net.wires.some((wire) => wire.node === relation.from))) continue;
    add(relation.id, relation.from, sourceNode(relation.to, index), "wire_relation", relation);
  }
  for (const route of data.graph.routes || []) {
    route.deliveries.forEach((delivery, ordinal) => {
      if (delivery.execution === 'classic_provenance') return;
      const owners = [...(routeOwners.get(route.id) || [route.id])];
      if (delivery.receiver) return;
      owners.forEach((owner, ownerIndex) => {
        const suffix = owners.length > 1 ? `:owner:${ownerIndex}` : "";
        add(`${route.id}:delivery:${ordinal}${suffix}`, owner, sourceNode(delivery.target, index), "route_delivery", delivery);
      });
    });
  }
  for (const link of data.graph.callback_links || []) {
    if (link.execution === 'classic_provenance') continue;
    add(link.id, link.from, link.to, "callback_link", link);
  }
  for (const net of data.graph.classic_nets || []) {
    add(`${net.id}:ingress`, net.source, net.id, 'classic_signal', net);
    net.inputs.forEach((input, ordinal) => add(`${net.id}:input:${ordinal}`, net.id, input.receiver, 'classic_input', input));
  }
  for (const query of data.graph.fixed_signal_queries || []) {
    const target = query.to.kind === "input" ? query.to.node : sourceNode(query.to.site, index);
    query.inputs.forEach((input, ordinal) => add(`${query.id}:input:${ordinal}`, input.node, target,
      query.semantics === "live_state" ? "fixed_live_read" : "settled_weighted_provenance", { ...query, input }));
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
  if (connection.type === 'classic_signal') return 'committed at original entry occurrence';
  if (connection.type === 'classic_input') return `main · net −${record.attenuation} · live ingress −${record.source_attenuation}`;
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
  if (row.kind?.builtin_reset) return 'built-in reset';
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
  const kinds = [...new Set(['site', 'route', 'classic_net', ...data.graph.nodes.map(category)])].sort();
  $('node-filters').replaceChildren(...kinds.map((kind) => check(kind, kind.replaceAll('_', ' '), true, 'node-filter')));
  const edgeTypes = ['power_link', 'wire_relation', 'route_delivery', 'callback_link', 'classic_signal', 'classic_input'];
  $('edge-filters').replaceChildren(...edgeTypes.map((kind) => check(kind, kind.replaceAll('_', ' '), kind === 'power_link' || kind.startsWith('classic_'), 'edge-filter')));
  document.querySelectorAll('.node-filter').forEach((input) => input.addEventListener('change', applyFilters));
  document.querySelectorAll('.edge-filter').forEach((input) => input.addEventListener('change', () => renderView()));
  $('snapshot').replaceChildren(option('', 'All potential connections'), ...data.snapshots.map((snapshot) => option(snapshot.id, snapshot.id)));
  $('snapshot').value = '';
  const layers = [...new Set(data.graph.nodes.concat(data.graph.sites).map((row) => row.position?.y).filter(Number.isFinite))].sort((a, b) => a - b);
  $('layer').replaceChildren(option('', 'All layers'), ...layers.map((y) => option(String(y), `Y = ${y}`)));
  $('layer').value = '';
  const fixedPlan = data.graph.fixed_classic;
  let fixedSummary = 'fixed classic: not included';
  if (fixedPlan?.status === 'declined') {
    fixedSummary = `fixed classic: declined · ${fixedPlan.decline || 'reason unavailable'}`;
  } else if (fixedPlan?.status === 'compiled') {
    const inventory = fixedPlan.inventory || {};
    const tables = Array.isArray(inventory.tables) ? inventory.tables : [];
    const counts = Object.fromEntries(tables.filter((table) => typeof table?.table === 'string' && Number.isInteger(table.entries)).map((table) => [table.table, table.entries]));
    const bytes = Number.isInteger(inventory.accounted_bytes) ? ` · ${inventory.accounted_bytes} accounted bytes` : '';
    fixedSummary = `fixed classic: compiled · ${counts.nodes ?? '?'} nodes · ${counts.wires ?? '?'} wires · ${counts.route_ops ?? '?'} route ops${bytes}`;
  }
  $('availability').textContent = `Routes: ${data.graph.routes === null ? 'unavailable' : data.graph.routes.length} · trace: ${data.trace === null ? 'unavailable' : (data.trace || []).length} · ${data.snapshots.length} snapshots · ${fixedSummary}`;
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


function validateInstantMachine(machine, graph) {
  if (machine == null) return;
  if (machine.encoding !== "closed-instant-machine-v1" || !Array.isArray(machine.controls)
    || machine.controls.length < 1 || machine.controls.length > 4 || !Array.isArray(machine.states)
    || machine.states.length < 1 || machine.states.length > 2048 || !Array.isArray(machine.transitions)
    || machine.transitions.length !== machine.states.length) throw Error("Invalid instant machine dimensions.");
  for (const [field, table] of [["components", "nodes"], ["pistons", "pistons"], ["resources", "resources"]]) {
    const owners = machine.owners?.[field];
    const ids = new Set(graph[table].map((row) => row.id));
    if (!Array.isArray(owners) || owners.length !== ids.size || new Set(owners).size !== ids.size
      || owners.some((id) => !ids.has(id)) || machine.states.some((state) => !Array.isArray(state?.[field])
        || state[field].length !== owners.length)) throw Error("Invalid instant machine owners/state dimensions.");
  }
  for (const row of machine.transitions) {
    if (!Array.isArray(row) || row.length !== machine.controls.length + 1 || row.some((edge) => edge != null
      && (!Number.isInteger(edge.next) || edge.next < 0 || edge.next >= machine.states.length))) throw Error("Invalid instant machine transition.");
  }
}

function validateInstantKernel(kernel, graph) {
  if (kernel == null) return;
  const fail = () => { throw Error("Invalid compact instant kernel."); };
  const nodeIds = graph.nodes.map((row) => row.id);
  const pistonIds = graph.pistons.map((row) => row.id);
  const resourceIds = graph.resources.map((row) => row.id);
  const contactIds = graph.contacts.map((row) => row.id);
  const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
  if (!isObject(kernel) || kernel.encoding !== "compact-instant-kernel-v1"
    || !nodeIds.includes(kernel.trigger) || !Array.isArray(kernel.data_controls)
    || kernel.data_controls.length > 3 || kernel.data_controls.some((id) => !nodeIds.includes(id))
    || kernel.data_controls.includes(kernel.trigger) || new Set(kernel.data_controls).size !== kernel.data_controls.length
    || !Array.isArray(kernel.expressions) || kernel.expressions.length < 1 || kernel.expressions.length > 1_000_000
    || !Number.isSafeInteger(kernel.estimated_runtime_bytes) || kernel.estimated_runtime_bytes < 0) fail();

  for (let index = 0; index < kernel.expressions.length; index++) {
    const expression = kernel.expressions[index];
    if (!isObject(expression)) fail();
    if (expression.kind === "constant") {
      if (typeof expression.value !== "string" || expression.value.length > 20 || !/^(0|[1-9][0-9]*)$/.test(expression.value)
        || BigInt(expression.value) > 18446744073709551615n) fail();
    } else if (expression.kind === "select") {
      if (!Number.isInteger(expression.control) || expression.control < 0 || expression.control >= kernel.data_controls.length
        || !Number.isInteger(expression.low) || expression.low < 0 || expression.low >= index
        || !Number.isInteger(expression.high) || expression.high < 0 || expression.high >= index) fail();
    } else fail();
  }

  const phases = kernel.phases;
  if (!Number.isSafeInteger(kernel.prefix) || kernel.prefix < 0 || !Number.isSafeInteger(kernel.period)
    || kernel.period < 1 || !Array.isArray(phases) || kernel.prefix + kernel.period !== phases.length || phases.length > 128) fail();
  const owners = kernel.owners;
  const sameIds = (values, expected) => Array.isArray(values) && values.length === expected.length
    && new Set(values).size === expected.length && values.every((id) => expected.includes(id));
  if (!isObject(owners) || !sameIds(owners.components, nodeIds) || !sameIds(owners.pistons, pistonIds)
    || !sameIds(owners.resources, resourceIds) || !sameIds(owners.contacts, contactIds)) fail();

  const root = (value) => Number.isInteger(value) && value >= 0 && value < kernel.expressions.length;
  const roots = (values) => Array.isArray(values) && values.every(root);
  const stateRoots = (state) => {
    if (!isObject(state) || !roots(state.components) || state.components.length !== nodeIds.length
      || !roots(state.pistons) || state.pistons.length !== pistonIds.length
      || !Array.isArray(state.resources) || state.resources.length !== resourceIds.length
      || state.resources.some((pair) => !Array.isArray(pair) || pair.length !== 2 || !roots(pair))) return false;
    const future = state.future;
    if (!isObject(future) || !root(future.tick_len) || !Array.isArray(future.ticks) || future.ticks.length > 128
      || !future.ticks.every((tick) => isObject(tick) && ["node", "delay", "priority"].every((field) => root(tick[field])))
      || !root(future.request_len) || !Array.isArray(future.requests) || future.requests.length > 64
      || !future.requests.every((request) => isObject(request) && ["piston", "action"].every((field) => root(request[field])))
      || !root(future.deferred_len) || !Array.isArray(future.deferred) || future.deferred.length > 128
      || !future.deferred.every((event) => isObject(event)
        && ["piston", "revision", "resource", "resource_revision", "contact", "delay", "extending"].every((field) => root(event[field])))) return false;
    return true;
  };
  if (!stateRoots(kernel.entry)) fail();
  for (const phase of phases) {
    if (!isObject(phase) || !stateRoots(phase.state)
      || !["scheduled", "accepted", "published"].every((field) => root(phase[field]))) fail();
    if (!Array.isArray(phase.components) || !phase.components.every((delta) => isObject(delta)
      && nodeIds.includes(delta.node) && root(delta.changed) && root(delta.value))) fail();
    if (!Array.isArray(phase.dirty) || !phase.dirty.every((delta) => isObject(delta)
      && nodeIds.includes(delta.node) && root(delta.touched))) fail();
    if (!Array.isArray(phase.piston_revisions) || !phase.piston_revisions.every((delta) => isObject(delta)
      && pistonIds.includes(delta.piston) && root(delta.increment))) fail();
    if (!Array.isArray(phase.resource_revisions) || !phase.resource_revisions.every((delta) => isObject(delta)
      && resourceIds.includes(delta.resource) && root(delta.increment))) fail();
    if (!Array.isArray(phase.effects) || !phase.effects.every((effect) => isObject(effect)
      && pistonIds.includes(effect.piston) && root(effect.enabled)
      && ["extend", "retract", "retract_without_pull"].includes(effect.action) && typeof effect.extended === "boolean")) fail();
  }
}
