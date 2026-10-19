#!/usr/bin/env python3 -I
"""PL-D16 (the owner, Oct 19: "data is valid, mark everything complete"):
marks the current hot bar versions `reviewed = complete` in the registry
and in their manifests, with the services stopped. For Databento's
OHLCV-1m and Norgate's daily bars a missing bar means the instrument did
not trade, which is what `complete` asserts (V-3.11, K-3.2). A hot
version's id hashes its blocks, not its attributes (A-D48), so no id
changes. The previous files are kept beside them as .bak."""
import json
import shutil
import sys

store = sys.argv[1]
reg_path = f"{store}/registry/registry.json"
reg = json.load(open(reg_path))
shutil.copy(reg_path, reg_path + ".bak")
bars = {"open", "high", "low", "close", "volume"}
n = 0
for rel, by_src in reg["current"].items():
    cls, _, name = rel.partition(".")
    if cls not in ("future", "equity") or name not in bars:
        continue
    for src, vid in by_src.items():
        v = reg["versions"][vid]
        if v.get("attributes", {}).get("storage") != "hot":
            continue
        v["attributes"]["reviewed"] = "complete"
        mp = f"{store}/manifest/{rel}/{vid}.json"
        m = json.load(open(mp))
        shutil.copy(mp, mp + ".bak")
        attrs = m.get("attributes")
        if isinstance(attrs, dict):
            attrs["reviewed"] = "complete"
        elif attrs is None:
            m["attributes"] = {"reviewed": "complete"}
        json.dump(m, open(mp, "w"), indent=2)
        n += 1
        print(rel, src, vid[:12])
json.dump(reg, open(reg_path, "w"))
print(n, "versions marked")
