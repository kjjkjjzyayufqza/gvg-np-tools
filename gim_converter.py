#!/usr/bin/env python3
import struct
from pathlib import Path
from typing import Optional, Tuple, List

try:
    from PIL import Image
except ImportError:
    Image = None


def ru16(d, o): return struct.unpack_from('<H', d, o)[0]
def ru32(d, o): return struct.unpack_from('<I', d, o)[0]


def unswizzle(data: bytes, width: int, height: int, bpp: int) -> bytearray:
    row_bytes = width * bpp // 8
    out = bytearray(len(data))

    block_w = 16
    block_h = 8
    blocks_per_row = row_bytes // block_w
    if blocks_per_row == 0:
        return bytearray(data)

    for by in range(0, height, block_h):
        for bx in range(blocks_per_row):
            block_idx = (by // block_h) * blocks_per_row + bx
            src_base = block_idx * block_w * block_h
            for iy in range(block_h):
                dst_row = by + iy
                if dst_row >= height:
                    break
                src_off = src_base + iy * block_w
                dst_off = dst_row * row_bytes + bx * block_w
                if src_off + block_w <= len(data) and dst_off + block_w <= len(out):
                    out[dst_off:dst_off + block_w] = data[src_off:src_off + block_w]

    return out


def decode_rgba5650(data: bytes, width: int, height: int) -> List[Tuple[int,int,int,int]]:
    pixels = []
    for i in range(width * height):
        c = ru16(data, i * 2)
        r = ((c & 0x1F) * 255 + 15) // 31
        g = (((c >> 5) & 0x3F) * 255 + 31) // 63
        b = (((c >> 11) & 0x1F) * 255 + 15) // 31
        pixels.append((r, g, b, 255))
    return pixels


def decode_rgba5551(data: bytes, width: int, height: int) -> List[Tuple[int,int,int,int]]:
    pixels = []
    for i in range(width * height):
        c = ru16(data, i * 2)
        r = ((c & 0x1F) * 255 + 15) // 31
        g = (((c >> 5) & 0x1F) * 255 + 15) // 31
        b = (((c >> 10) & 0x1F) * 255 + 15) // 31
        a = 255 if (c >> 15) & 1 else 0
        pixels.append((r, g, b, a))
    return pixels


def decode_rgba4444(data: bytes, width: int, height: int) -> List[Tuple[int,int,int,int]]:
    pixels = []
    for i in range(width * height):
        c = ru16(data, i * 2)
        r = ((c & 0xF) * 255 + 7) // 15
        g = (((c >> 4) & 0xF) * 255 + 7) // 15
        b = (((c >> 8) & 0xF) * 255 + 7) // 15
        a = (((c >> 12) & 0xF) * 255 + 7) // 15
        pixels.append((r, g, b, a))
    return pixels


def decode_rgba8888(data: bytes, width: int, height: int) -> List[Tuple[int,int,int,int]]:
    pixels = []
    for i in range(width * height):
        off = i * 4
        r, g, b, a = data[off], data[off+1], data[off+2], data[off+3]
        pixels.append((r, g, b, a))
    return pixels


PIXEL_DECODERS = {
    0x00: (decode_rgba5650, 16),
    0x01: (decode_rgba5551, 16),
    0x02: (decode_rgba4444, 16),
    0x03: (decode_rgba8888, 32),
}


def parse_gim_block(data: bytes, offset: int):
    if offset + 16 > len(data):
        return None
    block_id = ru16(data, offset)
    block_size = ru32(data, offset + 4)
    next_off = ru32(data, offset + 8)
    data_off = ru32(data, offset + 12)
    return {
        'id': block_id,
        'offset': offset,
        'size': block_size,
        'next': next_off,
        'data_offset': data_off,
    }


def find_blocks(data: bytes, start: int, end: int) -> list:
    blocks = []
    off = start
    while off + 16 <= end:
        blk = parse_gim_block(data, off)
        if blk is None:
            break
        blocks.append(blk)
        next_child = off + blk['data_offset']
        child_end = off + blk['size']

        if blk['id'] in (0x02, 0x03):
            children = find_blocks(data, next_child, child_end)
            blk['children'] = children
        else:
            blk['children'] = []

        off += blk['size']

    return blocks


def parse_image_block_data(data: bytes, block_offset: int, data_rel_offset: int):
    abs_data = block_offset + data_rel_offset
    if abs_data + 0x30 > len(data):
        return None

    d = abs_data
    img_format = ru16(data, d + 0x04)
    pixel_order = ru16(data, d + 0x06)
    width = ru16(data, d + 0x08)
    height = ru16(data, d + 0x0A)
    pixels_start = ru32(data, d + 0x1C)

    pixel_abs = d + pixels_start

    return {
        'format': img_format,
        'pixel_order': pixel_order,
        'width': width,
        'height': height,
        'pixels_offset': pixel_abs,
    }


def gim_to_png(gim_data: bytes, out_path: Path) -> bool:
    if Image is None:
        print("Pillow not installed, cannot convert GIM to PNG")
        return False

    if len(gim_data) < 16:
        return False

    is_le = gim_data[:4] == b'MIG.'
    is_be = gim_data[:4] == b'.GIM'
    if not is_le and not is_be:
        return False

    blocks = find_blocks(gim_data, 0x10, len(gim_data))

    image_info = None
    palette_info = None

    def search_blocks(blks):
        nonlocal image_info, palette_info
        for b in blks:
            if b['id'] == 0x05 and palette_info is None:
                palette_info = parse_image_block_data(gim_data, b['offset'], b['data_offset'])
            elif b['id'] == 0x04 and image_info is None:
                image_info = parse_image_block_data(gim_data, b['offset'], b['data_offset'])
            if 'children' in b:
                search_blocks(b['children'])

    search_blocks(blocks)

    if image_info is None:
        return False

    fmt = image_info['format']
    w = image_info['width']
    h = image_info['height']
    pix_off = image_info['pixels_offset']
    swizzled = image_info['pixel_order'] == 1

    if fmt in (0x04, 0x05):
        palette_colors = []
        if palette_info:
            pal_fmt = palette_info['format']
            pal_w = palette_info['width']
            pal_h = palette_info['height']
            pal_off = palette_info['pixels_offset']

            if pal_fmt in PIXEL_DECODERS:
                dec, bpp = PIXEL_DECODERS[pal_fmt]
                pal_size = pal_w * pal_h * bpp // 8
                if pal_off + pal_size <= len(gim_data):
                    pal_data = gim_data[pal_off:pal_off + pal_size]
                    palette_colors = dec(pal_data, pal_w, pal_h)

        if not palette_colors:
            palette_colors = [(i, i, i, 255) for i in range(256)]

        if fmt == 0x04:
            bpp = 4
            raw_size = w * h // 2
        else:
            bpp = 8
            raw_size = w * h

        if pix_off + raw_size > len(gim_data):
            return False

        raw_data = bytearray(gim_data[pix_off:pix_off + raw_size])

        if swizzled:
            raw_data = unswizzle(bytes(raw_data), w, h, bpp)

        pixels = []
        if fmt == 0x04:
            for i in range(w * h):
                byte_idx = i // 2
                if byte_idx >= len(raw_data):
                    break
                if i % 2 == 0:
                    idx = raw_data[byte_idx] & 0x0F
                else:
                    idx = (raw_data[byte_idx] >> 4) & 0x0F
                if idx < len(palette_colors):
                    pixels.append(palette_colors[idx])
                else:
                    pixels.append((0, 0, 0, 255))
        else:
            for i in range(w * h):
                if i >= len(raw_data):
                    break
                idx = raw_data[i]
                if idx < len(palette_colors):
                    pixels.append(palette_colors[idx])
                else:
                    pixels.append((0, 0, 0, 255))

    elif fmt in PIXEL_DECODERS:
        dec, bpp = PIXEL_DECODERS[fmt]
        raw_size = w * h * bpp // 8
        if pix_off + raw_size > len(gim_data):
            return False

        raw_data = gim_data[pix_off:pix_off + raw_size]
        if swizzled:
            raw_data = unswizzle(bytes(raw_data), w, h, bpp)

        pixels = dec(raw_data, w, h)
    else:
        print(f"  Unsupported GIM format: 0x{fmt:02X}")
        return False

    img = Image.new('RGBA', (w, h))
    img.putdata(pixels[:w*h])
    out_path.parent.mkdir(parents=True, exist_ok=True)
    img.save(str(out_path))
    return True


def convert_gim_file(gim_path: Path, png_path: Optional[Path] = None) -> bool:
    gim_data = gim_path.read_bytes()
    if png_path is None:
        png_path = gim_path.with_suffix('.png')
    return gim_to_png(gim_data, png_path)


if __name__ == "__main__":
    import sys

    if len(sys.argv) < 2:
        print("Usage: gim_converter.py <gim_file> [output.png]")
        sys.exit(1)

    gim_path = Path(sys.argv[1])
    png_path = Path(sys.argv[2]) if len(sys.argv) > 2 else None

    if convert_gim_file(gim_path, png_path):
        print(f"Converted: {gim_path} -> {png_path or gim_path.with_suffix('.png')}")
    else:
        print(f"Failed to convert: {gim_path}")
        sys.exit(1)
