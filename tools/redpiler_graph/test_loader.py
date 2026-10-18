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
    def test_classic_nets_validate_ownership_occurrences_and_compact_masks(self):
        fixture = Path(__file__).resolve().parents[2] / "test_data/graph-fixtures/simple_and_gate-classic.graph.json"
        data = json.loads(fixture.read_text(encoding="utf-8"))
        self.load_data(data)
        self.assertTrue(data["graph"]["classic_nets"])
        for mutation in ("mask", "attenuation", "occurrence", "overlap"):
            damaged = json.loads(json.dumps(data))
            net = damaged["graph"]["classic_nets"][0]
            if mutation == "mask":
                next(entry for entry in net["entries"] if entry["ops"])["ops"][0]["when"][0] = "invalid"
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
