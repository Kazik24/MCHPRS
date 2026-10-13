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


if __name__ == "__main__":
    unittest.main()
