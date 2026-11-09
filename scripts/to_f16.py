"""Rewrite a safetensors file with F32 tensors stored as F16 (half the size).

The core loads weights as F32 either way; this only shrinks what ships in the
app. Other dtypes (e.g. I64 position ids) are copied unchanged.

    python3 scripts/to_f16.py model.safetensors model-f16.safetensors
"""
import json
import struct
import sys

import numpy as np


def main(src: str, dst: str) -> None:
    with open(src, "rb") as f:
        n = struct.unpack("<Q", f.read(8))[0]
        header = json.loads(f.read(n))
        data = f.read()
    meta = header.pop("__metadata__", None)
    out, parts, off = {}, [], 0
    for name, t in sorted(header.items(), key=lambda kv: kv[1]["data_offsets"][0]):
        start, end = t["data_offsets"]
        buf, dtype = data[start:end], t["dtype"]
        if dtype == "F32":
            buf, dtype = np.frombuffer(buf, dtype="<f4").astype("<f2").tobytes(), "F16"
        out[name] = {"dtype": dtype, "shape": t["shape"], "data_offsets": [off, off + len(buf)]}
        parts.append(buf)
        off += len(buf)
    if meta:
        out["__metadata__"] = meta
    h = json.dumps(out, separators=(",", ":")).encode()
    h += b" " * ((8 - len(h) % 8) % 8)
    with open(dst, "wb") as f:
        f.write(struct.pack("<Q", len(h)))
        f.write(h)
        for p in parts:
            f.write(p)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
