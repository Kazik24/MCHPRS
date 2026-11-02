#!/usr/bin/env python3
"""Download RedstoneDB build metadata and schematic files into test_data/redstonedb."""

import argparse
import hashlib
import json
import re
import time
from html.parser import HTMLParser
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen


BASE = "https://redstonedb.com"
BUILD_PATH = re.compile(r"^/schematics/([0-9a-f]{32})$")


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.fields = {}
        self.title = ""
        self.capture = None
        self.capture_tag = None
        self.capture_depth = 0
        self.pending_key = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "a" and attrs.get("href"):
            self.links.append(attrs["href"].split("?", 1)[0])
        if tag in ("dt", "dd", "title"):
            self.capture, self.capture_tag, self.capture_depth = [], tag, 1
        elif self.capture is not None:
            self.capture_depth += 1

    def handle_endtag(self, tag):
        if self.capture is None:
            return
        if tag == self.capture_tag and self.capture_depth == 1:
            value = " ".join(" ".join(self.capture).split())
            if tag == "dt":
                self.pending_key = value
            elif tag == "dd" and self.pending_key:
                self.fields[self.pending_key] = value
                self.pending_key = None
            elif tag == "title":
                self.title = value
            self.capture = self.capture_tag = None
        else:
            self.capture_depth -= 1

    def handle_data(self, data):
        if self.capture is not None:
            self.capture.append(data)


def get(url):
    req = Request(url, headers={"User-Agent": "MCHPRS schematic test fixture collector"})
    with urlopen(req, timeout=45) as response:
        return response.read(), response.headers


def parse(data):
    page = Page()
    page.feed(data.decode("utf-8", errors="replace"))
    return page


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=Path("test_data/redstonedb"))
    ap.add_argument("--delay", type=float, default=0.4, help="seconds between requests")
    ap.add_argument("--id", action="append", help="download only this RedstoneDB build ID; repeatable")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    schematics = args.out / "schematics"
    schematics.mkdir(exist_ok=True)
    manifest_path = args.out / "manifest.json"
    manifest = (
        json.loads(manifest_path.read_text(encoding="utf-8-sig"))
        if manifest_path.exists()
        else {}
    )
    if isinstance(manifest, list):
        manifest = {record["id"]: record for record in manifest}

    if args.id:
        ids = list(dict.fromkeys(args.id))
        if any(not re.fullmatch(r"[0-9a-f]{32}", build_id) for build_id in ids):
            raise SystemExit("Build IDs must be 32 lowercase hexadecimal characters")
    else:
        listing, _ = get(BASE + "/builds")
        ids = sorted({match.group(1) for href in parse(listing).links if (match := BUILD_PATH.match(href))})
    if not ids:
        raise SystemExit("No build links found on /builds; site markup may have changed")

    for index, build_id in enumerate(ids, 1):
        url = f"{BASE}/schematics/{build_id}"
        try:
            page_data, _ = get(url)
            page = parse(page_data)
            filename = page.fields.get("Name", f"{build_id}.schem")
            # Keep site filenames, but never allow a remote path to leave our fixture directory.
            filename = Path(filename.replace("\\", "/")).name
            if not filename.lower().endswith((".schem", ".schematic")):
                filename += ".schem"
            file_path = schematics / f"{build_id}-{filename}"
            record = {"id": build_id, "title": page.title.removesuffix(" · RedstoneDB"), "url": url, "metadata": page.fields}
            if not file_path.exists():
                data, _ = get(url + "/download")
                file_path.write_bytes(data)
                time.sleep(args.delay)
            record["file"] = file_path.relative_to(args.out).as_posix()
            record["sha256"] = hashlib.sha256(file_path.read_bytes()).hexdigest()
            manifest[build_id] = record
            if index % 10 == 0 or index == len(ids):
                manifest_path.write_text(json.dumps(list(manifest.values()), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
                print(f"[{index}/{len(ids)}] {record['title']}")
        except (OSError, HTTPError, TimeoutError) as error:
            print(f"Failed {build_id}: {error}")
        finally:
            time.sleep(args.delay)
    manifest_path.write_text(json.dumps(list(manifest.values()), indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"Saved {len(manifest)} records to {manifest_path}")


if __name__ == "__main__":
    main()
