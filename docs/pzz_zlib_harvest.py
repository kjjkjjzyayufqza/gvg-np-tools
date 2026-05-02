#!/usr/bin/env python3

from __future__ import annotations

import json
import math
import struct
import zlib
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent

KNOWN_MAGICS = [
    (b"OMG.00.1PSP", "GMO_model"),
    (b"MIG.00.1PSP", "GIM_texture"),
    (b"GMO\x00", "GMO_short"),
    (b"GIM\x00", "GIM_short"),
    (b"PMO\x00", "PMO_model"),
    (b"TIM2", "TIM2_texture"),
    (b"AFS\x00", "AFS_archive"),
    (b"MWo3", "MWo3"),
    (b"RIFF", "RIFF"),
    (b"VAGp", "VAG"),
]


def scan_magics(data: bytes) -> Dict[str, List[int]]:
    out: Dict[str, List[int]] = {}
    for pat, label in KNOWN_MAGICS:
        idx = data.find(pat)
        if idx >= 0:
            out[label] = [idx]
    return out


def ru32(data: bytes, off: int) -> int:
    return struct.unpack_from("<I", data, off)[0] if off + 4 <= len(data) else 0


def xor_dec(data: bytes, key: int) -> bytes:
    out = bytearray(len(data))
    kb = struct.pack("<I", key)
    for i in range(0, len(data) - 3, 4):
        out[i] = data[i] ^ kb[0]
        out[i + 1] = data[i + 1] ^ kb[1]
        out[i + 2] = data[i + 2] ^ kb[2]
        out[i + 3] = data[i + 3] ^ kb[3]
    for i in range((len(data) // 4) * 4, len(data)):
        out[i] = data[i] ^ kb[i % 4]
    return bytes(out)


def find_key_from_fc(raw: bytes, sz: int) -> Optional[int]:
    raw_w0 = ru32(raw, 0)
    for fc in range(2, 200):
        key = raw_w0 ^ fc
        dec_partial = xor_dec(raw[:min(sz, 0x4000)], key)
        d0 = ru32(dec_partial, 0)
        if d0 != fc:
            continue
        table_bytes = (1 + fc) * 4
        padding_end = min((table_bytes + 0x7FF) & ~0x7FF, sz)
        padding_ok = True
        for off in range(table_bytes, padding_end, 4):
            if off + 4 > len(dec_partial):
                break
            if ru32(dec_partial, off) != 0:
                padding_ok = False
                break
        if padding_ok:
            return key
    return None


def harvest_zlib_streams(dec: bytes, min_output: int = 16) -> List[Dict]:
    results = []
    zlib_headers = [(b"\x78\x9c", "default"), (b"\x78\x01", "no"), (b"\x78\xda", "best"), (b"\x78\x5e", "fast")]

    offsets = set()
    for hdr, _ in zlib_headers:
        start = 0
        while True:
            idx = dec.find(hdr, start)
            if idx < 0:
                break
            offsets.add(idx)
            start = idx + 1

    for off in sorted(offsets):
        try:
            dobj = zlib.decompressobj(wbits=15)
            out = dobj.decompress(dec[off:], 16 * 1024 * 1024)
            out += dobj.flush()
            if len(out) >= min_output:
                unused = len(dobj.unused_data)
                consumed = len(dec[off:]) - unused
                results.append({
                    "offset": off,
                    "compressed_size": consumed,
                    "decompressed_size": len(out),
                    "data": out,
                })
        except Exception:
            pass

    return results


def process_pzz(z_bin: Path, inv_entry: Dict, out_dir: Path) -> Dict:
    name = inv_entry.get("name", "")
    off = inv_entry["offset"]
    sz = inv_entry["size"]

    with z_bin.open("rb") as f:
        f.seek(off)
        raw = f.read(sz)

    key = find_key_from_fc(raw, sz)
    if key is None:
        return {"name": name, "size": sz, "error": "no_key"}

    dec = xor_dec(raw, key)
    fc = ru32(dec, 0)

    streams = harvest_zlib_streams(dec, min_output=16)
    if not streams:
        return {"name": name, "size": sz, "key_hex": hex(key), "fc": fc, "zlib_streams": 0}

    pzz_dir = out_dir / name.replace(".pzz", "")
    pzz_dir.mkdir(parents=True, exist_ok=True)

    blocks_info = []
    model_hits = []
    total = 0

    for i, s in enumerate(streams):
        data = s["data"]
        total += len(data)
        magics = scan_magics(data[:16384])
        ext = ".bin"
        for label in magics:
            if "GMO" in label: ext = ".gmo"
            elif "GIM" in label: ext = ".gim"
            elif "TIM2" in label: ext = ".tm2"
            elif "PMO" in label: ext = ".pmo"

        fname = pzz_dir / f"stream{i:03d}_off{s['offset']:06x}{ext}"
        fname.write_bytes(data)

        info = {
            "index": i,
            "offset": s["offset"],
            "compressed_size": s["compressed_size"],
            "decompressed_size": len(data),
            "head_hex": data[:32].hex(),
            "file": str(fname),
        }
        if magics:
            info["magics"] = magics
            model_hits.append(info)
        blocks_info.append(info)

    concat = bytearray()
    for s in streams:
        concat.extend(s["data"])
    if concat:
        concat_path = pzz_dir / f"_all_concat.bin"
        concat_path.write_bytes(bytes(concat))
        full_magics = scan_magics(bytes(concat[:65536]))
        if full_magics:
            model_hits.append({
                "source": "concatenated",
                "magics": full_magics,
                "size": len(concat),
                "file": str(concat_path),
            })

    return {
        "name": name,
        "size": sz,
        "key_hex": hex(key),
        "fc": fc,
        "zlib_streams": len(streams),
        "total_decompressed": total,
        "model_hits": model_hits,
        "blocks": blocks_info[:20],
    }


def main() -> int:
    z_bin = SCRIPT_DIR / "Z_DATA.BIN"
    z_inv = SCRIPT_DIR / "data_bin_inventory" / "Z_DATA.BIN.inventory.json"
    out_base = SCRIPT_DIR / "pzz_harvest_out"

    inv = json.loads(z_inv.read_text(encoding="utf-8"))
    entries = inv.get("entries", [])
    by_name = {e.get("name", "").lower(): e for e in entries}

    targets = [
        "pl00.pzz", "pl00l.pzz", "pl10.pzz", "pl41.pzz",
        "dm00.pzz", "basic.pzz", "logo.pzz", "title.pzz",
        "menu.pzz", "gallery.pzz",
    ]

    report: Dict[str, Any] = {"results": []}

    for target in targets:
        e = by_name.get(target.lower())
        if not e:
            continue
        print(f"Processing {target}...")
        result = process_pzz(z_bin, e, out_base)
        report["results"].append(result)

        ns = result.get("zlib_streams", 0)
        td = result.get("total_decompressed", 0)
        mh = result.get("model_hits", [])
        print(f"  key={result.get('key_hex','')} fc={result.get('fc','')} "
              f"streams={ns} decompressed={td} models={len(mh)}")
        for m in mh:
            print(f"    HIT: {m.get('magics',{})} ({m.get('size',m.get('decompressed_size','?'))} bytes)")

    out_path = SCRIPT_DIR / "pzz_harvest_report.json"
    out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\nWrote: {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
