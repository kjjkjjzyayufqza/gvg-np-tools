from pathlib import Path
import hashlib
import struct
import math
import collections


def u32(d, o):
    return struct.unpack_from("<I", d, o)[0]


def main():
    paths = sorted(Path("converted_out/W_pl00ov").glob("pl00ov*.bin"))
    data = [p.read_bytes() for p in paths]
    for p, d in zip(paths, data):
        print()
        print(p.name, "size", len(d), "sha1", hashlib.sha1(d).hexdigest())
        print("head64", d[:64].hex(" "))
        print("u32", [hex(u32(d, i)) for i in range(0, 64, 4)])
        ascii_head = bytes(c if 32 <= c < 127 else 46 for c in d[:128])
        print("ascii", ascii_head.decode("ascii"))
        ptrs = []
        for off in range(0, min(len(d), 0x100), 4):
            v = u32(d, off)
            if 0 <= v < len(d):
                ptrs.append((off, v))
            if 0x08800000 <= v <= 0x09FFFFFF:
                ptrs.append((off, hex(v)))
        print("ptr_like", ptrs[:32])
        s1 = u32(d, 12)
        s2 = u32(d, 16)
        print("sections", hex(0x40), hex(0x40 + s1), hex(0x40 + s1 + s2))
        for label, a, b in (
            ("header", 0, 0x40),
            ("section1", 0x40, 0x40 + s1),
            ("section2", 0x40 + s1, 0x40 + s1 + s2),
        ):
            chunk = d[a:b]
            cnt = collections.Counter(chunk)
            ent = 0.0
            for c in cnt.values():
                p0 = c / len(chunk)
                ent -= p0 * math.log2(p0)
            print(label, hex(a), hex(b), "entropy", round(ent, 3), "zero", chunk.count(0))
        for needle in (b"MIG.", b".GIM", b"PMF2", b"SAD ", b"MWo3"):
            offs = []
            start = 0
            while True:
                pos = d.find(needle, start)
                if pos < 0:
                    break
                offs.append(pos)
                start = pos + 1
            if offs:
                print("found", needle, [hex(x) for x in offs])
        base = u32(d, 8)
        local_ptrs = []
        for off in range(0, len(d) - 3, 4):
            v = u32(d, off)
            if base <= v < base + len(d):
                local_ptrs.append((off, v - base))
        print("local_ptrs_count", len(local_ptrs))
        print("local_ptrs_first", [(hex(a), hex(b)) for a, b in local_ptrs[:40]])
        print("nonzero_u32_first_section1")
        shown = 0
        for off in range(0x40, min(0x300, len(d)), 4):
            v = u32(d, off)
            if v:
                print(hex(off), hex(v))
                shown += 1
                if shown >= 48:
                    break
        print("section2_head_u32", [hex(u32(d, off)) for off in range(0x5C78, 0x5C78 + 0x80, 4)])

    size = len(data[0])
    diff = []
    for i in range(size):
        if len({d[i] for d in data}) != 1:
            diff.append(i)
    ranges = []
    if diff:
        s = prev = diff[0]
        for x in diff[1:]:
            if x == prev + 1:
                prev = x
            else:
                ranges.append((s, prev))
                s = prev = x
        ranges.append((s, prev))
    print()
    print("same_sizes", [len(d) for d in data])
    print("diff_bytes", len(diff))
    print("diff_ranges")
    for a, b in ranges[:80]:
        print(hex(a), hex(b), b - a + 1)


if __name__ == "__main__":
    main()
