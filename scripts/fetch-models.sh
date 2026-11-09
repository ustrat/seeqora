#!/usr/bin/env bash
# Download candidate embedding models from Hugging Face into
# <out>/<name>/{config.json,tokenizer.json,model.safetensors,seeqora.json}.
# Model files are never committed (see .gitignore).
#
#   bash scripts/fetch-models.sh models            # all candidates in eval/models.json
#   bash scripts/fetch-models.sh models bge-small-en-v1.5
set -euo pipefail

OUT="${1:-models}"
ONLY="${2:-}"
cd "$(dirname "$0")/.."

python3 - "$OUT" "$ONLY" <<'PY'
import json, os, sys, urllib.request

out, only = sys.argv[1], sys.argv[2]
for m in json.load(open("eval/models.json")):
    if only and m["name"] != only:
        continue
    d = os.path.join(out, m["name"])
    os.makedirs(d, exist_ok=True)
    for f in ("config.json", "tokenizer.json", "model.safetensors"):
        p = os.path.join(d, f)
        if not os.path.exists(p):
            url = f"https://huggingface.co/{m['repo']}/resolve/main/{f}"
            print(f"fetch {url}", flush=True)
            urllib.request.urlretrieve(url, p)
    json.dump(
        {"id": f"{m['name']}@1", "pooling": m["pooling"], "query_prefix": m["query_prefix"]},
        open(os.path.join(d, "seeqora.json"), "w"),
    )
    print(f"ok {m['name']}: {os.path.getsize(os.path.join(d, 'model.safetensors')) // 1_000_000} MB", flush=True)
PY
