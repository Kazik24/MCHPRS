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


class DustPathLoaderTests(unittest.TestCase):
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
