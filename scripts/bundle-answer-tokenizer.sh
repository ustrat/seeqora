#!/usr/bin/env bash
# Put the Qwen2.5 tokenizer (Apache-2.0, shared by every Qwen2.5 size) into
# <dest>, for the candle answer generator (iOS for now). Android runs
# llama.cpp, which reads the tokenizer from the GGUF, so its APK ships none
# (docs/DECISION_LOG.md).
#
#   bash scripts/bundle-answer-tokenizer.sh <dest>/qwen2.5
set -euo pipefail

DEST="${1:?destination directory}"
URL="https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/resolve/main/tokenizer.json"

mkdir -p "$DEST"
if [ ! -s "$DEST/tokenizer.json" ]; then
  curl -fsSL --retry 3 -o "$DEST/tokenizer.json" "$URL"
fi
# Fail the build on an HTML error page or truncated download.
python3 - "$DEST/tokenizer.json" <<'PY'
import json, sys
t = json.load(open(sys.argv[1], encoding="utf-8"))
added = {a["content"] for a in t.get("added_tokens", [])}
assert {"<|im_start|>", "<|im_end|>", "<|endoftext|>"} <= added, "not a Qwen2.5 tokenizer"
PY
echo "bundled Qwen2.5 tokenizer: $(du -h "$DEST/tokenizer.json" | cut -f1)"
