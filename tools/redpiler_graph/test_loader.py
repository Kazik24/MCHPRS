"""Regression tests for graph export certificate loading."""

import json
import tempfile
import unittest
from pathlib import Path

from .loader import GraphFormatError, load_graph


def document() -> dict:
    graph = {
        table: []
        for table in (
            "nodes",
            "profiles",
            "sites",
            "guards",
            "contacts",
            "pistons",
            "resources",
            "power_links",
            "wire_plans",
            "dust_paths",
            "wire_relations",
            "comparator_plans",
            "callback_links",
            "routes",
        )
    }
    graph["nodes"] = [
        {"id": "node:0", "kind": {"kind": "electrical"}, "output_route": None},
        {"id": "node:1", "kind": {"kind": "wire", "plan": "wire:0"}, "output_route": None},
        {"id": "node:2", "kind": {"kind": "wire", "plan": "wire:1"}, "output_route": None},
    ]
    graph["sites"] = [
        {"id": "site:0", "position": {"x": 0, "y": 0, "z": 0}, "source": {"kind": "component", "node": "node:0"}}
    ]
    graph["wire_plans"] = [
        {
            "id": f"wire:{index}",
            "node": f"node:{index + 1}",
            "canonical_neighbors": ["site:0"] * 24,
            "shape_route": None,
            "turbo_route": None,
        }
        for index in range(2)
    ]
    graph["dust_paths"] = [
        {
            "id": "dust_path:0",
            "input_node": "node:0",
            "input_site": "site:0",
            "input_face": "east",
            "wires": ["node:1", "node:2"],
        }
    ]
    return {
        "format": "mchprs.redpiler.graph",
        "version": {"major": 1, "minor": 1},
        "stage": "callback_draft",
        "graph": graph,
        "snapshots": [],
        "trace": None,
    }


def compact_kernel(graph: dict) -> dict:
    def state() -> dict:
        return {
            "components": [0] * len(graph["nodes"]),
            "pistons": [0] * len(graph["pistons"]),
            "resources": [[0, 0] for _ in graph["resources"]],
            "future": {"tick_len": 0, "ticks": [], "request_len": 0, "requests": [],
                       "deferred_len": 0, "deferred": []},
        }

    return {
        "encoding": "compact-instant-kernel-v1",
        "trigger": graph["nodes"][0]["id"],
        "data_controls": [graph["nodes"][1]["id"]],
        "owners": {
            "components": [row["id"] for row in graph["nodes"]],
            "pistons": [row["id"] for row in graph["pistons"]],
            "resources": [row["id"] for row in graph["resources"]],
            "contacts": [row["id"] for row in graph["contacts"]],
        },
        "prefix": 0,
        "period": 1,
        "estimated_runtime_bytes": 8,
        "expressions": [{"kind": "constant", "value": "0"}],
        "entry": state(),
        "phases": [{"components": [], "dirty": [], "piston_revisions": [], "resource_revisions": [],
                    "effects": [], "scheduled": 0, "accepted": 0, "published": 0, "state": state()}],
    }


class DustPathLoaderTests(unittest.TestCase):
    def instant_boundary_document(self):
        fixture = Path(__file__).resolve().parents[2] / "test_data/graph-fixtures/adder-1bit/adder-1bit.graph.json"
        data = json.loads(fixture.read_text(encoding="utf-8"))
        graph = data["graph"]
        observer = next(node for node in graph["nodes"] if node["kind"]["kind"] == "observer")
        graph["instant_regions"] = []
        graph["instant_proxies"] = []
        graph["instant_cells"] = [{
            "id": "instant_cell:0", "actors": [graph["pistons"][0]["id"]],
            "resources": [graph["resources"][0]["id"]], "resets": [observer["id"]],
        }]
        graph["instant_corridors"] = [{"id": "instant_corridor:0", "wires": [], "conductive_sites": []}]
        graph["instant_boundary_ports"] = []
        graph["instant_declines"] = []
        return data

    def test_instant_boundary_tables_validate_all_bindings_and_keep_occurrences_separate(self):
        data = self.instant_boundary_document()
        graph = data["graph"]
        route = next(row for row in graph["routes"] if row["deliveries"])
        captured = next(
            (row, ordinal, delivery)
            for row in graph["routes"]
            if row.get("source") in {node["id"] for node in graph["nodes"] if node["kind"]["kind"] == "wire"}
            and row["id"] == next(plan for plan in graph["wire_plans"]
                                  if plan["node"] == row["source"]).get("turbo_route")
            for ordinal, delivery in enumerate(row["deliveries"])
            if delivery.get("view") == "captured"
            and delivery["target"] in next(plan for plan in graph["wire_plans"]
                                            if plan["node"] == row["source"])["canonical_neighbors"]
        )
        site_guard = next(guard for guard in graph["guards"] if guard["kind"] == "site")
        state_guard = next(guard for guard in graph["guards"] if guard["kind"] == "state")
        contact = next(contact for contact in graph["contacts"] if contact["variants"])
        observer = next(node for node in graph["nodes"] if node["kind"]["kind"] == "observer")
        wire_node = captured[0]["source"]
        wire_plan = next(plan for plan in graph["wire_plans"] if plan["node"] == wire_node)
        comparator_id = f"comparator_plan:{len(graph['comparator_plans'])}"
        graph["comparator_plans"].append({
            "id": comparator_id, "node": graph["nodes"][0]["id"], "rear": graph["sites"][0]["id"],
            "direct_override": {"kind": "literal", "value": 1},
            "far_override": {"site": graph["sites"][0]["id"], "guard": site_guard["id"],
                             "value": {"kind": "live_component_output", "node": graph["nodes"][0]["id"]}},
        })
        bindings = [
            {"kind": "power", "link": graph["power_links"][0]["id"]},
            {"kind": "callback", "occurrence": {"route": route["id"], "ordinal": 0}},
            {"kind": "wire_relation", "relation": graph["wire_relations"][0]["id"]},
            *({"kind": "comparator", "plan": comparator_id, "role": role}
              for role in ("rear", "direct_override", "far_override_site", "far_override_guard", "far_override_output")),
            {"kind": "guard_site", "guard": site_guard["id"], "site": site_guard["site"],
             "view": site_guard["view"], "test": site_guard["test"]},
            {"kind": "guard_state", "guard": state_guard["id"], "test": state_guard["test"]},
            {"kind": "contact_alternative", "contact": contact["id"], "ordinal": 0},
            {"kind": "observer_watch", "observer": observer["id"], "site": observer["kind"]["watched"]},
            {"kind": "captured_wire", "wire": wire_node,
             "occurrence": {"route": captured[0]["id"], "ordinal": captured[1]}, "neighbor": captured[2]["target"]},
            {"kind": "wire_neighborhood", "wire": wire_node, "neighbor": wire_plan["canonical_neighbors"][0]},
            {"kind": "fixed_read", "node": observer["id"], "site": graph["sites"][0]["id"], "role": "note_support"},
            {"kind": "fixed_read", "node": observer["id"], "site": graph["sites"][0]["id"], "role": "note_obstruction"},
        ]
        for binding in bindings:
            graph["instant_boundary_ports"].append({
                "id": f"instant_port:{len(graph['instant_boundary_ports'])}", "owner": "instant_cell:0",
                "direction": "input", "binding": binding,
            })
        duplicate = json.loads(json.dumps(graph["instant_boundary_ports"][1]))
        duplicate["id"] = f"instant_port:{len(graph['instant_boundary_ports'])}"
        graph["instant_boundary_ports"].append(duplicate)

        loaded = self.load_data(data)
        self.assertEqual(len(loaded.records["instant_port"]), len(bindings) + 1)
        self.assertIn("instant_cell:0", loaded.records["instant_cell"])
        self.assertEqual(loaded.record(duplicate["id"])["binding"], duplicate["binding"])

    def test_instant_boundary_tables_reject_bad_ownership_dangling_refs_and_atoms(self):
        mutations = (
            lambda graph: graph["instant_cells"].append({**graph["instant_cells"][0], "id": "instant_cell:1"}),
            lambda graph: graph["instant_cells"][0].update(actors=["node:0"]),
            lambda graph: graph["instant_cells"][0].update(resets=["node:0"]),
            lambda graph: graph["instant_corridors"][0].update(wires=["node:0"]),
            lambda graph: graph["instant_corridors"][0].update(wires=["node:999"]),
            lambda graph: graph["instant_boundary_ports"].append({
                "id": "instant_port:0", "owner": "instant_cell:999", "direction": "input",
                "binding": {"kind": "power", "link": "power_link:0"},
            }),
            lambda graph: graph["instant_boundary_ports"].append({
                "id": "instant_port:0", "owner": "instant_cell:0", "direction": "sideways",
                "binding": {"kind": "power", "link": "power_link:0"},
            }),
            lambda graph: graph["instant_boundary_ports"].append({
                "id": "instant_port:0", "owner": "instant_cell:0", "direction": "input",
                "binding": {"kind": "power", "link": "node:0"},
            }),
            lambda graph: graph["instant_declines"].append({
                "id": "instant_decline:0", "actors": graph["instant_cells"][0]["actors"],
                "reason": {"code": "unsupported_case"},
            }),
        )
        for mutation in mutations:
            data = self.instant_boundary_document()
            mutation(data["graph"])
            with self.subTest(mutation=mutation), self.assertRaises(GraphFormatError):
                self.load_data(data)

        data = self.instant_boundary_document()
        graph = data["graph"]
        guard = next(row for row in graph["guards"] if row["kind"] == "site")
        other_site = next(site["id"] for site in graph["sites"] if site["id"] != guard["site"])
        graph["instant_boundary_ports"].append({
            "id": "instant_port:0", "owner": "instant_cell:0", "direction": "input",
            "binding": {"kind": "guard_site", "guard": guard["id"], "site": other_site,
                        "view": guard["view"], "test": guard["test"]},
        })
        with self.assertRaisesRegex(GraphFormatError, "guard site atom"):
            self.load_data(data)

        data = self.instant_boundary_document()
        graph = data["graph"]
        site_guard = next(row for row in graph["guards"] if row["kind"] == "site")
        plan_id = "comparator_plan:0"
        graph["comparator_plans"].append({
            "id": plan_id, "node": graph["nodes"][0]["id"], "rear": graph["sites"][0]["id"],
            "direct_override": None,
            "far_override": {"site": graph["sites"][0]["id"], "guard": site_guard["id"],
                             "value": {"kind": "literal", "value": 1}},
        })
        graph["instant_boundary_ports"].append({
            "id": "instant_port:0", "owner": "instant_cell:0", "direction": "input",
            "binding": {"kind": "comparator", "plan": plan_id, "role": "far_override_output"},
        })
        with self.assertRaisesRegex(GraphFormatError, "far override output"):
            self.load_data(data)

    def test_instant_boundary_corridors_reject_shared_and_classic_owned_wires(self):
        data = self.instant_boundary_document()
        graph = data["graph"]
        wire_nodes = [node["id"] for node in graph["nodes"] if node["kind"]["kind"] == "wire"]
        corridor = graph["instant_corridors"][0]
        corridor["wires"] = [wire_nodes[0]]
        corridor["conductive_sites"] = [graph["sites"][0]["id"]]
        graph["instant_corridors"].append({
            "id": "instant_corridor:1", "wires": [wire_nodes[0]], "conductive_sites": [],
        })
        with self.assertRaisesRegex(GraphFormatError, "shares wires ownership"):
            self.load_data(data)

        data = self.instant_boundary_document()
        graph = data["graph"]
        site = graph["sites"][0]["id"]
        graph["instant_corridors"][0]["conductive_sites"] = [site]
        graph["instant_corridors"].append({
            "id": "instant_corridor:1", "wires": [], "conductive_sites": [site],
        })
        with self.assertRaisesRegex(GraphFormatError, "shares conductive_sites ownership"):
            self.load_data(data)

        data = self.instant_boundary_document()
        graph = data["graph"]
        wire_nodes = [node["id"] for node in graph["nodes"] if node["kind"]["kind"] == "wire"]
        graph["classic_nets"] = [{"id": "classic_net:0", "source": graph["nodes"][0]["id"], "initial_signal": 0,
                                  "wires": [{"node": wire_nodes[0], "attenuation": 0}], "inputs": [], "entries": []}]
        graph["instant_corridors"][0]["wires"] = [wire_nodes[0]]
        with self.assertRaisesRegex(GraphFormatError, "classic-owned wires"):
            self.load_data(data)

    def test_instant_boundary_tables_are_optional_for_older_exports(self):
        data = self.instant_boundary_document()
        for table in ("instant_cells", "instant_corridors", "instant_boundary_ports", "instant_declines"):
            del data["graph"][table]
        loaded = self.load_data(data)
        for prefix in ("instant_cell", "instant_corridor", "instant_port", "instant_decline"):
            self.assertEqual(loaded.records[prefix], {})

    def instant_layer_document(self):
        data = self.instant_boundary_document()
        graph = data["graph"]
        cell = graph["instant_cells"][0]
        cell["actors"].append(graph["pistons"][1]["id"])
        route = next(route for route in graph["routes"] if len(route["deliveries"]) >= 3)
        occurrences = [{"route": route["id"], "ordinal": index} for index in range(3)]
        observer = cell["resets"][0]
        actor_routes = graph["pistons"][0]["routes"]
        lifecycle_occurrence = lambda route_id: {"route": route_id, "ordinal": 0}
        watched_site = next(site for site in graph["sites"] if site["id"] == next(
            node["kind"]["watched"] for node in graph["nodes"] if node["id"] == observer))
        watched_contact = next(contact for contact in graph["contacts"] if contact["id"] == watched_site["source"]["contact"])
        watched_piston_id = next(variant["receiver"]["piston"] for variant in watched_contact["variants"]
                                 if variant.get("receiver", {}).get("kind") == "piston_base")
        watched_routes = next(piston["routes"] for piston in graph["pistons"] if piston["id"] == watched_piston_id)
        observer_site = next(site["id"] for site in graph["sites"]
                             if site["source"] == {"kind": "component", "node": observer})
        reset_link = next(link for link in graph["power_links"]
                          if link["from"]["site"] == observer_site
                          and (link["from"]["port"]["kind"] == "output"
                               or link["from"]["port"] == {"kind": "strong", "face": "bottom"})
                          and link["guard"] == "guard:0" and link["attenuation"] == 0)
        graph["instant_boundary_ports"] = [
            {"id": "instant_port:0", "owner": cell["id"], "direction": "input",
             "binding": {"kind": "power", "link": graph["power_links"][0]["id"]}},
            {"id": "instant_port:1", "owner": cell["id"], "direction": "input",
             "binding": {"kind": "callback", "occurrence": occurrences[0]}},
            {"id": "instant_port:2", "owner": cell["id"], "direction": "output",
             "binding": {"kind": "callback", "occurrence": occurrences[1]}},
        ]
        graph["instant_layer_regions"] = [{
            "id": "instant_layer_region:7", "stationary_entry": 6,
            "cells": [cell["id"]], "corridors": ["instant_corridor:0"],
            "inputs": [
                {"role": "activation_event", "source": {"kind": "boundary_event", "port": "instant_port:1", "occurrence": occurrences[0]}},
                {"role": "request_sample", "source": {"kind": "power_contribution", "port": "instant_port:0", "link": graph["power_links"][0]["id"]}},
                {"role": "acceptance_revalidation", "source": {"kind": "state", "test": {"kind": "piston_extended", "piston": graph["pistons"][0]["id"]}}},
                {"role": "request_sample", "source": {"kind": "local_reset_output", "observer": observer, "link": reset_link["id"]}},
                {"role": "acceptance_revalidation", "source": {"kind": "state", "test": {"kind": "resource_enabled", "resource": cell["resources"][0]}}},
            ],
            "outputs": [{"port": "instant_port:2", "emitted_when": 6}],
            "cold_lifecycle_occurrences": [
                {"actor": graph["pistons"][0]["id"], "phase": "retraction_base_changed",
                 "occurrence": lifecycle_occurrence(actor_routes["base_changed"]), "reason": {"kind": "inert_cell_callback"}},
                {"actor": graph["pistons"][0]["id"], "phase": "retraction_far_changed",
                 "occurrence": lifecycle_occurrence(actor_routes["far_changed"]),
                 "reason": {"kind": "inert_cell_callback"}},
                {"actor": graph["pistons"][0]["id"], "phase": "retraction_near_changed",
                 "occurrence": lifecycle_occurrence(actor_routes["near_changed"]),
                 "reason": {"kind": "inert_cell_callback"}},
                {"actor": graph["pistons"][0]["id"], "phase": "deferred_base_restoration",
                 "occurrence": lifecycle_occurrence(actor_routes["base_restored"]), "reason": {"kind": "ordinary_graph_lifecycle"}},
                {"actor": graph["pistons"][0]["id"], "phase": "deferred_head_restoration",
                 "occurrence": lifecycle_occurrence(actor_routes["head_restored"]), "reason": {"kind": "ordinary_graph_lifecycle"}},
                {"actor": graph["pistons"][0]["id"], "phase": "deferred_resource_restoration",
                 "occurrence": lifecycle_occurrence(actor_routes["resource_restored_near"]), "reason": {"kind": "ordinary_graph_lifecycle"}},
                {"actor": graph["pistons"][0]["id"], "phase": "reset_observer_lifecycle",
                 "occurrence": lifecycle_occurrence(watched_routes["base_changed"]),
                 "reason": {"kind": "reset_observer_update", "observer": observer}},
                {"actor": graph["pistons"][0]["id"], "phase": "reset_output",
                 "occurrence": lifecycle_occurrence(next(node["output_route"] for node in graph["nodes"] if node["id"] == observer)),
                 "reason": {"kind": "ordinary_graph_lifecycle"}},
                {"actor": graph["pistons"][0]["id"], "phase": "reset_far_shape_change",
                 "occurrence": lifecycle_occurrence(actor_routes["far_shape_changed"]),
                 "reason": {"kind": "ordinary_graph_lifecycle"}},
            ],
            "resource_outputs": [{"cell": cell["id"], "resource": cell["resources"][0],
                                  "source": graph["resources"][0]["contacts"][0],
                                  "destination": graph["resources"][0]["contacts"][1],
                                  "transfer_when": 7, "source_absent_after": 8}],
            "expressions": [
                {"kind": "constant", "value": True}, {"kind": "input", "input": 0},
                {"kind": "input", "input": 1}, {"kind": "input", "input": 2},
                {"kind": "not", "value": 0}, {"kind": "all", "values": [1, 4]},
                {"kind": "any", "values": [2, 3]}, {"kind": "input", "input": 3},
                {"kind": "input", "input": 4},
            ],
            "pin_values": [{"cell": cell["id"], "pin": 0, "value": 5}],
            "layers": [{"order": "fifo", "uses": [
                {"use_id": 0, "actor": graph["pistons"][0]["id"], "requested": 1, "accepted": 2,
                 "consumed_before": 0, "consumed_after": 5,
                 "cause": {"kind": "boundary", "occurrences": [occurrences[0], occurrences[1]]}},
                {"use_id": 1, "actor": graph["pistons"][1]["id"], "requested": 2, "accepted": 3,
                 "consumed_before": 0, "consumed_after": 6,
                 "cause": {"kind": "boundary", "occurrences": [occurrences[0], occurrences[1]]}},
            ]}],
        }]
        graph["instant_layer_declines"] = []
        return data

    def test_instant_boundary_state_requires_its_matching_input_guard(self):
        data = self.instant_layer_document()
        graph = data["graph"]
        region = graph["instant_layer_regions"][0]
        guard = next(guard for guard in graph["guards"] if guard["kind"] == "state")
        graph["instant_boundary_ports"].append({
            "id": "instant_port:900", "owner": region["cells"][0], "direction": "input",
            "binding": {"kind": "guard_state", "guard": guard["id"], "test": guard["test"]},
        })
        source = {"kind": "boundary_state", "port": "instant_port:900", "test": dict(guard["test"])}
        region["inputs"].append({"source": source, "role": "request_sample"})
        self.load_data(data)
        for port in ("instant_port:999", "instant_port:0", "instant_port:2"):
            source["port"] = port
            with self.subTest(port=port), self.assertRaises(GraphFormatError):
                self.load_data(data)

    def test_instant_graph_observations_keep_typed_boundary_bindings(self):
        data = self.instant_layer_document()
        graph = data["graph"]
        region = graph["instant_layer_regions"][0]
        wire = next(node for node in graph["nodes"] if node["kind"]["kind"] == "wire")
        plan = next(plan for plan in graph["wire_plans"] if plan["id"] == wire["kind"]["plan"])
        graph["instant_boundary_ports"].append({
            "id": "instant_port:3", "owner": region["cells"][0], "direction": "output",
            "binding": {"kind": "wire_neighborhood", "wire": wire["id"], "neighbor": plan["canonical_neighbors"][0]},
        })
        region["graph_outputs"] = ["instant_port:3"]
        self.load_data(data)
        for invalid in (["instant_port:3", "instant_port:3"], ["instant_port:999"],
                        ["instant_port:0"], ["instant_port:2"], "instant_port:3"):
            with self.subTest(invalid=invalid):
                region["graph_outputs"] = invalid
                with self.assertRaises(GraphFormatError):
                    self.load_data(data)

    def test_instant_layer_tables_validate_local_dag_and_preserve_occurrences(self):
        data = self.instant_layer_document()
        actor_routes = data["graph"]["pistons"][0]["routes"]
        loaded = self.load_data(data)
        self.assertIn("instant_layer_region:7", loaded.records["instant_layer_region"])
        self.assertEqual(len(loaded.record("instant_layer_region:7")["inputs"]), 5)
        self.assertEqual(len(loaded.record("instant_layer_region:7")["layers"][0]["uses"]), 2)

        # Connected/shared cells may have several conditional transfer occurrences.
        transfers = data["graph"]["instant_layer_regions"][0]["resource_outputs"]
        transfers.append({**transfers[0], "transfer_when": 4})
        self.assertEqual(len(self.load_data(data).record("instant_layer_region:7")["resource_outputs"]), 2)

        uses = data["graph"]["instant_layer_regions"][0]["layers"][0]["uses"]
        uses.append({**uses[0], "accepted": 3})
        self.assertEqual(len(self.load_data(data).record("instant_layer_region:7")["layers"][0]["uses"]), 3)

        for mutation in (
            lambda region: region.update(cells=["instant_cell:999"]),
            lambda region: region["expressions"][4].update(value=4),
            lambda region: region.update(stationary_entry=99),
            lambda region: region["outputs"][0].update(emitted_when=99),
            lambda region: region["inputs"][0]["source"].update(port="instant_port:999"),
            lambda region: region["inputs"][1]["source"].update(link="power_link:999"),
            lambda region: region["inputs"][2]["source"]["test"].update(piston="resource:0"),
            lambda region: region["inputs"][3]["source"].update(observer="node:999"),
            lambda region: region["inputs"][3]["source"].update(link="power_link:0"),
            lambda region: region["inputs"][4]["source"]["test"].update(resource="resource:999"),
            lambda region: region["resource_outputs"][0].update(source="contact:999"),
            lambda region: region.update(resource_outputs=[]),
            lambda region: region["resource_outputs"][0].update(destination=region["resource_outputs"][0]["source"]),
            lambda region: region["cold_lifecycle_occurrences"][0]["occurrence"].update(ordinal=99),
            lambda region: region["cold_lifecycle_occurrences"][0]["reason"].update(kind="base_pending_self_update"),
            lambda region: region["cold_lifecycle_occurrences"][6]["reason"].update(observer="node:999"),
            lambda region: region["cold_lifecycle_occurrences"][3].update(phase="deferred_resource_restoration"),
            lambda region: region["cold_lifecycle_occurrences"][7]["occurrence"].update(route=actor_routes["base_changed"]),
            lambda region: region["cold_lifecycle_occurrences"][8]["occurrence"].update(route=actor_routes["far_changed"]),
            lambda region: region["layers"][0]["uses"][0].update(actor="piston:999"),
            lambda region: region["layers"][0]["uses"][0]["cause"]["occurrences"][0].update(ordinal=99),
            lambda region: region["layers"][0]["uses"].append(dict(region["layers"][0]["uses"][0])),
        ):
            damaged = self.instant_layer_document()
            mutation(damaged["graph"]["instant_layer_regions"][0])
            with self.subTest(mutation=mutation), self.assertRaises(GraphFormatError):
                self.load_data(damaged)

        for lifecycle_index, source_owner in (
            (7, data["graph"]["pistons"][0]["node"]),
            (8, data["graph"]["instant_cells"][0]["resets"][0]),
        ):
            damaged = self.instant_layer_document()
            occurrence = damaged["graph"]["instant_layer_regions"][0]["cold_lifecycle_occurrences"][lifecycle_index]["occurrence"]
            route = next(route for route in damaged["graph"]["routes"] if route["id"] == occurrence["route"])
            route["source"] = source_owner
            with self.subTest(lifecycle_index=lifecycle_index), self.assertRaises(GraphFormatError):
                self.load_data(damaged)

        data = self.instant_layer_document()
        graph = data["graph"]
        graph["instant_layer_regions"].append({**graph["instant_layer_regions"][0], "id": "instant_layer_region:9"})
        with self.assertRaisesRegex(GraphFormatError, "shares cells ownership"):
            self.load_data(data)

    def test_layer_threshold_inputs_validate_direction_strength_and_wire_ownership(self):
        data = self.instant_layer_document()
        graph = data["graph"]
        observer = graph["instant_cells"][0]["resets"][0]
        site = next(site["id"] for site in graph["sites"] if site["source"] == {"kind": "component", "node": observer})
        source = {"kind": "source_above", "site": site, "port": {"kind": "strong", "face": "bottom"}, "threshold": 3}
        graph["instant_layer_regions"][0]["inputs"].append({"source": source, "role": "request_sample"})
        self.load_data(data)
        for replacement in (
            {"threshold": -1}, {"threshold": 15}, {"threshold": True}, {"site": "site:999999"},
            {"port": {"kind": "weak", "face": "diagonal"}}, {"port": {"kind": "unknown"}},
        ):
            damaged = json.loads(json.dumps(data))
            damaged["graph"]["instant_layer_regions"][0]["inputs"][-1]["source"].update(replacement)
            with self.subTest(replacement=replacement), self.assertRaises(GraphFormatError):
                self.load_data(damaged)
        wire = next(node["id"] for node in graph["nodes"] if node["kind"]["kind"] == "wire")
        wire_site = next(site["id"] for site in graph["sites"] if site["source"] == {"kind": "component", "node": wire})
        graph["instant_corridors"][0]["wires"] = [wire]
        source["site"] = wire_site
        with self.assertRaisesRegex(GraphFormatError, "owned internal wire"):
            self.load_data(data)

    def test_instant_layer_declines_and_absent_tables(self):
        data = self.instant_layer_document()
        graph = data["graph"]
        graph["instant_layer_regions"] = []
        graph["instant_layer_declines"] = [{
            "id": "instant_layer_decline:4", "cells": ["instant_cell:0"], "corridors": ["instant_corridor:0"],
            "witness": {"kind": "actor", "piston": graph["pistons"][0]["id"]},
            "reason": "shared_owner_order_unknown",
        }]
        self.assertIn("instant_layer_decline:4", self.load_data(data).records["instant_layer_decline"])
        graph["instant_layer_declines"][0]["witness"]["piston"] = "piston:999"
        with self.assertRaises(GraphFormatError):
            self.load_data(data)

        older = self.instant_boundary_document()
        older["graph"].pop("instant_layer_regions", None)
        older["graph"].pop("instant_layer_declines", None)
        loaded = self.load_data(older)
        self.assertEqual(loaded.records["instant_layer_region"], {})
        self.assertEqual(loaded.records["instant_layer_decline"], {})

    def test_instant_boundary_cells_reject_each_shared_owner_and_legacy_coexistence(self):
        for field in ("actors", "resources", "resets"):
            data = self.instant_boundary_document()
            graph = data["graph"]
            duplicate = {"id": "instant_cell:1", "actors": [], "resources": [], "resets": []}
            duplicate[field] = list(graph["instant_cells"][0][field])
            graph["instant_cells"].append(duplicate)
            with self.subTest(field=field), self.assertRaisesRegex(GraphFormatError, f"shares {field} ownership"):
                self.load_data(data)

        data = self.instant_boundary_document()
        graph = data["graph"]
        graph["instant_regions"] = [{
            "id": "instant_region:0", "wires": [], "actors": [], "resources": [], "resets": [],
            "contacts": [], "readable_sites": [], "power_inputs": [], "power_readers": [], "routes": [],
            "wire_readers": [], "observers": [], "incoming": [], "outgoing": [],
        }]
        with self.assertRaisesRegex(GraphFormatError, "cannot coexist with legacy"):
            self.load_data(data)

    def test_classic_nets_validate_ownership_occurrences_and_compact_masks(self):
        fixture = Path(__file__).resolve().parents[2] / "test_data/graph-fixtures/simple_and_gate-classic.graph.json"
        data = json.loads(fixture.read_text(encoding="utf-8"))
        self.load_data(data)
        self.assertTrue(data["graph"]["classic_nets"])
        sampled = json.loads(json.dumps(data))
        operation = next(entry for entry in sampled["graph"]["classic_nets"][0]["entries"] if entry["ops"])["ops"][0]
        operation["sample"] = {"kind": "comparator", "strength": 7, "powered": True}
        operation["nonzero"] = None
        self.load_data(sampled)
        for mutation in ("mask", "sample", "attenuation", "occurrence", "overlap"):
            damaged = json.loads(json.dumps(data))
            net = damaged["graph"]["classic_nets"][0]
            if mutation == "mask":
                next(entry for entry in net["entries"] if entry["ops"])["ops"][0]["when"][0] = "invalid"
            elif mutation == "sample":
                next(entry for entry in net["entries"] if entry["ops"])["ops"][0]["sample"] = {"kind": "comparator", "strength": 16, "powered": True}
            elif mutation == "attenuation":
                net["wires"][0]["attenuation"] = 16
            elif mutation == "occurrence":
                net["entries"][0]["ordinal"] = 999999
            else:
                copied = json.loads(json.dumps(net))
                copied["id"] = "classic_net:1"
                damaged["graph"]["classic_nets"].append(copied)
            with self.subTest(mutation=mutation), self.assertRaises(GraphFormatError):
                self.load_data(damaged)

    def test_wire_hop_roots_bind_their_wire_and_allow_older_exports(self):
        data = document()
        site = {"id": "site:1", "position": {"x": 1, "y": 0, "z": 0},
                "source": {"kind": "component", "node": "node:1"}}
        data["graph"]["sites"].append(site)
        wire = data["graph"]["wire_plans"][0]
        wire["hop_root"] = "site:1"
        self.load_data(data)
        for root in ("site:999", "node:1", 0):
            wire["hop_root"] = root
            with self.assertRaises(GraphFormatError):
                self.load_data(data)
        wire["hop_root"] = "site:1"
        site["source"]["node"] = "node:2"
        with self.assertRaises(GraphFormatError):
            self.load_data(data)
        wire["hop_root"] = None
        self.load_data(data)
        del wire["hop_root"]
        self.load_data(data)

    def test_builtin_resets_preserve_projection_and_reject_output_routes(self):
        data = self.instant_document()
        observer = data["graph"]["nodes"][3]
        observer["kind"]["builtin_reset"] = True
        self.assertTrue(self.load_data(data).record("node:3")["kind"]["builtin_reset"])
        observer["kind"]["builtin_reset"] = "true"
        with self.assertRaises(GraphFormatError):
            self.load_data(data)
        observer["kind"]["builtin_reset"] = True
        data["graph"]["routes"] = [{"id": "route:0", "source": "node:3", "deliveries": []}]
        observer["output_route"] = "route:0"
        with self.assertRaises(GraphFormatError):
            self.load_data(data)
        observer["output_route"] = None
        del observer["kind"]["builtin_reset"]
        self.load_data(data)

    def load_data(self, data: dict):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "graph.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            return load_graph(path)

    def instant_document(self):
        data = document()
        graph = data["graph"]
        graph["nodes"].extend([
            {"id": "node:3", "kind": {"kind": "observer", "watched": "site:0"}, "output_route": None},
            {"id": "node:4", "kind": {"kind": "piston", "piston": "piston:0"}, "output_route": None},
            {"id": "node:5", "kind": {"kind": "contact", "contact": "contact:0"}, "output_route": None},
        ])
        graph["profiles"] = [{"id": "profile:0"}]
        graph["contacts"] = [{"id": "contact:0", "node": "node:5", "variants": []}]
        graph["pistons"] = [{"id": "piston:0", "node": "node:4", "base": "contact:0", "near": "contact:0",
                             "far": "contact:0", "lane": {"kind": "empty"}}]
        graph["resources"] = [{"id": "resource:0", "material": "profile:0", "contacts": ["contact:0"]}]
        graph["wire_plans"][0]["instant_reset"] = {
            "piston": "piston:0", "resource": "resource:0", "observer": "node:3", "partner": "node:2",
        }
        return data

    def test_loads_additive_instant_bindings_and_older_exports(self):
        data = self.instant_document()
        loaded = self.load_data(data)
        self.assertEqual(loaded.record("wire:0")["instant_reset"]["partner"], "node:2")
        data["graph"]["wire_plans"][0]["instant_reset"] = None
        self.load_data(data)
        del data["graph"]["wire_plans"][0]["instant_reset"]
        self.load_data(data)

    def test_rejects_malformed_or_overlapping_instant_bindings(self):
        valid = self.instant_document()["graph"]["wire_plans"][0]["instant_reset"]
        invalid = [[], False, "reset", {}, {**valid, "partner": "node:1"}, {**valid, "partner": "node:0"},
                   {**valid, "observer": "node:0"}, {**valid, "observer": "node:99"},
                   {**valid, "resource": "piston:0"}]
        for binding in invalid:
            with self.subTest(binding=binding):
                data = self.instant_document()
                data["graph"]["wire_plans"][0]["instant_reset"] = binding
                with self.assertRaises(GraphFormatError):
                    self.load_data(data)
        data = self.instant_document()
        data["graph"]["wire_plans"][1]["instant_reset"] = {**valid, "partner": "node:1"}
        with self.assertRaisesRegex(GraphFormatError, "overlapping"):
            self.load_data(data)

    def test_loads_a_valid_dust_path_certificate(self):
        graph = self.load_data(document())
        self.assertEqual(graph.record("dust_path:0")["wires"], ["node:1", "node:2"])

    def test_rejects_dangling_dust_path_references(self):
        data = document()
        data["graph"]["dust_paths"][0]["wires"][1] = "node:99"
        with self.assertRaisesRegex(GraphFormatError, "missing node ID"):
            self.load_data(data)

        data = document()
        data["graph"]["dust_paths"][0]["input_face"] = "up"
        with self.assertRaisesRegex(GraphFormatError, "input face must be horizontal"):
            self.load_data(data)

    def test_rejects_duplicate_dust_path_ids_and_wire_membership(self):
        data = document()
        duplicate = dict(data["graph"]["dust_paths"][0])
        data["graph"]["dust_paths"].append(duplicate)
        with self.assertRaisesRegex(GraphFormatError, "duplicate dust_path ID"):
            self.load_data(data)

        data = document()
        data["graph"]["dust_paths"][0]["wires"] = ["node:1", "node:1"]
        with self.assertRaisesRegex(GraphFormatError, "repeats or shares"):
            self.load_data(data)

    def test_accepts_older_exports_without_the_additive_table(self):
        data = document()
        del data["graph"]["dust_paths"]
        graph = self.load_data(data)
        self.assertEqual(graph.records["dust_path"], {})

    def test_validates_compiled_wire_inputs_and_accepts_older_facts(self):
        data = document()
        facts = {"known": 0xffffff, "solid": 0, "transparent": 0, "wire": 0, "zero_no_dust": 0}
        data["graph"]["wire_plans"][0]["query_facts"] = facts
        self.load_data(data)
        facts["transfer"] = {"seed": {"direct": 4, "conditional": 1 << 20}, "turbo": {"direct": 1, "conditional": 1 << 7}}
        self.load_data(data)
        for invalid in (None, [], {"direct": True, "conditional": 0}, {"direct": 1 << 23, "conditional": 0},
                        {"direct": 1 << 7, "conditional": 1 << 7}, {"direct": 0, "conditional": 1 << 2},
                        {"direct": 4, "conditional": 0}):
            facts["transfer"]["turbo"] = invalid
            with self.subTest(inputs=invalid), self.assertRaises(GraphFormatError):
                self.load_data(data)

    def test_accepts_wire_query_facts_and_absent_older_field(self):
        data = document()
        self.load_data(data)
        data["graph"]["wire_plans"][0]["query_facts"] = None
        self.load_data(data)
        facts = {"known": 0xffffff, "solid": 1, "transparent": 2, "wire": 4, "zero_no_dust": 0xfffffe}
        data["graph"]["wire_plans"][0]["query_facts"] = facts
        loaded = self.load_data(data)
        self.assertEqual(loaded.record("wire:0")["query_facts"], facts)
        for isolated in (True, False):
            facts["isolated"] = isolated
            self.assertIs(self.load_data(data).record("wire:0")["query_facts"]["isolated"], isolated)

    def test_private_proxy_exports_validate_boundaries_and_instruction_ordinals(self):
        source = Path(__file__).resolve().parents[2] / "test_data/graph-fixtures/adder-1bit/adder-1bit-o3.graph.json"
        original = json.loads(source.read_text(encoding="utf-8"))
        self.assertEqual(len(self.load_data(original).records["instant_proxy"]), 4)
        mutations = (
            lambda graph: graph["instant_regions"][0]["incoming"][0].update(ordinal=999999),
            lambda graph: graph["instant_proxies"][0].update(region="instant_region:999999"),
            lambda graph: graph["instant_proxies"][0]["routes"][0]["operations"].append("dispatch"),
            lambda graph: graph["instant_proxies"][0]["routes"][0]["operations"].__setitem__(0, "skip"),
            lambda graph: graph["instant_regions"][0].update(stable_inputs="true"),
            lambda graph: graph["instant_proxies"][0]["routes"][0]["operations"].__setitem__(0, {"kind": "sample", "piston": "node:0"}),
            lambda graph: graph["instant_proxies"][0]["private_cells"].append(graph["instant_proxies"][0]["private_cells"][0]),
        )
        for mutation in mutations:
            data = json.loads(json.dumps(original))
            mutation(data["graph"])
            with self.assertRaises(GraphFormatError):
                self.load_data(data)
        older = json.loads(json.dumps(original))
        del older["graph"]["instant_regions"]
        del older["graph"]["instant_proxies"]
        self.load_data(older)

    def test_closed_instant_machine_validates_owners_targets_and_references(self):
        data = document()
        graph = data["graph"]
        machine = {
            "encoding": "closed-instant-machine-v1", "controls": ["node:0"],
            "owners": {"components": [n["id"] for n in graph["nodes"]], "pistons": [], "resources": []},
            "states": [{"components": [0, 0, 0], "pistons": [], "resources": [], "ticks": [], "requests": [], "deferred": []}],
            "transitions": [[{"next": 0}, None]],
        }
        graph["instant_machine"] = machine
        self.load_data(data)
        mutations = [lambda m: m["transitions"][0][0].update(next=1),
                     lambda m: m["controls"].__setitem__(0, "node:999"),
                     lambda m: m["owners"]["components"].pop(),
                     lambda m: m["states"][0]["components"].pop()]
        for mutation in mutations:
            bad = json.loads(json.dumps(data))
            mutation(bad["graph"]["instant_machine"])
            with self.assertRaises(GraphFormatError):
                self.load_data(bad)

    def test_compact_instant_kernel_checks_required_roots_and_typed_ids(self):
        data = self.instant_document()
        graph = data["graph"]
        graph["instant_kernel"] = compact_kernel(graph)
        self.load_data(data)

        piston_id = graph["pistons"][0]["id"]
        node_id = graph["nodes"][0]["id"]
        mutations = (
            lambda kernel: kernel["owners"].pop("components"),
            lambda kernel: kernel["entry"]["future"].pop("ticks"),
            lambda kernel: kernel["phases"][0]["effects"].append(
                {"enabled": 0, "action": "extend", "extended": False}
            ),
            lambda kernel: kernel.update(trigger=piston_id),
            lambda kernel: kernel["phases"][0]["components"].append(
                {"node": piston_id, "changed": 0, "value": 0}
            ),
            lambda kernel: kernel["phases"][0]["effects"].append(
                {"enabled": 0, "piston": node_id, "action": "extend", "extended": False}
            ),
            lambda kernel: kernel["expressions"][0].update(value="00"),
            lambda kernel: kernel["expressions"][0].update(value="9" * 5000),
            lambda kernel: kernel.update(period=0),
        )
        for mutation in mutations:
            invalid = json.loads(json.dumps(data))
            mutation(invalid["graph"]["instant_kernel"])
            with self.subTest(kernel=invalid["graph"]["instant_kernel"]), self.assertRaises(GraphFormatError):
                self.load_data(invalid)

        max_u64 = json.loads(json.dumps(data))
        max_u64["graph"]["instant_kernel"]["expressions"][0]["value"] = "18446744073709551615"
        self.load_data(max_u64)

    def test_rejects_invalid_wire_query_fact_masks(self):
        valid = {"known": 0xffffff, "solid": 1, "transparent": 2, "wire": 4, "zero_no_dust": 0xfffffe}
        invalid = [[], False, "masks", {}, {**valid, "known": 0}, {**valid, "zero_no_dust": 1}]
        invalid.extend({**valid, "isolated": value} for value in (None, 0, 1, "true", [], {}))
        for key in valid:
            for value in (-1, 1 << 24, 0.5, True, None, "1"):
                invalid.append({**valid, key: value})
        for facts in invalid:
            with self.subTest(facts=facts):
                data = document()
                data["graph"]["wire_plans"][0]["query_facts"] = facts
                with self.assertRaises(GraphFormatError):
                    self.load_data(data)


if __name__ == "__main__":
    unittest.main()
