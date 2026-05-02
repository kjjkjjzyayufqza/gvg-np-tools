import struct, json
from pathlib import Path
from gvg_converter import extract_pzz_streams, classify_stream, parse_pmf2_sections, parse_pmf2

inv = json.loads(Path("data_bin_inventory/Z_DATA.BIN.inventory.json").read_text(encoding="utf-8"))
target = next(e for e in inv["entries"] if e.get("name","").lower() == "pl00.pzz")

with open("Z_DATA.BIN", "rb") as f:
    f.seek(target["offset"])
    pzz_data = f.read(target["size"])

streams = extract_pzz_streams(pzz_data)
for i, s in enumerate(streams):
    ct = classify_stream(s)
    if ct == "pmf2":
        print(f"\n=== Stream {i} ({ct}, {len(s)} bytes) ===")
        data = s
        nsec = struct.unpack_from("<I", data, 4)[0]
        hdr_size = struct.unpack_from("<I", data, 8)[0]
        bbox = [struct.unpack_from("<f", data, 0x10 + j*4)[0] for j in range(3)]
        print(f"  sections={nsec}, hdr_size=0x{hdr_size:X}, bbox={bbox}")

        print(f"  Header bytes 0x00-0x20:")
        for off in range(0, 0x20, 4):
            v = struct.unpack_from("<I", data, off)[0]
            fv = struct.unpack_from("<f", data, off)[0]
            print(f"    0x{off:02X}: 0x{v:08X}  ({fv:.6f})")

        secs, _ = parse_pmf2_sections(data)
        mesh_secs = [s for s in secs if s.has_mesh]
        no_mesh = [s for s in secs if not s.has_mesh]
        print(f"  Total bones: {len(secs)}")
        print(f"  Bones WITH mesh: {len(mesh_secs)}")
        print(f"  Bones WITHOUT mesh: {len(no_mesh)}")

        categories = {}
        for s in mesh_secs:
            cat = s.category or "other"
            categories.setdefault(cat, []).append(s.name)

        print(f"  Mesh categories:")
        for cat, names in sorted(categories.items()):
            print(f"    {cat}: {len(names)} parts -> {names[:8]}{'...' if len(names)>8 else ''}")

        model = parse_pmf2(data)
        print(f"  GE regions: {len(model.ge_cmd_regions)}")
        print(f"  Draw calls: {len(model.draw_calls)}")
        print(f"  Model names found: {model.model_names[:10]}")

        print(f"\n  Bone hierarchy (roots):")
        for s in secs:
            if s.parent < 0:
                print(f"    ROOT: [{s.index}] '{s.name}' has_mesh={s.has_mesh}")
