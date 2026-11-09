#!/usr/bin/env bash
# Put the shipping embedding model (see docs/DECISION_LOG.md) into <dest> at F16.
#
#   bash scripts/bundle-model.sh android/app/src/main/assets/models/bge-small-en-v1.5
set -euo pipefail

NAME="bge-small-en-v1.5"
DEST="${1:?destination directory}"
cd "$(dirname "$0")/.."

bash scripts/fetch-models.sh models "$NAME"
python3 -c "import numpy" 2>/dev/null || python3 -m pip install --quiet numpy
mkdir -p "$DEST"
cp "models/$NAME/config.json" "models/$NAME/tokenizer.json" "models/$NAME/seeqora.json" "$DEST/"
python3 scripts/to_f16.py "models/$NAME/model.safetensors" "$DEST/model.safetensors"
echo "bundled $NAME (f16): $(du -h "$DEST/model.safetensors" | cut -f1)"
