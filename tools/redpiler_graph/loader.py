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
    "wire_relation": "wire_relations",
    "comparator_plan": "comparator_plans",
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


def _validate(data: Any) -> dict[str, dict[str, dict[str, Any]]]:
    root = _object(data, "graph document")
    if root.get("format") != "mchprs.redpiler.graph":
        raise GraphFormatError("unsupported graph format")
    version = _object(root.get("version"), "version")
    if version.get("major") != 1:
        raise GraphFormatError(f"unsupported graph major version: {version.get('major')}")
    if not isinstance(version.get("minor"), int) or version["minor"] < 0:
        raise GraphFormatError("version.minor must be a nonnegative integer")
    if root.get("stage") not in ("electrical_draft", "program"):
        raise GraphFormatError(f"unsupported graph stage: {root.get('stage')}")

    graph = _object(root.get("graph"), "graph")
    records: dict[str, dict[str, dict[str, Any]]] = {}
    for prefix, table in TABLES.items():
        values = graph.get(table)
        if table == "routes" and values is None:
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
        if node_type == "observer":
            ref("site", kind.get("watched"), identifier)
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
            if test_kind in ("piston_extended", "piston_availability"):
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

    for identifier, wire in records["wire"].items():
        ref("node", wire.get("node"), identifier)
        neighbors = _array(wire.get("canonical_neighbors"), f"{identifier}.canonical_neighbors")
        if len(neighbors) != 24:
            raise GraphFormatError(f"{identifier} must have exactly 24 canonical neighbors")
        for site in neighbors:
            ref("site", site, identifier)
        optional("route", wire.get("shape_route"), identifier)
        optional("route", wire.get("turbo_route"), identifier)

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
        for delivery in _array(route.get("deliveries"), f"{identifier}.deliveries"):
            delivery = _object(delivery, f"{identifier}.delivery")
            ref("site", delivery.get("target"), identifier)
            ref("guard", delivery.get("guard"), identifier)

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

    return records


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
