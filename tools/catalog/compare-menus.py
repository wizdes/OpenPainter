#!/usr/bin/env python3
"""Compares OpenPainter's live menu bar with the paint.net 5.1 menu catalog (M55).

Usage:
  tools/catalog/compare-menus.py [--menu FILE] [--catalog FILE] [--strict]

--menu is the live menu bar as MenuCatalogTests writes it on every
`make -C mac test` run: mac/build/menu-live.tsv, one visible item per line,
"path<TAB>shortcut<TAB>on|off<TAB>icon|-<TAB>command". When that file is
missing, the script falls back to the checked-in golden list
(mac/Tests/OpenPainterKitTests/menu-skeleton.txt, which MenuTests holds equal to
the live menu) and skips the icon and command checks.

It prints every difference: missing items, extra items, wrong order, wrong
shortcut, wrong command, wrong on/off state, missing or extra icons and shared
shortcuts. Items the catalog marks "not in v1" (absent) and "pending" (off, or
absent) are printed as allowed differences. Exit 0 when nothing else differs;
--strict also fails on pending items. MenuCatalogTests runs the same check in
process.
"""
import argparse
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def load_live(path):
    """Returns ([(path, key, off, icon, command)], has_details)."""
    items = []
    text = Path(path).read_text(encoding="utf-8")
    if path.suffix == ".tsv":
        for line in text.splitlines():
            if not line or line.startswith("#"):
                continue
            p, key, state, icon, command = line.split("\t")
            items.append((p, key, state == "off", icon == "icon", command))
        return items, True
    for line in text.splitlines():
        if not line or line.startswith("#") or line.endswith(" > ---"):
            continue
        off = line.endswith(" (off)")
        if off:
            line = line[: -len(" (off)")]
        key = ""
        if line.endswith("]") and " [" in line:
            line, key = line[: line.rindex(" [")], line[line.rindex(" [") + 2: -1]
        items.append((line, key, off, None, None))
    return items, False


def parent(path):
    return path.rsplit(" > ", 1)[0] if " > " in path else ""


def compare(catalog, live, details):
    """Returns (failures, allowed) as lists of strings."""
    failures, allowed = [], []
    expected = [i for i in catalog["item"] if i["mac"]]
    by_path = {}
    for i in expected:
        if i["mac"] in by_path:
            failures.append(f"CATALOG  {i['mac']} is listed twice")
        by_path[i["mac"]] = i
    for i in catalog["item"]:
        if i["status"] == "not in v1":
            if i["mac"]:
                failures.append(f"CATALOG  {i['pdn']} is not in v1 but has a Mac path")
            allowed.append(f"NOT IN V1  {i['pdn']}: {i.get('reason', '')}")
        elif i["status"] == "pending" and not i["mac"]:
            allowed.append(f"PENDING    {i['pdn'] or i['mac']} (absent): {i.get('reason', '')}")
    live_by_path = {p: (k, off, icon, cmd) for p, k, off, icon, cmd in live}
    for p in by_path:
        if p not in live_by_path:
            failures.append(f"MISSING  {p}")
    for p, *_ in live:
        if p not in by_path:
            failures.append(f"EXTRA    {p}")
    # Order inside each menu, over the items both sides have.
    parents = []
    for i in expected:
        if parent(i["mac"]) not in parents:
            parents.append(parent(i["mac"]))
    for par in parents:
        want = [i["mac"] for i in expected if parent(i["mac"]) == par and i["mac"] in live_by_path]
        got = [p for p, *_ in live if parent(p) == par and p in by_path]
        if want != got:
            failures.append(f"ORDER    {par or '(menu bar)'}: expected {want} got {got}")
    for p, (key, off, icon, cmd) in live_by_path.items():
        i = by_path.get(p)
        if i is None:
            continue
        if key != i["key"]:
            failures.append(f"SHORTCUT {p}: expected [{i['key']}] got [{key}]")
        is_submenu = any(parent(q) == p for q in live_by_path)
        if i["status"] == "pending":
            if not off:
                failures.append(f"STATE    {p}: pending items stay off")
            allowed.append(f"PENDING    {p} (off): {i.get('reason', '')}")
        elif off and not is_submenu:
            failures.append(f"STATE    {p}: off, but the catalog says it is built ({i['status']})")
        if details:
            if i["icon"] and not icon:
                failures.append(f"ICON     {p}: paint.net shows an icon; the Mac item has none")
            # macOS adds its own icons to standard Mac items (Hide, Minimize, Revert).
            if icon and not i["icon"] and i["pdn"]:
                failures.append(f"ICON     {p}: has an icon paint.net does not show")
            if not is_submenu and i["command"] != cmd:
                failures.append(f"COMMAND  {p}: expected {i['command']!r} got {cmd!r}")
    shared = set(catalog.get("shared_shortcuts", []))
    seen = {}
    for p, key, *_ in live:
        if key and key not in shared:
            seen.setdefault(key, []).append(p)
    for key, paths in seen.items():
        if len(paths) > 1:
            failures.append(f"DUPLICATE [{key}]: {paths}")
    return failures, allowed


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--menu", type=Path)
    ap.add_argument("--catalog", type=Path, default=ROOT / "tests/catalog/paintnet-5.1-menus.toml")
    ap.add_argument("--strict", action="store_true", help="pending items count as differences")
    a = ap.parse_args()
    menu = a.menu
    if menu is None:
        menu = ROOT / "mac/build/menu-live.tsv"
        if not menu.exists():
            menu = ROOT / "mac/Tests/OpenPainterKitTests/menu-skeleton.txt"
            print(f"note: {ROOT / 'mac/build/menu-live.tsv'} is missing (run `make -C mac test`); "
                  "using the golden list, without the icon and command checks")
    catalog = tomllib.loads(a.catalog.read_text(encoding="utf-8"))
    live, details = load_live(menu)
    failures, allowed = compare(catalog, live, details)
    for line in allowed:
        print(line)
    for line in failures:
        print(line)
    pending = [x for x in allowed if x.startswith("PENDING")]
    print(f"{len(live)} live items, {len(catalog['item'])} catalog items: {len(failures)} differences, "
          f"{len(allowed)} allowed ({len(pending)} pending)")
    if failures or (a.strict and pending):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
