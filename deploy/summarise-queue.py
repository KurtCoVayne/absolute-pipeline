#!/usr/bin/env python3 -I
"""Each seed post's outcome from the file-backed queue, as Markdown rows."""
import json, collections, sys
q = sys.argv[1]
def rd(t):
    out = []
    try:
        for l in open(f"{q}/{t}.jsonl"):
            try:
                out.append(json.loads(l))
            except Exception:
                pass
    except FileNotFoundError:
        pass
    return out
docs, tri, sub, fail, jobs = rd("docs"), rd("triage"), rd("submissions"), rd("failures"), rd("jobs")
spend = sum(j.get("cost_usd") or 0 for j in jobs)
print("SPEND", round(spend, 4), "JOBS", dict(collections.Counter(j["stage"] for j in jobs)))
docs.sort(key=lambda d: d["fixed_at"])
# Each post's latest job per stage (the queue's jobs in the order they ended).
latest = {}
for j in jobs:
    latest[(j.get("post_id"), j["stage"])] = j
outc = collections.Counter()
for d in docs:
    pid, did = d["source_id"], d["id"]
    v = next((t.get("verdict", "") for t in reversed(tri) if t.get("doc_id") == did), "")
    s = next((x for x in sub if x.get("post_id") == pid), None)
    a, q = latest.get((pid, "analyze")), latest.get((pid, "strategize"))
    if s:
        o = "**submission** " + s["id"][:12]
        key = "submission"
    elif q is not None:
        f = q.get("failure") or {}
        o = f'quant `{f.get("reason") or q.get("state")}`: ' + (f.get("detail") or "").replace("|", "/").replace("\n", " ")[:170]
        key = "quant:" + (f.get("reason") or str(q.get("state")))
    elif a is not None and (a.get("failure") or {}).get("state") == "done":
        o, key = "analyst: no hypothesis, or the quant has not finished", "analyst:none_or_pending"
    elif a is not None:
        f = a.get("failure") or {}
        o, key = f'analyst `{f.get("reason")}`', "analyst:" + str(f.get("reason"))
    else:
        o, key = "triage only", "triage"
    outc[key] += 1
    print(f'| {d["fixed_at"][:16]} | @{d["author"]["handle"]} | {pid} | {v} | {o} |')
print("OUTCOMES", dict(outc))
