"""List, group and search the archive's pack names (the only path text in a Trinity archive).

    python -I .claude/skills/find-assets/packnames.py                   counts by prefix and by extension
    python -I .claude/skills/find-assets/packnames.py <regex> [limit]   pack names matching the regex

The RomFS is taken from $TRINITY_ROMFS, else the folder the viewer remembers. Works with a dumped folder
(arc/data.trpfd) or a romfs.bin + index.json image. The full list is cached in target/packnames.txt, which is
git-ignored: never copy it into tracked files.
"""

import collections
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CACHE = ROOT / "target" / "packnames.txt"


def romfs() -> Path:
    env = os.environ.get("TRINITY_ROMFS")
    if env:
        return Path(env)
    cfg = Path(os.environ.get("APPDATA") or Path.home() / ".config") / "trinity-model-viewer" / "romfs_path.txt"
    return Path(cfg.read_text().strip())


def trpfd_bytes(base: Path) -> bytes:
    if base.is_file():
        base = base.parent
    for cand in (base / "arc" / "data.trpfd", base / "data.trpfd", base / "romfs" / "arc" / "data.trpfd"):
        if cand.is_file():
            return cand.read_bytes()
    index = base / "index.json"
    image = base / "romfs.bin"
    entries = json.loads(index.read_text())["files"]
    e = next(x for x in entries if x["path"] == "arc/data.trpfd")
    with open(image, "rb") as f:
        f.seek(e["offset"])
        return f.read(e["size"])


def names() -> list[str]:
    if CACHE.is_file():
        return CACHE.read_text().split("\n")
    data = trpfd_bytes(romfs())
    out = sorted({m.decode() for m in re.findall(rb"arc/[A-Za-z0-9_.\-]{3,250}\.trpak", data)})
    CACHE.parent.mkdir(exist_ok=True)
    CACHE.write_text("\n".join(out))
    return out


def main() -> None:
    n = names()
    if len(sys.argv) > 1:
        rx = re.compile(sys.argv[1])
        limit = int(sys.argv[2]) if len(sys.argv) > 2 else 50
        hits = [x for x in n if rx.search(x)]
        print(f"{len(hits)} of {len(n)} packs match")
        print("\n".join(hits[:limit]))
        return
    print(f"{len(n)} packs")
    ext = collections.Counter((re.search(r"\.([a-z0-9]+)\.trpak$", x) or [None, "?"])[1] for x in n)
    print("by extension:", ", ".join(f"{k} {v}" for k, v in ext.most_common()))
    pre = collections.Counter((re.match(r"[a-z]+", x[4:]) or [x[4:14]])[0][:24] for x in n)
    print("by prefix:")
    for k, v in pre.most_common(40):
        print(f"  {v:6} {k}")


if __name__ == "__main__":
    main()
