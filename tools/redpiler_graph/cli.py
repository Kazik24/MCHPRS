"""Command-line inspection for versioned Redpiler graph exports."""

import argparse
import json
import re
import sys
from typing import Any, Iterator

from .loader import GraphDocument, GraphFormatError, TABLES, load_graph

TYPED_ID = re.compile(r"[a-z_]+:(?:0|[1-9][0-9]*)\Z")


def walk_ids(value: Any, path: str = "$", owner: str = "") -> Iterator[tuple[str, str, str]]:
    """Yield each typed ID string with its JSON path and owning record."""
    if isinstance(value, dict):
        for key, child in value.items():
            yield from walk_ids(child, f"{path}.{key}", owner)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk_ids(child, f"{path}[{index}]", owner)
    elif isinstance(value, str) and TYPED_ID.fullmatch(value):
        yield owner, path, value


def summary(document: GraphDocument) -> None:
    data = document.data
    print(f"{document.path}")
    print(f"format: {data['format']} v{data['version']['major']}.{data['version']['minor']} ({data['stage']})")
    print("capabilities: " + (", ".join(data.get("capabilities", [])) or "none"))
    print("records:")
    for table in TABLES.values():
        value = data["graph"].get(table)
        if table == "dust_paths" and value is None:
            value = []
        print(f"  {table}: {'unavailable' if value is None else len(value)}")
    print(f"snapshots: {len(data['snapshots'])}")
    print(f"trace: {'unavailable' if data['trace'] is None else len(data['trace'])}")
    for diagnostic in data.get("diagnostics", []):
        print(f"diagnostic [{diagnostic.get('code', 'unknown')}]: {diagnostic.get('message', '')}")


def show(document: GraphDocument, identifier: str) -> None:
    record = document.record(identifier)
    print(json.dumps(record, indent=2, ensure_ascii=False))


def references(document: GraphDocument, identifier: str) -> None:
    selected = document.record(identifier)
    outgoing = [(path, target) for _, path, target in walk_ids(selected) if target != identifier]
    incoming = []
    for records in document.records.values():
        for owner, record in records.items():
            if owner == identifier:
                continue
            for _, path, target in walk_ids(record, owner=owner):
                if target == identifier:
                    incoming.append((owner, path))

    print(f"outgoing from {identifier}:")
    for path, target in outgoing:
        print(f"  {path} -> {target}")
    print(f"incoming to {identifier}:")
    for owner, path in incoming:
        print(f"  {owner} {path} -> {identifier}")
    if not outgoing and not incoming:
        print("  (no graph references)")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("graph", help="path to a Redpiler graph JSON export")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("summary", help="show schema, availability, and record counts")
    record = commands.add_parser("show", help="print one record by typed ID")
    record.add_argument("id", help="for example node:49 or power_link:0")
    refs = commands.add_parser("refs", help="show graph references in both directions")
    refs.add_argument("id", help="for example node:49 or site:70")
    args = parser.parse_args(argv)

    try:
        document = load_graph(args.graph)
        if args.command == "summary":
            summary(document)
        elif args.command == "show":
            show(document, args.id)
        else:
            references(document, args.id)
    except GraphFormatError as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    sys.exit(main())
