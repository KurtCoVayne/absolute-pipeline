#!/bin/sh
# Run GP-5's usability task on MODEL through the harness (the integrator's gateway):
#   ABT_GATEWAY=http://127.0.0.1:7777 ABT_TOKEN_FILE=/path/to/token usability/run.sh gpt-6-luna
#   usability/run.sh gpt-6-luna --dry-run        # prints the prompt, calls nothing
# Then: .venv/bin/python usability/score.py usability/runs/<dir> --run-decisions N
set -eu
MODEL=${1:?usage: usability/run.sh MODEL [--dry-run]}
shift
HERE=$(cd "$(dirname "$0")" && pwd)
PY=${PY:-$HERE/../.venv/bin/python}
[ -x "$PY" ] || PY=python3
exec "$PY" "$HERE/run.py" --model "$MODEL" "$@"
