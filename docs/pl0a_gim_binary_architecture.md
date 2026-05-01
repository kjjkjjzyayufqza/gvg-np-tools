# pl0a.gim Binary Architecture Notes

This note records the native binary layout observed in `E:\research\gvg_np\pl0a.gim`.
It is intentionally based on binary offsets, not on the current Rust implementation.

## File Summary

- File: `pl0a.gim`
- Size: `0x102D0` bytes (`66256`)
- Magic/header: `MIG.00.1PSP`, first `0x10` bytes
- Texture format: indexed 8-bit image with a 256-color palette
- Image dimensions: `256 x 256`
- Image pixel order: swizzled
- Palette format: `RGBA5551`

```text
000000: 4D 49 47 2E 30 30 2E 31 50 53 50 00 00 00 00 00
```

## Block Tree

All block integers are little-endian.

Each observed block header is:

```text
+0x00 u16 block_id
+0x02 u16 unknown_or_flags
+0x04 u32 block_size
+0x08 u32 block_size_mirror
+0x0C u32 data_offset
```

Observed structure:

```text
0x00010: block 0x02
  size        = 0x102C0
  data_offset = 0x10
  end         = 0x102D0

0x00020: block 0x03
  size        = 0x102B0
  data_offset = 0x10
  end         = 0x102D0

0x00030: block 0x04 image
  size        = 0x10050
  data_offset = 0x10
  data_base   = 0x00040
  end         = 0x10080

0x10080: block 0x05 palette
  size        = 0x250
  data_offset = 0x10
  data_base   = 0x10090
  end         = 0x102D0
```

Parent size relationships:

```text
block_03_size = 0x10 + image_block_size + palette_block_size
block_02_size = 0x10 + block_03_size
file_size     = 0x10 + block_02_size
```

For the original file:

```text
image_block_size   = 0x10050
palette_block_size = 0x250
block_03_size      = 0x10 + 0x10050 + 0x250 = 0x102B0
block_02_size      = 0x10 + 0x102B0 = 0x102C0
file_size          = 0x10 + 0x102C0 = 0x102D0
```

## Image Block 0x04

Image block header:

```text
0x00030 u16 block_id           = 0x0004
0x00032 u16 unknown_or_flags   = 0x0000
0x00034 u32 block_size         = 0x00010050
0x00038 u32 block_size_mirror  = 0x00010050
0x0003C u32 data_offset        = 0x00000010
```

Image block data starts at `0x00040`.

Important fields:

```text
0x00040 u32 info_size      = 0x00000030
0x00044 u16 pixel_format   = 0x0005       ; Indexed8
0x00046 u16 pixel_order    = 0x0001       ; swizzled
0x00048 u16 width          = 0x0100       ; 256
0x0004A u16 height         = 0x0100       ; 256
0x0004C u16 bpp            = 0x0008       ; 8 bits per pixel
0x0004E u16 swizzle_width  = 0x0010       ; likely 16-byte swizzle block width
0x00050 u16 swizzle_height = 0x0008       ; likely 8-row swizzle block height
0x00058 u32 info_offset    = 0x00000030
0x0005C u32 pixels_offset  = 0x00000040
0x00060 u32 data_size      = 0x00010040   ; 0x40 metadata + 0x10000 pixels
```

Image pixel bytes:

```text
pixel_start = image_data_base + pixels_offset
            = 0x00040 + 0x40
            = 0x00080

pixel_size  = width * height * bpp / 8
            = 256 * 256 * 8 / 8
            = 0x10000

pixel_end   = 0x00080 + 0x10000
            = 0x10080
```

Since `pixel_order = 1`, these bytes are swizzled. For this file's Indexed8 format,
one pixel is one byte.

## Palette Block 0x05

Palette block header:

```text
0x10080 u16 block_id           = 0x0005
0x10082 u16 unknown_or_flags   = 0x0000
0x10084 u32 block_size         = 0x00000250
0x10088 u32 block_size_mirror  = 0x00000250
0x1008C u32 data_offset        = 0x00000010
```

Palette block data starts at `0x10090`.

Important fields:

```text
0x10090 u32 info_size      = 0x00000030
0x10094 u16 pixel_format   = 0x0001       ; RGBA5551
0x10096 u16 pixel_order    = 0x0000       ; linear
0x10098 u16 width          = 0x0100       ; 256 palette entries
0x1009A u16 height         = 0x0001
0x1009C u16 bpp            = 0x0010       ; 16 bits per palette entry
0x100AC u32 pixels_offset  = 0x00000040
0x100B0 u32 data_size      = 0x00000240   ; 0x40 metadata + 0x200 palette bytes
```

Palette bytes:

```text
palette_start = 0x10090 + 0x40 = 0x100D0
palette_size  = 256 * 2 = 0x200
palette_end   = 0x100D0 + 0x200 = 0x102D0
```

Each palette entry is `RGBA5551`.

## Replacing With A Different Resolution PNG

For `pl0a.gim`, the safest compatible strategy is:

- Keep main image format as `Indexed8`.
- Keep main image swizzled (`pixel_order = 1`).
- Keep palette as 256 entries of `RGBA5551`.
- Quantize/remap the input PNG to at most 256 colors.
- Encode the index image into swizzled bytes.
- Regenerate the 256-entry RGBA5551 palette.

Do not simply replace pixel bytes or only patch width/height. A larger image moves the palette
block and changes parent block sizes.

Let:

```text
W = new width
H = new height
new_pixel_size       = W * H
new_image_data_size  = 0x40 + new_pixel_size
new_image_block_size = 0x10 + new_image_data_size
                     = 0x50 + W * H
new_palette_size     = 0x250    ; if keeping 256 RGBA5551 palette entries
new_palette_offset   = 0x00030 + new_image_block_size
new_block_03_size    = 0x10 + new_image_block_size + new_palette_size
new_block_02_size    = 0x10 + new_block_03_size
new_file_size        = 0x10 + new_block_02_size
```

Required binary updates:

```text
0x00014 u32 block_02_size
0x00018 u32 block_02_size mirror

0x00024 u32 block_03_size
0x00028 u32 block_03_size mirror

0x00034 u32 image block size
0x00038 u32 image block size mirror

0x00048 u16 image width
0x0004A u16 image height
0x00060 u32 image data size
```

Then write:

```text
0x00080 .. 0x00080 + new_pixel_size
```

with the new swizzled Indexed8 pixel data.

The palette block must be moved to:

```text
new_palette_offset = 0x00030 + new_image_block_size
```

and the palette block bytes should be regenerated or copied with a regenerated palette:

```text
new_palette_offset + 0x00 u16 block_id          = 0x0005
new_palette_offset + 0x04 u32 block_size        = 0x250
new_palette_offset + 0x08 u32 block_size_mirror = 0x250
new_palette_offset + 0x0C u32 data_offset       = 0x10
new_palette_offset + 0x10 ... palette metadata
new_palette_offset + 0x50 ... 0x200 bytes RGBA5551 palette
```

## Worked Example: 512 x 512

For a `512 x 512` replacement, keeping Indexed8 + 256-entry RGBA5551 palette:

```text
W = 512
H = 512

new_pixel_size       = 0x40000
new_image_data_size  = 0x40040
new_image_block_size = 0x40050
new_palette_offset   = 0x40080
new_block_03_size    = 0x402B0
new_block_02_size    = 0x402C0
new_file_size        = 0x402D0
```

Fields to write:

```text
0x00014 = C0 02 04 00
0x00018 = C0 02 04 00

0x00024 = B0 02 04 00
0x00028 = B0 02 04 00

0x00034 = 50 00 04 00
0x00038 = 50 00 04 00

0x00048 = 00 02
0x0004A = 00 02

0x00060 = 40 00 04 00
```

Move/regenerate the palette block:

```text
old palette: 0x10080 .. 0x102D0
new palette: 0x40080 .. 0x402D0
```

## Swizzle Constraints

Observed swizzle metadata:

```text
swizzle_width  = 0x10
swizzle_height = 0x08
```

For Indexed8, row bytes equal image width. To stay compatible with the observed swizzle shape:

- Width should be a multiple of `16`.
- Height should be a multiple of `8`.
- Common safe sizes are `128`, `256`, `512`, etc.

## Indexed8 Palette Constraints

Because `pl0a.gim` is `Indexed8`, arbitrary PNG RGBA pixels cannot be inserted directly.

Required conversion:

1. Quantize PNG to at most 256 colors.
2. Convert each pixel to an 8-bit palette index.
3. Encode index pixels with the GIM swizzle layout.
4. Encode the palette as 256 RGBA5551 entries.

RGBA5551 limitations:

- Red: 5 bits
- Green: 5 bits
- Blue: 5 bits
- Alpha: 1 bit

Semi-transparent PNG pixels will lose alpha precision.

## Outer Containers

If this GIM is inside a PZZ stream, increasing the GIM size also requires updating the outer
containers:

### PZZ stream chunk

- Chunk `comp_len` (`u32`, big-endian)
- Chunk `raw_len` (`u32`, big-endian), equal to the new GIM size
- Zlib-compressed stream bytes
- Chunk padding to 128-byte units
- PZZ descriptor lower 30 bits: `padded_chunk_size / 128`
- Descriptor high flags should keep the stream flag, usually `0x40000000`

### PZZ archive

- PZZ body size may change.
- XOR key may change because key derivation depends on body size.
- If the PZZ has a 16-byte tail, recompute it over the decrypted body.

### AFS entry

- AFS entry size becomes the new PZZ size.
- Entry data remains 2048-byte aligned.
- Later AFS entries shift if aligned size changes.
- Name-table size mirror, if present, must be updated.

### CWCheat body size patch

The EBOOT ROM table body size override must match the final PZZ body size expected by the game.
For the current modding workflow, regenerate the CWCheat body-size patch after changing the GIM.

## Implementation Recommendation

Support for arbitrary PNG resolution should not be implemented as an in-place pixel overwrite.
It should be implemented as a GIM rebuild for this layout:

1. Parse block tree and confirm it matches `0x02 -> 0x03 -> image 0x04 + palette 0x05`.
2. Decode/quantize PNG to Indexed8.
3. Build new image block with updated width, height, data size, and swizzled pixel bytes.
4. Build new palette block with 256 RGBA5551 colors.
5. Recompute block 0x03 and 0x02 sizes.
6. Write the new GIM file bytes.
7. Let the existing PZZ/AFS save pipeline rebuild outer archive sizes and body-size patch data.

