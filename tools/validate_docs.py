"""Check repository Markdown links and local heading anchors without network access."""
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]


def anchors(content):
    slugs = set()
    counts = {}
    fence = None
    for line in content.splitlines():
        marker = re.match(r"^\s*(`{3,}|~{3,})", line)
        if marker:
            if fence is None:
                fence = marker[1][0]
            elif marker[1][0] == fence:
                fence = None
        if fence is not None:
            continue
        heading = re.match(r"^#{1,6}\s+(.+?)\s*#*\s*$", line)
        if heading:
            slug = re.sub(r"[^\w\- ]", "", heading[1].lower()).replace(" ", "-")
            count = counts.get(slug, 0)
            counts[slug] = count + 1
            slugs.add(f"{slug}-{count}" if count else slug)
    return slugs


def check_links(path):
    content = path.read_text(encoding="utf-8")
    for target in re.findall(r"\[[^\]\n]+\]\(([^)\n]+)\)", content):
        if re.match(r"[a-zA-Z][a-zA-Z0-9+.-]*:", target):
            continue
        name, _, anchor = unquote(target.strip("<>")).partition("#")
        linked = (path.parent / name).resolve() if name else path
        assert linked.exists(), (path, target, "missing local target")
        if anchor and linked.suffix == ".md":
            assert anchor in anchors(linked.read_text(encoding="utf-8")), (path, target, "missing heading")


def main():
    files = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "*.md"],
        cwd=ROOT, text=True, encoding="utf-8",
    ).splitlines()
    paths = sorted({ROOT / name for name in files if (ROOT / name).is_file()})
    for path in paths:
        check_links(path)
    print(f"Verified local links and heading anchors in {len(paths)} Markdown files.")


if __name__ == "__main__":
    main()
