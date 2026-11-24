#!/usr/bin/env python3
"""Dependency-free PNG decoder + pixel diff used by both visual check layers.

Supports 8-bit RGB/RGBA non-interlaced PNGs, which is what Rune's --snap and
Chrome screenshots produce. No Pillow/NumPy needed in CI.
"""

from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path

_MAX_CHUNK = 1 << 31


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def read_png(path: str | Path) -> tuple[int, int, bytearray]:
    data = Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"not a PNG: {path}")
    pos = 8
    width = height = bit_depth = color_type = interlace = None
    idat = bytearray()
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if length >= _MAX_CHUNK:
            raise ValueError("chunk too large")
        if kind == b"IHDR":
            width, height, bit_depth, color_type, _comp, _filt, interlace = struct.unpack(
                ">IIBBBBB", chunk[:13]
            )
        elif kind == b"IDAT":
            idat.extend(chunk)
        elif kind == b"IEND":
            break
    if bit_depth != 8 or color_type not in (2, 6) or interlace != 0:
        raise ValueError(f"unsupported PNG format: depth={bit_depth} type={color_type} interlace={interlace}")
    bpp = 4 if color_type == 6 else 3
    raw = zlib.decompress(bytes(idat))
    stride = width * bpp
    rows = bytearray(height * stride)
    prev = bytearray(stride)
    p = 0
    for y in range(height):
        f = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        if f == 1:
            for i in range(bpp, stride):
                line[i] = (line[i] + line[i - bpp]) & 0xFF
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 0xFF
        elif f == 3:
            for i in range(stride):
                left = line[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif f == 4:
            for i in range(stride):
                left = line[i - bpp] if i >= bpp else 0
                c = prev[i - bpp] if i >= bpp else 0
                line[i] = (line[i] + _paeth(left, prev[i], c)) & 0xFF
        elif f != 0:
            raise ValueError(f"bad PNG filter {f}")
        rows[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, rows


def _chunk(kind: bytes, payload: bytes) -> bytes:
    return (
        struct.pack(">I", len(payload))
        + kind
        + payload
        + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
    )


def write_png(path: str | Path, width: int, height: int, rows: bytes | bytearray, bpp: int = 4) -> None:
    color_type = 6 if bpp == 4 else 2
    stride = width * bpp
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        raw.extend(rows[y * stride : (y + 1) * stride])
    out = b"\x89PNG\r\n\x1a\n"
    out += _chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, color_type, 0, 0, 0))
    out += _chunk(b"IDAT", zlib.compress(bytes(raw), 6))
    out += _chunk(b"IEND", b"")
    Path(path).write_bytes(out)


def resize_nearest(width: int, height: int, rows: bytes, nw: int, nh: int, bpp: int = 4) -> bytearray:
    out = bytearray(nw * nh * bpp)
    for y in range(nh):
        sy = min(height - 1, y * height // nh)
        for x in range(nw):
            sx = min(width - 1, x * width // nw)
            src = (sy * width + sx) * bpp
            dst = (y * nw + x) * bpp
            out[dst : dst + bpp] = rows[src : src + bpp]
    return out


def compare(
    a_path: str | Path,
    b_path: str | Path,
    out_dir: str | Path | None = None,
    threshold: int = 16,
) -> dict:
    aw, ah, a = read_png(a_path)
    bw, bh, b = read_png(b_path)
    resized = (aw, ah) != (bw, bh)
    if resized:
        b = resize_nearest(bw, bh, b, aw, ah)
    bpp = 4
    total = 0
    changed = 0
    grid_cols, grid_rows = 8, 6
    cell_sum = [0] * (grid_cols * grid_rows)
    cell_count = [0] * (grid_cols * grid_rows)
    heat = bytearray(aw * ah * bpp)

    for i in range(0, len(a), bpp):
        da = abs(a[i] - b[i])
        dg = abs(a[i + 1] - b[i + 1])
        db = abs(a[i + 2] - b[i + 2])
        d = da + dg + db
        total += d
        peak = max(da, dg, db)
        if peak > threshold:
            changed += 1
        pixel = i // bpp
        x = pixel % aw
        y = pixel // aw
        cell = min(grid_rows - 1, y * grid_rows // ah) * grid_cols + min(grid_cols - 1, x * grid_cols // aw)
        cell_sum[cell] += d
        cell_count[cell] += 1
        v = min(255, peak * 4)
        heat[i] = v if da >= dg and da >= db else v // 3
        heat[i + 1] = v if dg >= da and dg >= db else v // 3
        heat[i + 2] = v if db >= da and db >= dg else v // 3
        heat[i + 3] = 255

    pixels = aw * ah
    mae = total / (pixels * 3)
    regions = []
    for idx, s in enumerate(cell_sum):
        if cell_count[idx]:
            regions.append(
                {
                    "col": idx % grid_cols,
                    "row": idx // grid_cols,
                    "mae": round(s / (cell_count[idx] * 3), 3),
                }
            )
    regions.sort(key=lambda r: r["mae"], reverse=True)
    report = {
        "a": str(a_path),
        "b": str(b_path),
        "size": [aw, ah],
        "resized": resized,
        "mae": round(mae, 4),
        "changed_ratio": round(changed / pixels, 5),
        "threshold": threshold,
        "top_regions": regions[:10],
    }
    if out_dir:
        out = Path(out_dir)
        out.mkdir(parents=True, exist_ok=True)
        write_png(out / "diff.png", aw, ah, heat)
        (out / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    return report


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Pixel-diff two PNGs")
    parser.add_argument("a")
    parser.add_argument("b")
    parser.add_argument("--out-dir", default=None)
    parser.add_argument("--threshold", type=int, default=16)
    parser.add_argument("--max-mae", type=float, default=None, help="fail if MAE exceeds this")
    args = parser.parse_args()
    report = compare(args.a, args.b, args.out_dir, args.threshold)
    print(json.dumps(report, indent=2, ensure_ascii=False))
    if args.max_mae is not None and report["mae"] > args.max_mae:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
