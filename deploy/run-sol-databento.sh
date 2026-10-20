#!/usr/bin/env bash
# Every pending post on gpt-6.1-sol against the Databento futures store (the owner, Oct 20: futures from
# Databento only). Requeues the jobs the 04:30 out-of-memory kill left "running", and the full backtests
# made on Norgate's futures (set aside in the attic, not deleted); then one queue of quant jobs, six at a time.
#   FUTURES_STORE=/srv/absolute/store-futures-current bash deploy/run-sol-databento.sh
#   ONLY_MARKETS='\b(ES|NQ|SPY|QQQ)\b' OTHER_MARKETS='\b(CL|GC)\b' …: only the posts on the snapshot's markets now
set -uo pipefail
: "${FUTURES_STORE:?the Databento snapshot the futures service serves}"
export FUTURES_STORE FUTURES_SOCK=${FUTURES_SOCK:-/srv/absolute/run/futures.sock}
D=/srv/absolute/direct; A=$D/attic/databento-requeue-$(date -u +%H%M); mkdir -p "$A"
cd "$D/out"
ids=$(python3 -I - "$A" <<'EOF'
import json, glob, os, shutil, sys
attic = sys.argv[1]
out = []
for f in sorted(glob.glob("*/status.json")):
    p = f.split("/")[0]
    s = json.load(open(f))
    state, stage = s.get("state"), s.get("stage")
    norgate = state == "done" and s.get("by") == "claude-opus"
    if (state, stage) == ("queued", "quant") or state == "running" or norgate:
        if state != "queued" or norgate:
            os.makedirs(f"{attic}/{p}", exist_ok=True)
            for name in ("verdict.json", "report.json", "reveal.json", "nav.csv", "quant.json", "check.json", "status.json"):
                if os.path.exists(f"{p}/{name}"):
                    shutil.copy2(f"{p}/{name}", f"{attic}/{p}/{name}") if name == "status.json" else shutil.move(f"{p}/{name}", f"{attic}/{p}/{name}")
            if os.path.isdir(f"/srv/absolute/direct/jobs/{p}"):
                shutil.move(f"/srv/absolute/direct/jobs/{p}", f"{attic}/{p}/job")
            reason = "requeued on Databento futures (the Norgate backtest is in the attic)" if norgate else "requeued after the 04:30 out-of-memory kill"
            json.dump({"state": "queued", "stage": "quant", "reason": reason}, open(f"{p}/status.json", "w"))
        out.append(p)
# ONLY_MARKETS (a regex): run now only the posts whose hypothesis names a market the snapshot holds;
# the others stay queued for the next snapshot.
import re
pat = os.environ.get("ONLY_MARKETS")
if pat:
    other = re.compile(os.environ.get("OTHER_MARKETS", "$^"), re.I)
    keep = []
    for p in out:
        try:
            h = json.load(open(f"{p}/hypothesis.json"))
        except Exception:
            continue
        text = " ".join(str(h.get(k) or "") for k in ("universe", "statement", "signal", "instrument"))
        if re.search(pat, text, re.I) and not other.search(text):
            keep.append(p)
    out = keep
print(",".join(out))
EOF
)
echo "$(echo "$ids" | tr , '\n' | grep -c .) posts queued; attic $A"
cd /srv/absolute/absolute-pipeline/agents-harness/direct
exec /srv/absolute/venv/bin/python run_direct.py --only "$ids" --parallel 6 --cap-usd 45
