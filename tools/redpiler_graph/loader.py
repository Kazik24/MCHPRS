"""Load and validate versioned Redpiler graph exports without dependencies."""

from dataclasses import dataclass
import json
from pathlib import Path
import re
from typing import Any

MAX_GRAPH_BYTES = 64 * 1024 * 1024
TABLES = {
    "node": "nodes",
    "profile": "profiles",
    "site": "sites",
    "guard": "guards",
    "contact": "contacts",
    "piston": "pistons",
    "resource": "resources",
    "power_link": "power_links",
    "wire": "wire_plans",
    "dust_path": "dust_paths",
    "classic_net": "classic_nets",
    "instant_region": "instant_regions",
    "instant_proxy": "instant_proxies",
    "instant_cell": "instant_cells",
    "instant_corridor": "instant_corridors",
    "instant_port": "instant_boundary_ports",
    "instant_decline": "instant_declines",
    "instant_layer_region": "instant_layer_regions",
    "instant_layer_decline": "instant_layer_declines",
    "wire_relation": "wire_relations",
    "comparator_plan": "comparator_plans",
    "callback_link": "callback_links",
    "route": "routes",
}
ID_PATTERN = re.compile(r"([a-z_]+):(0|[1-9][0-9]*)\Z")


class GraphFormatError(ValueError):
    """The file is not a supported, internally consistent graph export."""


@dataclass(frozen=True)
class GraphDocument:
    path: Path
    data: dict[str, Any]
    records: dict[str, dict[str, dict[str, Any]]]

    def record(self, identifier: str) -> dict[str, Any]:
        """Return a typed graph record or raise a readable lookup error."""
        match = ID_PATTERN.fullmatch(identifier)
        if match is None:
            raise GraphFormatError(f"invalid typed ID: {identifier}")
        prefix = match.group(1)
        try:
            return self.records[prefix][identifier]
        except KeyError as error:
            raise GraphFormatError(f"unknown {prefix} ID: {identifier}") from error


def load_graph(path: str | Path, *, max_bytes: int = MAX_GRAPH_BYTES) -> GraphDocument:
    """Read a bounded UTF-8 graph file and validate IDs and references."""
    source = Path(path)
    try:
        size = source.stat().st_size
        if size > max_bytes:
            raise GraphFormatError(f"graph is {size} bytes; limit is {max_bytes}")
        data = json.loads(source.read_text(encoding="utf-8"))
    except GraphFormatError:
        raise
    except (OSError, UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise GraphFormatError(f"cannot read graph JSON {source}: {error}") from error

    records = _validate(data)
    return GraphDocument(source, data, records)


def _object(value: Any, where: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise GraphFormatError(f"{where} must be an object")
    return value


def _array(value: Any, where: str) -> list[Any]:
    if not isinstance(value, list):
        raise GraphFormatError(f"{where} must be an array")
    return value


def _typed_refs(value: Any) -> list[str]:
    if isinstance(value, str) and re.match(r"[a-z_]+:", value):
        return [value]
    if isinstance(value, dict):
        return [reference for child in value.values() for reference in _typed_refs(child)]
    if isinstance(value, list):
        return [reference for child in value for reference in _typed_refs(child)]
    return []


def _validate_site_test(test: Any, owner: str, ref: Any) -> dict[str, Any]:
    test = _object(test, f"{owner}.test")
    if test.get("kind") not in ("solid", "transparent", "is_wire", "profile"):
        raise GraphFormatError(f"{owner} has an invalid site test")
    if test.get("kind") == "profile":
        ref("profile", test.get("profile"), owner)
    return test


def _validate_state_test(test: Any, owner: str, ref: Any) -> dict[str, Any]:
    test = _object(test, f"{owner}.test")
    kind = test.get("kind")
    if kind in ("piston_extended", "piston_head_present"):
        ref("piston", test.get("piston"), owner)
    elif kind == "piston_availability":
        ref("piston", test.get("piston"), owner)
        if test.get("availability") not in ("ready", "head_pending", "publishing_head_removal",
                                              "base_pending_head_present", "base_pending"):
            raise GraphFormatError(f"{owner} has an invalid piston availability")
    elif kind == "resource_at":
        ref("resource", test.get("resource"), owner)
        ref("contact", test.get("contact"), owner)
    elif kind == "resource_enabled":
        ref("resource", test.get("resource"), owner)
    else:
        raise GraphFormatError(f"{owner} has an invalid state test")
    return test


def _route_occurrence(value: Any, owner: str, records: dict, ref: Any) -> tuple[str, int, dict[str, Any]]:
    occurrence = _object(value, f"{owner}.occurrence")
    route = occurrence.get("route")
    ref("route", route, owner)
    ordinal = occurrence.get("ordinal")
    deliveries = records["route"][route]["deliveries"]
    if type(ordinal) is not int or not 0 <= ordinal < len(deliveries):
        raise GraphFormatError(f"{owner} has an invalid route occurrence")
    return route, ordinal, deliveries[ordinal]


def _validate(data: Any) -> dict[str, dict[str, dict[str, Any]]]:
    root = _object(data, "graph document")
    if root.get("format") != "mchprs.redpiler.graph":
        raise GraphFormatError("unsupported graph format")
    version = _object(root.get("version"), "version")
    if version.get("major") != 1:
        raise GraphFormatError(f"unsupported graph major version: {version.get('major')}")
    if not isinstance(version.get("minor"), int) or version["minor"] < 0:
        raise GraphFormatError("version.minor must be a nonnegative integer")
    if root.get("stage") not in ("electrical_draft", "callback_draft", "program"):
        raise GraphFormatError(f"unsupported graph stage: {root.get('stage')}")

    graph = _object(root.get("graph"), "graph")
    records: dict[str, dict[str, dict[str, Any]]] = {}
    for prefix, table in TABLES.items():
        values = graph.get(table)
        if table in ("routes", "callback_links", "dust_paths", "instant_regions", "instant_proxies", "classic_nets",
                     "instant_cells", "instant_corridors", "instant_boundary_ports", "instant_declines",
                     "instant_layer_regions", "instant_layer_declines") and values is None:
            values = []
        values = _array(values, f"graph.{table}")
        indexed: dict[str, dict[str, Any]] = {}
        for index, value in enumerate(values):
            item = _object(value, f"graph.{table}[{index}]")
            identifier = item.get("id")
            match = ID_PATTERN.fullmatch(identifier) if isinstance(identifier, str) else None
            if match is None or match.group(1) != prefix:
                raise GraphFormatError(f"invalid {prefix} ID in graph.{table}[{index}]: {identifier!r}")
            if identifier in indexed:
                raise GraphFormatError(f"duplicate {prefix} ID: {identifier}")
            indexed[identifier] = item
        records[prefix] = indexed

    snapshots = _array(root.get("snapshots"), "snapshots")
    snapshot_records: dict[str, dict[str, Any]] = {}
    for index, value in enumerate(snapshots):
        item = _object(value, f"snapshots[{index}]")
        identifier = item.get("id")
        if not isinstance(identifier, str) or not identifier.startswith("snapshot:"):
            raise GraphFormatError(f"invalid snapshot ID: {identifier!r}")
        if identifier in snapshot_records:
            raise GraphFormatError(f"duplicate snapshot ID: {identifier}")
        snapshot_records[identifier] = item
    records["snapshot"] = snapshot_records

    def ref(prefix: str, identifier: Any, owner: str) -> None:
        if not isinstance(identifier, str) or identifier not in records[prefix]:
            raise GraphFormatError(f"{owner} references missing {prefix} ID {identifier!r}")

    def optional(prefix: str, identifier: Any, owner: str) -> None:
        if identifier is not None:
            ref(prefix, identifier, owner)

    for identifier, node in records["node"].items():
        optional("route", node.get("output_route"), identifier)
        kind = _object(node.get("kind"), f"{identifier}.kind")
        node_type = kind.get("kind")
        if "builtin_reset" in kind and node_type != "observer":
            raise GraphFormatError(f"{identifier}.builtin_reset requires an observer projection")
        if node_type == "observer":
            ref("site", kind.get("watched"), identifier)
            if "builtin_reset" in kind and type(kind["builtin_reset"]) is not bool:
                raise GraphFormatError(f"{identifier}.builtin_reset must be a boolean")
            if kind.get("builtin_reset") and node.get("output_route") is not None:
                raise GraphFormatError(f"{identifier} built-in reset must not have an output route")
        elif node_type == "wire":
            ref("wire", kind.get("plan"), identifier)
        elif node_type == "piston":
            ref("piston", kind.get("piston"), identifier)
        elif node_type == "contact":
            ref("contact", kind.get("contact"), identifier)
        elif node_type != "electrical":
            raise GraphFormatError(f"{identifier} has unknown node kind {node_type!r}")

    for identifier, site in records["site"].items():
        source = _object(site.get("source"), f"{identifier}.source")
        source_kind = source.get("kind")
        if source_kind == "component":
            ref("node", source.get("node"), identifier)
        elif source_kind == "contact":
            ref("contact", source.get("contact"), identifier)
        elif source_kind == "constant":
            ref("profile", source.get("profile"), identifier)
        else:
            raise GraphFormatError(f"{identifier} has unknown site source {source_kind!r}")

    for index, (identifier, guard) in enumerate(records["guard"].items()):
        kind = guard.get("kind")
        test = guard.get("test")
        if kind == "state":
            test = _object(test, f"{identifier}.test")
            test_kind = test.get("kind")
            if test_kind in ("piston_extended", "piston_head_present", "piston_availability"):
                ref("piston", test.get("piston"), identifier)
            elif test_kind == "resource_at":
                ref("resource", test.get("resource"), identifier)
                ref("contact", test.get("contact"), identifier)
            elif test_kind == "resource_enabled":
                ref("resource", test.get("resource"), identifier)
            else:
                raise GraphFormatError(f"{identifier} has unknown state test {test_kind!r}")
        elif kind == "site":
            ref("site", guard.get("site"), identifier)
            test = _object(test, f"{identifier}.test")
            if test.get("kind") == "profile":
                ref("profile", test.get("profile"), identifier)
        elif kind == "not":
            ref("guard", guard.get("guard"), identifier)
        elif kind in ("all", "any"):
            for child in _array(guard.get("guards"), f"{identifier}.guards"):
                ref("guard", child, identifier)
        elif kind != "always":
            raise GraphFormatError(f"{identifier} has unknown guard kind {kind!r}")
        if kind in ("not", "all", "any"):
            children = [guard.get("guard")] if kind == "not" else guard["guards"]
            if any(int(child.split(":", 1)[1]) >= index for child in children):
                raise GraphFormatError(f"{identifier} must reference earlier guards")

    for identifier, contact in records["contact"].items():
        ref("node", contact.get("node"), identifier)
        for variant in _array(contact.get("variants"), f"{identifier}.variants"):
            variant = _object(variant, f"{identifier}.variant")
            ref("guard", variant.get("when"), identifier)
            ref("profile", variant.get("profile"), identifier)
            receiver = variant.get("receiver")
            if receiver is not None:
                receiver = _object(receiver, f"{identifier}.receiver")
                receiver_kind = receiver.get("kind")
                target = {"component": "node", "piston_base": "piston", "piston_head": "piston", "contact": "contact"}.get(receiver_kind)
                if target is None:
                    raise GraphFormatError(f"{identifier} has unknown receiver kind {receiver_kind!r}")
                ref(target, receiver.get(target), identifier)

    for identifier, piston in records["piston"].items():
        for key, prefix in (("node", "node"), ("base", "contact"), ("near", "contact"), ("far", "contact")):
            ref(prefix, piston.get(key), identifier)
        lane = _object(piston.get("lane"), f"{identifier}.lane")
        if lane.get("kind") == "single_resource":
            ref("contact", lane.get("far"), identifier)
        elif lane.get("kind") != "empty":
            raise GraphFormatError(f"{identifier} has unknown lane kind {lane.get('kind')!r}")
        routes = piston.get("routes")
        if routes is not None:
            routes = _object(routes, f"{identifier}.routes")
            for route in routes.values():
                optional("route", route, identifier)

    for identifier, resource in records["resource"].items():
        ref("profile", resource.get("material"), identifier)
        for contact in _array(resource.get("contacts"), f"{identifier}.contacts"):
            ref("contact", contact, identifier)

    for identifier, link in records["power_link"].items():
        source = _object(link.get("from"), f"{identifier}.from")
        ref("site", source.get("site"), identifier)
        target = _object(link.get("to"), f"{identifier}.to")
        if target.get("kind") == "input":
            ref("node", target.get("node"), identifier)
        elif target.get("kind") == "conductor":
            ref("site", target.get("site"), identifier)
        else:
            raise GraphFormatError(f"{identifier} has unknown target kind {target.get('kind')!r}")
        ref("guard", link.get("guard"), identifier)

    instant_wires: set[str] = set()
    for identifier, wire in records["wire"].items():
        ref("node", wire.get("node"), identifier)
        neighbors = _array(wire.get("canonical_neighbors"), f"{identifier}.canonical_neighbors")
        if len(neighbors) != 24:
            raise GraphFormatError(f"{identifier} must have exactly 24 canonical neighbors")
        for site in neighbors:
            ref("site", site, identifier)
        optional("route", wire.get("shape_route"), identifier)
        optional("route", wire.get("turbo_route"), identifier)
        root = wire.get("hop_root")
        if root is not None:
            ref("site", root, identifier)
            if records["site"][root].get("source") != {"kind": "component", "node": wire["node"]}:
                raise GraphFormatError(f"{identifier} hop root must bind its component")
        instant = wire.get("instant_reset")
        if instant is not None:
            instant = _object(instant, f"{identifier}.instant_reset")
            for kind, field in (("piston", "piston"), ("resource", "resource"), ("node", "observer"), ("node", "partner")):
                ref(kind, instant.get(field), identifier)
            owner, partner, observer = wire["node"], instant["partner"], instant["observer"]
            if (records["node"][owner]["kind"].get("kind") != "wire"
                    or records["node"][partner]["kind"].get("kind") != "wire"
                    or records["node"][observer]["kind"].get("kind") != "observer"
                    or owner == partner or {owner, partner} & instant_wires):
                raise GraphFormatError(f"{identifier} has invalid or overlapping instant reset bindings")
            instant_wires.update((owner, partner))
        facts = wire.get("query_facts")
        if facts is not None:
            facts = _object(facts, f"{identifier}.query_facts")
            if "isolated" in facts and type(facts["isolated"]) is not bool:
                raise GraphFormatError(f"{identifier}.query_facts.isolated must be a boolean")
            masks = [facts.get(key) for key in ("known", "solid", "transparent", "wire", "zero_no_dust")]
            if any(type(mask) is not int or not 0 <= mask <= 0xffffff for mask in masks):
                raise GraphFormatError(f"{identifier} has invalid wire query fact masks")
            known, solid, transparent, is_wire, zero_no_dust = masks
            if (solid | transparent | is_wire | zero_no_dust) & ~known or zero_no_dust & solid:
                raise GraphFormatError(f"{identifier} has invalid wire query fact masks")
            transfer = facts.get("transfer")
            if transfer is not None:
                transfer = _object(transfer, f"{identifier}.query_facts.transfer")
                diagonals = sum(1 << slot for slot in (7, 8, 12, 13, 17, 18, 20, 21))
                for phase in ("seed", "turbo"):
                    inputs = _object(transfer.get(phase), f"{identifier}.transfer.{phase}")
                    direct, conditional = inputs.get("direct"), inputs.get("conditional")
                    if (any(type(mask) is not int or not 0 <= mask <= 0xffffff for mask in (direct, conditional))
                            or direct & ~(diagonals | 0x3f) or conditional & ~diagonals
                            or direct & conditional or phase == "turbo" and direct & 0xc):
                        raise GraphFormatError(f"{identifier} has invalid wire transfer inputs")

    assigned_dust_wires: set[str] = set()
    for identifier, path in records["dust_path"].items():
        input_node = path.get("input_node")
        input_site = path.get("input_site")
        ref("node", input_node, identifier)
        ref("site", input_site, identifier)
        if records["node"][input_node]["kind"].get("kind") != "electrical":
            raise GraphFormatError(f"{identifier} input must be an electrical node")
        if records["site"][input_site]["source"] != {"kind": "component", "node": input_node}:
            raise GraphFormatError(f"{identifier} input site must be owned by its input node")
        if path.get("input_face") not in ("north", "south", "east", "west"):
            raise GraphFormatError(f"{identifier} input face must be horizontal")
        wires = _array(path.get("wires"), f"{identifier}.wires")
        if len(wires) < 2:
            raise GraphFormatError(f"{identifier} must contain at least two wire nodes")
        local_wires: set[str] = set()
        for wire in wires:
            ref("node", wire, identifier)
            if records["node"][wire]["kind"].get("kind") != "wire":
                raise GraphFormatError(f"{identifier} references non-wire node {wire!r}")
            if wire in local_wires or wire in assigned_dust_wires:
                raise GraphFormatError(f"{identifier} repeats or shares wire node {wire}")
            local_wires.add(wire)
            assigned_dust_wires.add(wire)

    for identifier, region in records["instant_region"].items():
        if type(region.get("stable_inputs", False)) is not bool:
            raise GraphFormatError(f"{identifier} stable_inputs must be boolean")
        for wire in _array(region.get("wires", []), f"{identifier}.wires"):
            ref("node", wire, identifier)
            if records["node"][wire]["kind"].get("kind") != "wire":
                raise GraphFormatError(f"{identifier} references a non-wire internal node")
        for field, kind in {"actors": "piston", "resources": "resource", "resets": "node",
                            "contacts": "contact", "readable_sites": "site", "power_inputs": "power_link",
                            "power_readers": "power_link", "routes": "route", "wire_readers": "node", "observers": "node"}.items():
            for reference in _array(region.get(field), f"{identifier}.{field}"):
                ref(kind, reference, identifier)
        for field in ("incoming", "outgoing"):
            for event in _array(region.get(field), f"{identifier}.{field}"):
                event = _object(event, identifier)
                ref("route", event.get("route"), identifier)
                ordinal = event.get("ordinal")
                if type(ordinal) is not int or not 0 <= ordinal < len(records["route"][event["route"]]["deliveries"]):
                    raise GraphFormatError(f"{identifier} has an invalid route occurrence")

    proxy_routes: set[str] = set()
    for identifier, proxy in records["instant_proxy"].items():
        ref("instant_region", proxy.get("region"), identifier)
        region = records["instant_region"][proxy["region"]]
        private_resources: set[str] = set()
        for cell in _array(proxy.get("private_cells"), f"{identifier}.private_cells"):
            cell = _object(cell, identifier)
            for field, kind, members in (("actor", "piston", "actors"), ("resource", "resource", "resources"), ("reset", "node", "resets")):
                ref(kind, cell.get(field), identifier)
                if cell[field] not in region[members]:
                    raise GraphFormatError(f"{identifier} private cell is outside its region")
            if cell["resource"] in private_resources:
                raise GraphFormatError(f"{identifier} repeats a private resource")
            private_resources.add(cell["resource"])
        for plan in _array(proxy.get("routes"), f"{identifier}.routes"):
            plan = _object(plan, identifier)
            ref("route", plan.get("route"), identifier)
            route = plan["route"]
            operations = _array(plan.get("operations"), f"{identifier}.operations")
            if (route not in region["routes"] or route in proxy_routes
                    or len(operations) != len(records["route"][route]["deliveries"])):
                raise GraphFormatError(f"{identifier} has an invalid proxy route")
            for operation in operations:
                operation = _object(operation, identifier)
                kind = operation.get("kind")
                if kind == "sample" and operation.get("piston") in region["actors"]:
                    continue
                if kind == "arm_reset" and operation.get("node") in region["resets"]:
                    continue
                if kind != "dispatch":
                    raise GraphFormatError(f"{identifier} has an invalid proxy operation")
            proxy_routes.add(route)

    for identifier, relation in records["wire_relation"].items():
        ref("node", relation.get("from"), identifier)
        ref("site", relation.get("to"), identifier)
        ref("guard", relation.get("guard"), identifier)

    for identifier, plan in records["comparator_plan"].items():
        ref("node", plan.get("node"), identifier)
        ref("site", plan.get("rear"), identifier)
        _validate_override(plan.get("direct_override"), identifier, ref)
        far = plan.get("far_override")
        if far is not None:
            far = _object(far, f"{identifier}.far_override")
            ref("site", far.get("site"), identifier)
            ref("guard", far.get("guard"), identifier)
            _validate_override(far.get("value"), identifier, ref)

    for identifier, route in records["route"].items():
        optional("node", route.get("source"), identifier)
        for delivery in _array(route.get("deliveries"), f"{identifier}.deliveries"):
            delivery = _object(delivery, f"{identifier}.delivery")
            ref("site", delivery.get("target"), identifier)
            ref("guard", delivery.get("guard"), identifier)
            receiver = delivery.get("receiver")
            if receiver is not None:
                receiver = _object(receiver, f"{identifier}.receiver")
                receiver_kind = receiver.get("kind")
                target = {"component": "node", "piston_base": "piston", "piston_head": "piston", "contact": "contact"}.get(receiver_kind)
                if target is None:
                    raise GraphFormatError(f"{identifier} has unknown route receiver kind {receiver_kind!r}")
                ref(target, receiver.get(target), identifier)

    for identifier, link in records["callback_link"].items():
        ref("node", link.get("from"), identifier)
        ref("node", link.get("to"), identifier)
        ref("guard", link.get("guard"), identifier)
        ref("route", link.get("route"), identifier)

    for index, snapshot in enumerate(snapshots):
        identifier = snapshot["id"]
        for component in _array(snapshot.get("components"), f"{identifier}.components"):
            component = _object(component, f"{identifier}.component")
            ref("node", component.get("node"), identifier)
        for piston in _array(snapshot.get("pistons"), f"{identifier}.pistons"):
            ref("piston", _object(piston, f"{identifier}.piston").get("piston"), identifier)
        for resource in _array(snapshot.get("resources"), f"{identifier}.resources"):
            resource = _object(resource, f"{identifier}.resource")
            ref("resource", resource.get("resource"), identifier)
            ref("contact", resource.get("selected_contact"), identifier)

    classic_wires = set(assigned_dust_wires) | set(instant_wires)
    classic_receivers = set()
    classic_entries = set()
    for identifier, net in records["classic_net"].items():
        ref("node", net.get("source"), identifier)
        if type(net.get("initial_signal")) is not int or not 0 <= net["initial_signal"] <= 15:
            raise GraphFormatError(f"{identifier} has invalid initial signal")
        wires = _array(net.get("wires"), f"{identifier}.wires")
        if not wires:
            raise GraphFormatError(f"{identifier} has no private wires")
        for wire in wires:
            node = wire.get("node")
            ref("node", node, identifier)
            if node in classic_wires or records["node"][node]["kind"]["kind"] != "wire":
                raise GraphFormatError(f"{identifier} has overlapping or non-wire ownership")
            classic_wires.add(node)
            if type(wire.get("attenuation")) is not int or not 0 <= wire["attenuation"] <= 15:
                raise GraphFormatError(f"{identifier} has invalid attenuation")
        for value in _array(net.get("inputs"), f"{identifier}.inputs"):
            ref("node", value.get("receiver"), identifier)
            if value["receiver"] in classic_receivers:
                raise GraphFormatError(f"{identifier} shares receiver ownership")
            classic_receivers.add(value["receiver"])
            for key in ("attenuation", "source_attenuation"):
                if type(value.get(key)) is not int or not 0 <= value[key] <= 15:
                    raise GraphFormatError(f"{identifier} has invalid direct input")
        for entry in _array(net.get("entries"), f"{identifier}.entries"):
            ref("route", entry.get("route"), identifier)
            ref("node", entry.get("wire"), identifier)
            ordinal = entry.get("ordinal")
            if type(ordinal) is not int or not 0 <= ordinal < len(records["route"][entry["route"]]["deliveries"]) or type(entry.get("commit")) is not bool:
                raise GraphFormatError(f"{identifier} has invalid route occurrence")
            binding = (entry["route"], ordinal)
            if binding in classic_entries:
                raise GraphFormatError(f"{identifier} repeats a route occurrence")
            classic_entries.add(binding)
            for op in _array(entry.get("ops"), f"{identifier}.ops"):
                ref("node", op.get("receiver"), identifier)
                masks = _array(op.get("when"), f"{identifier}.when")
                sample = op.get("sample")
                valid_sample = type(op.get("nonzero")) is bool
                if sample is not None:
                    valid_sample = isinstance(sample, dict) and (
                        sample.get("kind") == "boolean" and valid_sample and sample.get("nonzero") is op["nonzero"]
                        or sample.get("kind") == "comparator" and type(sample.get("strength")) is int
                        and 0 <= sample["strength"] <= 15 and type(sample.get("powered")) is bool
                    )
                if not valid_sample or len(masks) != 4 or any(not isinstance(mask, str) or re.fullmatch(r"[0-9a-f]{16}", mask) is None for mask in masks):
                    raise GraphFormatError(f"{identifier} has invalid operation mask")

    boundary_tables = ("instant_cell", "instant_corridor", "instant_port", "instant_decline")
    if any(records[prefix] for prefix in boundary_tables) and (records["instant_region"] or records["instant_proxy"]):
        raise GraphFormatError("instant boundary tables cannot coexist with legacy instant regions or proxies")

    cell_actors: set[str] = set()
    cell_resources: set[str] = set()
    cell_resets: set[str] = set()
    for identifier, cell in records["instant_cell"].items():
        for field, prefix, owned in (("actors", "piston", cell_actors),
                                     ("resources", "resource", cell_resources),
                                     ("resets", "node", cell_resets)):
            members = _array(cell.get(field), f"{identifier}.{field}")
            local: set[str] = set()
            for member in members:
                ref(prefix, member, identifier)
                if prefix == "node" and records["node"][member]["kind"].get("kind") != "observer":
                    raise GraphFormatError(f"{identifier} reset must be an observer node")
                if member in local or member in owned:
                    raise GraphFormatError(f"{identifier} repeats or shares {field} ownership")
                local.add(member)
                owned.add(member)

    declined_actors: set[str] = set()
    for identifier, decline in records["instant_decline"].items():
        for actor in _array(decline.get("actors"), f"{identifier}.actors"):
            ref("piston", actor, identifier)
            if actor in declined_actors:
                raise GraphFormatError(f"{identifier} repeats declined actor ownership")
            declined_actors.add(actor)
        reason = _object(decline.get("reason"), f"{identifier}.reason")
        code = reason.get("code")
        if not isinstance(code, str) or re.fullmatch(r"[a-z][a-z0-9]*(?:_[a-z0-9]+)*", code) is None:
            raise GraphFormatError(f"{identifier} has an invalid decline reason code")
        for prefix in ("piston", "resource", "contact"):
            optional(prefix, reason.get(prefix), identifier)
    if cell_actors & declined_actors:
        raise GraphFormatError("an instant actor cannot be both admitted and declined")

    corridor_wires: set[str] = set()
    corridor_sites: set[str] = set()
    for identifier, corridor in records["instant_corridor"].items():
        for field, prefix, owned in (("wires", "node", corridor_wires), ("conductive_sites", "site", corridor_sites)):
            members = _array(corridor.get(field), f"{identifier}.{field}")
            local: set[str] = set()
            for member in members:
                ref(prefix, member, identifier)
                if field == "wires" and records["node"][member]["kind"].get("kind") != "wire":
                    raise GraphFormatError(f"{identifier} references a non-wire corridor node")
                if member in local or member in owned:
                    raise GraphFormatError(f"{identifier} repeats or shares {field} ownership")
                local.add(member)
                owned.add(member)
    classic_owned_wires = {
        wire["node"] for net in records["classic_net"].values()
        for wire in _array(net.get("wires"), f"{net['id']}.wires")
    }
    if corridor_wires & classic_owned_wires:
        raise GraphFormatError("instant corridors cannot own classic-owned wires")

    for identifier, port in records["instant_port"].items():
        owner = port.get("owner")
        if isinstance(owner, str) and owner.startswith("instant_cell:"):
            ref("instant_cell", owner, identifier)
        elif isinstance(owner, str) and owner.startswith("instant_corridor:"):
            ref("instant_corridor", owner, identifier)
        else:
            raise GraphFormatError(f"{identifier} has an invalid instant boundary owner")
        if port.get("direction") not in ("input", "output"):
            raise GraphFormatError(f"{identifier} has an invalid boundary direction")
        binding = _object(port.get("binding"), f"{identifier}.binding")
        kind = binding.get("kind")
        if kind == "power":
            ref("power_link", binding.get("link"), identifier)
        elif kind == "callback":
            _route_occurrence(binding.get("occurrence"), identifier, records, ref)
        elif kind == "wire_relation":
            ref("wire_relation", binding.get("relation"), identifier)
        elif kind == "comparator":
            ref("comparator_plan", binding.get("plan"), identifier)
            role = binding.get("role")
            if role not in ("rear", "direct_override", "far_override_site", "far_override_guard", "far_override_output"):
                raise GraphFormatError(f"{identifier} has an invalid comparator role")
            plan = records["comparator_plan"][binding["plan"]]
            if role == "direct_override" and plan.get("direct_override") is None:
                raise GraphFormatError(f"{identifier} references an absent direct override")
            if role.startswith("far_override_"):
                far = plan.get("far_override")
                if far is None:
                    raise GraphFormatError(f"{identifier} references an absent far override")
                if role == "far_override_output":
                    value = _object(far.get("value"), f"{identifier}.far_override.value")
                    if value.get("kind") != "live_component_output":
                        raise GraphFormatError(f"{identifier} references an absent far override output")
                    ref("node", value.get("node"), identifier)
                else:
                    field = "site" if role == "far_override_site" else "guard"
                    ref("site" if field == "site" else "guard", far.get(field), identifier)
        elif kind == "guard_site":
            ref("guard", binding.get("guard"), identifier)
            ref("site", binding.get("site"), identifier)
            guard = records["guard"][binding["guard"]]
            test = _validate_site_test(binding.get("test"), identifier, ref)
            if (guard.get("kind") != "site" or guard.get("site") != binding["site"]
                    or guard.get("view") != binding.get("view") or guard.get("view") not in ("live", "captured")
                    or guard.get("test") != test):
                raise GraphFormatError(f"{identifier} does not match its guard site atom")
        elif kind == "guard_state":
            ref("guard", binding.get("guard"), identifier)
            guard = records["guard"][binding["guard"]]
            test = _validate_state_test(binding.get("test"), identifier, ref)
            if guard.get("kind") != "state" or guard.get("test") != test:
                raise GraphFormatError(f"{identifier} does not match its guard state atom")
        elif kind == "contact_alternative":
            ref("contact", binding.get("contact"), identifier)
            ordinal = binding.get("ordinal")
            variants = records["contact"][binding["contact"]]["variants"]
            if type(ordinal) is not int or not 0 <= ordinal < len(variants):
                raise GraphFormatError(f"{identifier} has an invalid contact alternative")
        elif kind == "observer_watch":
            ref("node", binding.get("observer"), identifier)
            ref("site", binding.get("site"), identifier)
            observer = records["node"][binding["observer"]]
            if observer["kind"].get("kind") != "observer" or observer["kind"].get("watched") != binding["site"]:
                raise GraphFormatError(f"{identifier} does not match its observer watch")
        elif kind in ("captured_wire", "wire_neighborhood"):
            ref("node", binding.get("wire"), identifier)
            ref("site", binding.get("neighbor"), identifier)
            wire_node = records["node"][binding["wire"]]
            if wire_node["kind"].get("kind") != "wire":
                raise GraphFormatError(f"{identifier} references a non-wire node")
            plan_id = wire_node["kind"]["plan"]
            if binding["neighbor"] not in records["wire"][plan_id]["canonical_neighbors"]:
                raise GraphFormatError(f"{identifier} neighbor is not canonical for its wire")
            if kind == "captured_wire":
                route, _, delivery = _route_occurrence(binding.get("occurrence"), identifier, records, ref)
                if (route != records["wire"][plan_id].get("turbo_route")
                        or delivery.get("view") != "captured" or delivery.get("target") != binding["neighbor"]):
                    raise GraphFormatError(f"{identifier} does not match its captured wire occurrence")
        elif kind == "fixed_read":
            ref("node", binding.get("node"), identifier)
            role = binding.get("role")
            if role not in ("note_support", "note_obstruction"):
                raise GraphFormatError(f"{identifier} has an invalid fixed-read role")
            ref("site", binding.get("site"), identifier)
            node = records["node"][binding["node"]]
            component_type = node["kind"].get("component", {}).get("type")
            if component_type is not None and component_type != "note_block":
                raise GraphFormatError(f"{identifier} fixed-read role does not match its node kind")
        else:
            raise GraphFormatError(f"{identifier} has an unknown boundary binding kind {kind!r}")
        for reference in _typed_refs(binding):
            match = ID_PATTERN.fullmatch(reference)
            if match is None or reference not in records.get(match.group(1), {}):
                raise GraphFormatError(f"{identifier} references missing typed ID {reference!r}")

    _validate_instant_layers(graph, records)
    _validate_instant_machine(graph.get("instant_machine"), records)
    _validate_instant_kernel(graph.get("instant_kernel"), records)
    return records


def _validate_instant_layers(graph: dict[str, Any], records: dict[str, dict[str, dict[str, Any]]]) -> None:
    """Validate the cold causal DAG with indices scoped to each layer region."""
    regions = records["instant_layer_region"]
    declines = records["instant_layer_decline"]
    if not regions and not declines:
        return
    if records["instant_region"] or records["instant_proxy"]:
        raise GraphFormatError("instant layer tables cannot coexist with legacy instant regions or proxies")
    cells = records["instant_cell"]
    corridors = records["instant_corridor"]
    ports = records["instant_port"]
    if not cells and not corridors:
        raise GraphFormatError("instant layer tables require the boundary catalog")

    def ref(prefix: str, identifier: Any, owner: str) -> dict[str, Any]:
        if not isinstance(identifier, str) or identifier not in records[prefix]:
            raise GraphFormatError(f"{owner} references missing {prefix} ID {identifier!r}")
        return records[prefix][identifier]

    def local_index(value: Any, length: int, owner: str) -> int:
        if type(value) is not int or not 0 <= value < length or value > 0xFFFFFFFF:
            raise GraphFormatError(f"{owner} references an invalid local index")
        return value

    def expression_root(value: Any, length: int, owner: str) -> int:
        return local_index(value, length, owner)

    def occurrence(value: Any, owner: str) -> tuple[str, int]:
        occurrence = _object(value, f"{owner}.occurrence")
        route = occurrence.get("route")
        ref("route", route, owner)
        ordinal = occurrence.get("ordinal")
        deliveries = records["route"][route]["deliveries"]
        if type(ordinal) is not int or not 0 <= ordinal < len(deliveries):
            raise GraphFormatError(f"{owner} has an invalid route occurrence")
        return route, ordinal

    owned_cells: set[str] = set()
    owned_corridors: set[str] = set()
    for identifier, region in regions.items():
        region_cells: set[str] = set()
        region_corridors: set[str] = set()
        for field, prefix, owners, local in (
            ("cells", "instant_cell", owned_cells, region_cells),
            ("corridors", "instant_corridor", owned_corridors, region_corridors),
        ):
            for member in _array(region.get(field), f"{identifier}.{field}"):
                ref(prefix, member, identifier)
                if member in local or member in owners:
                    raise GraphFormatError(f"{identifier} repeats or shares {field} ownership")
                local.add(member)
                owners.add(member)

        region_actors = {actor for cell in region_cells for actor in _array(cells[cell].get("actors"), f"{cell}.actors")}
        region_resources = {resource for cell in region_cells for resource in _array(cells[cell].get("resources"), f"{cell}.resources")}
        region_resets = {observer for cell in region_cells for observer in _array(cells[cell].get("resets"), f"{cell}.resets")}
        region_actor_nodes = {records["piston"][actor]["node"] for actor in region_actors}
        for item in _array(region.get("inputs"), f"{identifier}.inputs"):
            item = _object(item, f"{identifier}.input")
            if item.get("role") not in ("request_sample", "activation_event", "acceptance_revalidation"):
                raise GraphFormatError(f"{identifier} has an invalid input role")
            source = _object(item.get("source"), f"{identifier}.input.source")
            source_kind = source.get("kind")
            if source_kind in ("boundary_event", "power_contribution"):
                port = ref("instant_port", source.get("port"), identifier)
                if port.get("direction") != "input" or port.get("owner") not in region_cells | region_corridors:
                    raise GraphFormatError(f"{identifier} input port is outside its input boundary")
                if source_kind == "boundary_event":
                    occurrence(source.get("occurrence"), identifier)
                    if port.get("binding", {}).get("kind") != "callback" or port["binding"].get("occurrence") != source.get("occurrence"):
                        raise GraphFormatError(f"{identifier} event source does not match its callback port")
                else:
                    ref("power_link", source.get("link"), identifier)
                    binding = port.get("binding", {})
                    if binding.get("kind") != "power" or binding.get("link") != source.get("link"):
                        raise GraphFormatError(f"{identifier} power source does not match its power port")
            elif source_kind == "source_above":
                site = ref("site", source.get("site"), identifier)
                threshold = source.get("threshold")
                port = _object(source.get("port"), f"{identifier}.source.port")
                valid_port = port.get("kind") in ("output", "stored") or (
                    port.get("kind") in ("weak", "strong") and port.get("face") in
                    ("top", "bottom", "north", "south", "east", "west")
                )
                if type(threshold) is not int or not 0 <= threshold < 15 or not valid_port:
                    raise GraphFormatError(f"{identifier} has an invalid threshold source")
                owned_wires = {wire for corridor in region_corridors for wire in records["instant_corridor"][corridor]["wires"]}
                if site.get("source", {}).get("node") in owned_wires:
                    raise GraphFormatError(f"{identifier} exposes an owned internal wire as input")
            elif source_kind == "state":
                test = _validate_state_test(source.get("test"), identifier, ref)
                if test["kind"].startswith("piston_"):
                    if test["piston"] not in region_actors:
                        raise GraphFormatError(f"{identifier} reads state for an actor outside its cells")
                elif test["resource"] not in region_resources:
                    raise GraphFormatError(f"{identifier} reads state for a resource outside its cells")
                elif test["kind"] == "resource_at" and test["contact"] not in records["resource"][test["resource"]]["contacts"]:
                    raise GraphFormatError(f"{identifier} reads a resource at a contact it does not own")
            elif source_kind == "local_reset_output":
                observer_id = source.get("observer")
                observer = ref("node", observer_id, identifier)
                if observer_id not in region_resets or observer["kind"].get("kind") != "observer":
                    raise GraphFormatError(f"{identifier} reset source is not an owned observer")
                link = ref("power_link", source.get("link"), identifier)
                source_site = ref("site", link["from"].get("site"), identifier)
                source_port = link["from"].get("port", {})
                target = link.get("to", {})
                guard = ref("guard", link.get("guard"), identifier)
                site_source = source_site.get("source", {})
                valid_port = source_port.get("kind") == "output" or (
                    source_port.get("kind") == "strong" and source_port.get("face") == "bottom"
                )
                target_is_quasi = (target.get("kind") == "input" and target.get("port") == "quasi"
                                   and target.get("node") in region_actor_nodes)
                target_is_static_conductor = False
                if target.get("kind") == "conductor":
                    target_site = ref("site", target.get("site"), identifier)
                    target_is_static_conductor = (source_port == {"kind": "strong", "face": "bottom"}
                                                  and link.get("face") == "bottom"
                                                  and target_site.get("source", {}).get("kind") == "constant")
                if (site_source.get("kind") != "component" or site_source.get("node") != observer_id
                        or not valid_port or link.get("attenuation") != 0 or guard.get("kind") != "always"
                        or not (target_is_quasi or target_is_static_conductor)):
                    raise GraphFormatError(f"{identifier} reset source does not match its original quasi link")
            else:
                raise GraphFormatError(f"{identifier} has an invalid input source")
        for item in _array(region.get("outputs"), f"{identifier}.outputs"):
            item = _object(item, f"{identifier}.output")
            port = ref("instant_port", item.get("port"), identifier)
            if port.get("direction") != "output" or port.get("owner") not in region_cells | region_corridors:
                raise GraphFormatError(f"{identifier} output port is outside its output boundary")
            local_index(item.get("emitted_when"), len(_array(region.get("expressions"), f"{identifier}.expressions")), identifier)

        resource_outputs = _array(region.get("resource_outputs"), f"{identifier}.resource_outputs")
        if len(resource_outputs) != 1:
            raise GraphFormatError(f"{identifier} must expose one resource output")
        for item in resource_outputs:
            item = _object(item, f"{identifier}.resource_output")
            cell_id = item.get("cell")
            ref("instant_cell", cell_id, identifier)
            resource_id = item.get("resource")
            ref("resource", resource_id, identifier)
            if cell_id not in region_cells or resource_id not in cells[cell_id]["resources"]:
                raise GraphFormatError(f"{identifier} resource output is outside its cell")
            resource_contacts = records["resource"][resource_id]["contacts"]
            for field in ("source", "destination"):
                contact_id = item.get(field)
                ref("contact", contact_id, identifier)
                if contact_id not in resource_contacts:
                    raise GraphFormatError(f"{identifier} resource output contact is outside its resource")
            if item["source"] == item["destination"]:
                raise GraphFormatError(f"{identifier} resource output must move between distinct contacts")

        expressions = _array(region.get("expressions"), f"{identifier}.expressions")
        for index, raw_expression in enumerate(expressions):
            expression = _object(raw_expression, f"{identifier}.expressions[{index}]")
            kind = expression.get("kind")
            if kind == "constant":
                if type(expression.get("value")) is not bool:
                    raise GraphFormatError(f"{identifier} has an invalid Boolean constant")
            elif kind == "input":
                local_index(expression.get("input"), len(_array(region.get("inputs"), f"{identifier}.inputs")), identifier)
            elif kind == "not":
                local_index(expression.get("value"), index, identifier)
            elif kind in ("all", "any"):
                for operand in _array(expression.get("values"), f"{identifier}.{kind}.values"):
                    local_index(operand, index, identifier)
            else:
                raise GraphFormatError(f"{identifier} has an unknown layer expression {kind!r}")

        expression_root(region.get("stationary_entry"), len(expressions), identifier)
        for item in resource_outputs:
            expression_root(item.get("transfer_when"), len(expressions), identifier)
            expression_root(item.get("source_absent_after"), len(expressions), identifier)

        for item in _array(region.get("cold_lifecycle_occurrences"), f"{identifier}.cold_lifecycle_occurrences"):
            item = _object(item, f"{identifier}.cold_lifecycle_occurrence")
            actor = item.get("actor")
            ref("piston", actor, identifier)
            if actor not in region_actors:
                raise GraphFormatError(f"{identifier} lifecycle actor is outside its cells")
            route_id, _ = occurrence(item.get("occurrence"), identifier)
            phase = item.get("phase")
            reason = _object(item.get("reason"), f"{identifier}.lifecycle_reason")
            reason_kind = reason.get("kind")
            if phase in ("retraction_base_changed", "retraction_far_changed", "retraction_near_changed"):
                valid_reason = reason_kind == "inert_cell_callback"
            elif phase == "reset_observer_lifecycle":
                valid_reason = reason_kind == "reset_observer_update"
            elif phase in ("reset_output", "reset_far_shape_change"):
                valid_reason = reason_kind == "ordinary_graph_lifecycle"
            elif phase in ("deferred_base_restoration", "deferred_head_restoration", "deferred_resource_restoration"):
                valid_reason = reason_kind == "ordinary_graph_lifecycle"
            else:
                valid_reason = False
            if not valid_reason:
                raise GraphFormatError(f"{identifier} has an invalid lifecycle phase and reason")
            if reason_kind == "reset_observer_update":
                observer_id = reason.get("observer")
                observer = ref("node", observer_id, identifier)
                if observer_id not in region_resets or observer["kind"].get("kind") != "observer":
                    raise GraphFormatError(f"{identifier} lifecycle observer is outside its resets")
            actor_routes = records["piston"][actor].get("routes") or {}
            reset_routes: list[str | None] = []
            if phase == "reset_observer_lifecycle" and reason_kind == "reset_observer_update":
                observer_row = records["node"][reason["observer"]]
                watched_site = records["site"].get(observer_row["kind"].get("watched"), {})
                watched_contact = watched_site.get("source", {}).get("contact")
                contact_row = records["contact"].get(watched_contact, {})
                watched_actors = {
                    receiver.get("piston")
                    for variant in contact_row.get("variants", [])
                    if (receiver := variant.get("receiver")) and receiver.get("kind") == "piston_base"
                }
                for watched_actor in watched_actors:
                    routes = records["piston"].get(watched_actor, {}).get("routes") or {}
                    reset_routes.extend((routes.get("base_changed"), routes.get("base_restored")))
            expected_routes = {
                "retraction_base_changed": [actor_routes.get("base_changed")],
                "retraction_far_changed": [actor_routes.get("far_changed")],
                "retraction_near_changed": [actor_routes.get("near_changed")],
                "reset_far_shape_change": [actor_routes.get("far_shape_changed")],
                "deferred_base_restoration": [actor_routes.get("base_restored")],
                "deferred_head_restoration": [actor_routes.get("head_restored")],
                "deferred_resource_restoration": [actor_routes.get("resource_restored_near"), actor_routes.get("resource_restored_far")],
                "reset_observer_lifecycle": reset_routes,
                "reset_output": [
                    observer_id for observer_id in region_resets
                    if records["node"][observer_id].get("output_route") == route_id
                    and records["route"][route_id].get("source") == observer_id
                    and records["node"][observer_id]["kind"].get("kind") == "observer"
                ],
            }
            if phase == "reset_output":
                if not expected_routes[phase]:
                    raise GraphFormatError(f"{identifier} reset output route is not owned by a recognized observer")
            elif phase == "reset_far_shape_change":
                if (route_id not in expected_routes[phase]
                        or records["route"][route_id].get("source") != records["piston"][actor].get("node")):
                    raise GraphFormatError(f"{identifier} far-shape route is not owned by its actor")
            elif route_id not in [route for route in expected_routes[phase] if route is not None]:
                raise GraphFormatError(f"{identifier} lifecycle occurrence does not match its declared route phase")

        for raw_pin in _array(region.get("pin_values"), f"{identifier}.pin_values"):
            pin = _object(raw_pin, f"{identifier}.pin_value")
            ref("instant_cell", pin.get("cell"), identifier)
            if pin["cell"] not in region_cells or type(pin.get("pin")) is not int or not 0 <= pin["pin"] <= 0xFFFFFFFF:
                raise GraphFormatError(f"{identifier} has a pin value outside its cell")
            expression_root(pin.get("value"), len(expressions), identifier)

        use_actors: dict[int, str] = {}
        seen_occurrences: set[tuple] = set()
        for layer in _array(region.get("layers"), f"{identifier}.layers"):
            layer = _object(layer, f"{identifier}.layer")
            if layer.get("order") != "fifo":
                raise GraphFormatError(f"{identifier} has an invalid layer order")
            for raw_use in _array(layer.get("uses"), f"{identifier}.uses"):
                use = _object(raw_use, f"{identifier}.use")
                use_id = local_index(use.get("use_id"), 0x100000000, identifier)
                actor = use.get("actor")
                ref("piston", actor, identifier)
                if actor not in region_actors:
                    raise GraphFormatError(f"{identifier} use actor is outside its region")
                if use_id in use_actors and use_actors[use_id] != actor:
                    raise GraphFormatError(f"{identifier} changes the actor for a use ID")
                use_actors[use_id] = actor
                for field in ("requested", "accepted", "consumed_before", "consumed_after"):
                    expression_root(use.get(field), len(expressions), identifier)
                cause = _object(use.get("cause"), f"{identifier}.cause")
                if cause.get("kind") == "boundary":
                    cause_occurrences = _array(cause.get("occurrences"), f"{identifier}.cause.occurrences")
                    if not cause_occurrences:
                        raise GraphFormatError(f"{identifier} has a boundary cause without occurrences")
                    cause_key = ("boundary", tuple(occurrence(item, identifier) for item in cause_occurrences))
                else:
                    raise GraphFormatError(f"{identifier} has an invalid use cause")
                occurrence_key = (use_id, actor, cause_key)
                if occurrence_key in seen_occurrences:
                    raise GraphFormatError(f"{identifier} repeats an exact use occurrence")
                seen_occurrences.add(occurrence_key)

    decline_reasons = {
        "signal_arrival_unproven", "callback_semantics_unsupported", "shared_owner_order_unknown",
        "boundary_effect_unsupported",
    }
    for identifier, decline in declines.items():
        declined_cells: set[str] = set()
        declined_corridors: set[str] = set()
        for field, prefix, owners, local in (
            ("cells", "instant_cell", owned_cells, declined_cells),
            ("corridors", "instant_corridor", owned_corridors, declined_corridors),
        ):
            for member in _array(decline.get(field), f"{identifier}.{field}"):
                ref(prefix, member, identifier)
                if member in local or member in owners:
                    raise GraphFormatError(f"{identifier} repeats or shares {field} ownership")
                local.add(member)
                owners.add(member)
        declined_actors = {actor for cell in declined_cells for actor in _array(cells[cell].get("actors"), f"{cell}.actors")}
        declined_wires = {wire for corridor in declined_corridors for wire in _array(corridors[corridor].get("wires"), f"{corridor}.wires")}
        reason = decline.get("reason")
        if reason not in decline_reasons:
            raise GraphFormatError(f"{identifier} has an invalid decline reason")
        witness = _object(decline.get("witness"), f"{identifier}.witness")
        kind = witness.get("kind")
        if kind == "actor":
            actor = witness.get("piston")
            ref("piston", actor, identifier)
            if actor not in declined_actors:
                raise GraphFormatError(f"{identifier} actor witness is outside its cells")
        elif kind == "callback":
            occurrence(witness.get("occurrence"), identifier)
        elif kind == "power":
            ref("power_link", witness.get("link"), identifier)
        elif kind == "power_at_cut":
            actor = witness.get("actor")
            ref("piston", actor, identifier)
            ref("power_link", witness.get("link"), identifier)
            if actor not in declined_actors or witness.get("phase") not in (
                "request_sample", "activation_event", "acceptance_revalidation",
            ):
                raise GraphFormatError(f"{identifier} has an invalid power-at-cut witness")
        elif kind == "wire":
            node_id = witness.get("node")
            node = ref("node", node_id, identifier)
            if node["kind"].get("kind") != "wire" or node_id not in declined_wires:
                raise GraphFormatError(f"{identifier} wire witness is outside its corridors")
        else:
            raise GraphFormatError(f"{identifier} has an invalid witness kind")


def _validate_override(value: Any, owner: str, ref: Any) -> None:
    if value is None:
        return
    override = _object(value, f"{owner}.override")
    if override.get("kind") == "literal":
        strength = override.get("value")
        if not isinstance(strength, int) or not 0 <= strength <= 15:
            raise GraphFormatError(f"{owner} has invalid literal comparator strength")
    elif override.get("kind") == "live_component_output":
        ref("node", override.get("node"), owner)
    else:
        raise GraphFormatError(f"{owner} has unknown comparator override {override.get('kind')!r}")


def _validate_instant_machine(value: Any, records: dict) -> None:
    """Inspect the bounded exact program; revisions are decimal strings to avoid JS rounding."""
    if value is None:
        return
    machine = _object(value, "instant_machine")
    if machine.get("encoding") != "closed-instant-machine-v1":
        raise GraphFormatError("unknown instant machine encoding")
    controls = _array(machine.get("controls"), "instant machine controls")
    states = _array(machine.get("states"), "instant machine states")
    rows = _array(machine.get("transitions"), "instant machine transitions")
    if not 1 <= len(controls) <= 4 or not 1 <= len(states) <= 2048 or len(rows) != len(states):
        raise GraphFormatError("invalid instant machine dimensions")
    owners = _object(machine.get("owners"), "instant machine owners")
    for field, prefix in (("components", "node"), ("pistons", "piston"), ("resources", "resource")):
        ids = _array(owners.get(field), f"instant machine {field} owners")
        if len(ids) != len(records[prefix]) or set(ids) != set(records[prefix]):
            raise GraphFormatError("invalid instant machine owners")
        for state in states:
            if len(_array(_object(state, "instant state").get(field), f"instant state {field}")) != len(ids):
                raise GraphFormatError("invalid instant machine state dimensions")
    for row in rows:
        row = _array(row, "instant machine transition row")
        if len(row) != len(controls) + 1:
            raise GraphFormatError("invalid instant machine action count")
        for edge in row:
            if edge is not None:
                target = _object(edge, "instant transition").get("next")
                if type(target) is not int or not 0 <= target < len(states):
                    raise GraphFormatError("invalid instant machine target")
    def references(value: Any) -> None:
        if isinstance(value, str) and ":" in value:
            match = ID_PATTERN.fullmatch(value)
            if match is None or value not in records.get(match.group(1), {}):
                raise GraphFormatError("invalid instant machine reference")
        elif isinstance(value, list):
            for child in value:
                references(child)
        elif isinstance(value, dict):
            for child in value.values():
                references(child)
    references(machine)


def _validate_instant_kernel(value: Any, records: dict) -> None:
    """Validate the compact runtime DAG; u64 constants stay decimal strings."""
    if value is None:
        return
    kernel = _object(value, "instant_kernel")
    if kernel.get("encoding") != "compact-instant-kernel-v1":
        raise GraphFormatError("unknown compact instant kernel encoding")
    node_ids = set(records["node"])
    piston_ids = set(records["piston"])
    resource_ids = set(records["resource"])
    trigger = kernel.get("trigger")
    controls = _array(kernel.get("data_controls"), "kernel data controls")
    if not isinstance(trigger, str) or trigger not in node_ids or len(controls) > 3:
        raise GraphFormatError("invalid compact kernel controls")
    if any(not isinstance(control, str) or control not in node_ids for control in controls):
        raise GraphFormatError("compact kernel control references a missing node")
    if trigger in controls or len(set(controls)) != len(controls):
        raise GraphFormatError("invalid compact kernel controls")

    expressions = _array(kernel.get("expressions"), "kernel expressions")
    if not expressions or len(expressions) > 1_000_000:
        raise GraphFormatError("invalid compact kernel expression count")
    for index, raw_expression in enumerate(expressions):
        expression = _object(raw_expression, f"kernel expressions[{index}]")
        if expression.get("kind") == "constant":
            scalar = expression.get("value")
            if not isinstance(scalar, str) or len(scalar) > 20 or not re.fullmatch(r"0|[1-9][0-9]*", scalar):
                raise GraphFormatError("kernel scalar constants must be decimal strings")
            if int(scalar) > (1 << 64) - 1:
                raise GraphFormatError("kernel scalar constant exceeds u64")
        elif expression.get("kind") == "select":
            control, low, high = expression.get("control"), expression.get("low"), expression.get("high")
            if (type(control) is not int or not 0 <= control < len(controls)
                    or type(low) is not int or not 0 <= low < index
                    or type(high) is not int or not 0 <= high < index):
                raise GraphFormatError("invalid compact kernel DAG edge")
        else:
            raise GraphFormatError("unknown compact kernel expression")

    prefix, period = kernel.get("prefix"), kernel.get("period")
    phases = _array(kernel.get("phases"), "kernel phases")
    if (type(prefix) is not int or type(period) is not int or period <= 0
            or prefix < 0 or prefix + period != len(phases) or len(phases) > 128):
        raise GraphFormatError("invalid compact kernel phase dimensions")
    owners = _object(kernel.get("owners"), "kernel owners")
    for field, prefix_name in (("components", "node"), ("pistons", "piston"),
                               ("resources", "resource"), ("contacts", "contact")):
        values = _array(owners.get(field), f"kernel {field} owners")
        expected = list(records[prefix_name])
        if (any(not isinstance(owner, str) for owner in values)
                or len(values) != len(expected) or set(values) != set(expected)):
            raise GraphFormatError("invalid compact kernel owners")
    runtime_bytes = kernel.get("estimated_runtime_bytes")
    if type(runtime_bytes) is not int or runtime_bytes < 0:
        raise GraphFormatError("invalid compact kernel size estimate")

    def root(item: Any, where: str) -> None:
        if type(item) is not int or not 0 <= item < len(expressions):
            raise GraphFormatError(f"{where} references an invalid expression")

    def state_roots(raw_state: Any, where: str) -> None:
        state = _object(raw_state, where)
        for field, prefix_name in (("components", "node"), ("pistons", "piston"), ("resources", "resource")):
            values = _array(state.get(field), f"{where}.{field}")
            if len(values) != len(records[prefix_name]):
                raise GraphFormatError(f"{where}.{field} dimensions do not match owners")
            if field == "resources":
                for pair in values:
                    pair = _array(pair, f"{where}.resource roots")
                    if len(pair) != 2:
                        raise GraphFormatError("resource state roots must have two selectors")
                    for selector in pair:
                        root(selector, where)
            else:
                for selector in values:
                    root(selector, where)
        future = _object(state.get("future"), f"{where}.future")
        for field, maximum in (("ticks", 128), ("requests", 64), ("deferred", 128)):
            entries = _array(future.get(field), f"{where}.future.{field}")
            if len(entries) > maximum:
                raise GraphFormatError(f"{where}.future.{field} exceeds its bound")
        root(future.get("tick_len"), where)
        for index, tick in enumerate(future["ticks"]):
            tick = _object(tick, f"{where}.future.ticks[{index}]")
            for field in ("node", "delay", "priority"):
                root(tick.get(field), where)
        root(future.get("request_len"), where)
        for index, request in enumerate(future["requests"]):
            request = _object(request, f"{where}.future.requests[{index}]")
            for field in ("piston", "action"):
                root(request.get(field), where)
        root(future.get("deferred_len"), where)
        for index, event in enumerate(future["deferred"]):
            event = _object(event, f"{where}.future.deferred[{index}]")
            for field in ("piston", "revision", "resource", "resource_revision", "contact", "delay", "extending"):
                root(event.get(field), where)

    def ref(prefix_name: str, item: Any, where: str) -> None:
        if not isinstance(item, str) or item not in records[prefix_name]:
            raise GraphFormatError(f"{where} references an invalid {prefix_name} ID")

    def references(item: Any) -> None:
        if isinstance(item, str) and ":" in item:
            match = ID_PATTERN.fullmatch(item)
            if match is None or item not in records.get(match.group(1), {}):
                raise GraphFormatError("invalid compact kernel graph reference")
        elif isinstance(item, list):
            for child in item:
                references(child)
        elif isinstance(item, dict):
            for child in item.values():
                references(child)

    state_roots(kernel.get("entry"), "kernel entry")
    for phase_index, raw_phase in enumerate(phases):
        phase = _object(raw_phase, f"kernel phases[{phase_index}]")
        state_roots(phase.get("state"), f"kernel phases[{phase_index}].state")
        for field in ("scheduled", "accepted", "published"):
            root(phase.get(field), f"kernel phases[{phase_index}]")
        for item in _array(phase.get("components"), "kernel component deltas"):
            item = _object(item, "kernel component delta")
            ref("node", item.get("node"), "kernel component delta")
            root(item.get("changed"), "kernel component delta")
            root(item.get("value"), "kernel component delta")
        for item in _array(phase.get("dirty"), "kernel dirty roots"):
            item = _object(item, "kernel dirty root")
            ref("node", item.get("node"), "kernel dirty root")
            root(item.get("touched"), "kernel dirty root")
        for field, prefix_name in (("piston_revisions", "piston"), ("resource_revisions", "resource")):
            for item in _array(phase.get(field), f"kernel {field}"):
                item = _object(item, f"kernel {field} entry")
                ref(prefix_name, item.get(prefix_name), f"kernel {field} entry")
                root(item.get("increment"), f"kernel {field} entry")
        for item in _array(phase.get("effects"), "kernel effects"):
            item = _object(item, "kernel effect")
            ref("piston", item.get("piston"), "kernel effect")
            root(item.get("enabled"), "kernel effect")
            if item.get("action") not in ("extend", "retract", "retract_without_pull") or type(item.get("extended")) is not bool:
                raise GraphFormatError("invalid compact kernel effect")
    references(kernel)
