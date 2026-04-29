use anyhow::{bail, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba5650,
    Rgba5551,
    Rgba4444,
    Rgba8888,
    Indexed4,
    Indexed8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GimMetadata {
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
    pub swizzled: bool,
    pub image_offset: usize,
    pub image_size: usize,
}

#[derive(Clone, Debug)]
pub struct GimImage {
    pub metadata: GimMetadata,
    pub rgba: Vec<[u8; 4]>,
    original: Vec<u8>,
}

#[derive(Clone, Debug)]
struct GimBlock {
    id: u16,
    offset: usize,
    data_offset: usize,
    children: Vec<GimBlock>,
}

#[derive(Clone, Debug)]
struct ImageInfo {
    format: PixelFormat,
    pixel_order: u16,
    width: usize,
    height: usize,
    pixels_offset: usize,
}

impl GimImage {
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 0x10 {
            bail!("GIM data is too small");
        }
        if &data[0..4] != b"MIG." {
            bail!("unsupported GIM magic");
        }
        let blocks = find_blocks(data, 0x10, data.len())?;
        let image_info = find_image_info(data, &blocks, 0x04)?
            .ok_or_else(|| anyhow::anyhow!("GIM image block not found"))?;
        let palette_info = find_image_info(data, &blocks, 0x05)?;
        let swizzled = pixel_order_is_swizzled(image_info.pixel_order)?;
        let (rgba, image_size) = decode_pixels(data, &image_info, palette_info.as_ref())?;
        Ok(Self {
            metadata: GimMetadata {
                width: image_info.width,
                height: image_info.height,
                format: image_info.format,
                swizzled,
                image_offset: image_info.pixels_offset,
                image_size,
            },
            rgba,
            original: data.to_vec(),
        })
    }

    pub fn replace_rgba(&self, pixels: &[[u8; 4]]) -> Result<Vec<u8>> {
        let expected = self.metadata.width * self.metadata.height;
        if pixels.len() != expected {
            bail!(
                "replacement pixel count {} does not match GIM dimensions {}x{}",
                pixels.len(),
                self.metadata.width,
                self.metadata.height
            );
        }
        let mut encoded = match self.metadata.format {
            PixelFormat::Rgba5650 => encode_rgba5650(pixels),
            PixelFormat::Rgba5551 => encode_rgba5551(pixels),
            PixelFormat::Rgba4444 => encode_rgba4444(pixels),
            PixelFormat::Rgba8888 => encode_rgba8888(pixels),
            PixelFormat::Indexed4 | PixelFormat::Indexed8 => {
                bail!("PNG replacement for indexed GIM textures requires palette remapping")
            }
        };
        if self.metadata.swizzled {
            encoded = swizzle(
                &encoded,
                self.metadata.width,
                self.metadata.height,
                bits_per_pixel(self.metadata.format)?,
            )?;
        }
        if encoded.len() != self.metadata.image_size {
            bail!(
                "encoded replacement size {} does not match original image size {}",
                encoded.len(),
                self.metadata.image_size
            );
        }
        let mut out = self.original.clone();
        let start = self.metadata.image_offset;
        let end = start + encoded.len();
        if end > out.len() {
            bail!("GIM image data exceeds file size");
        }
        out[start..end].copy_from_slice(&encoded);
        Ok(out)
    }

    pub fn replace_png_bytes(&self, png: &[u8]) -> Result<Vec<u8>> {
        let image = image::load_from_memory(png)?.to_rgba8();
        if image.width() as usize != self.metadata.width
            || image.height() as usize != self.metadata.height
        {
            bail!(
                "PNG dimensions {}x{} do not match GIM dimensions {}x{}",
                image.width(),
                image.height(),
                self.metadata.width,
                self.metadata.height
            );
        }
        let pixels = image
            .pixels()
            .map(|p| [p.0[0], p.0[1], p.0[2], p.0[3]])
            .collect::<Vec<_>>();
        self.replace_rgba(&pixels)
    }
}

fn decode_pixels(
    data: &[u8],
    image: &ImageInfo,
    palette: Option<&ImageInfo>,
) -> Result<(Vec<[u8; 4]>, usize)> {
    match image.format {
        PixelFormat::Indexed4 | PixelFormat::Indexed8 => {
            decode_indexed_pixels(data, image, palette)
        }
        format => {
            let bpp = bits_per_pixel(format)?;
            let size = image.width * image.height * bpp / 8;
            let raw = image_bytes(data, image.pixels_offset, size)?;
            let linear = if pixel_order_is_swizzled(image.pixel_order)? {
                unswizzle(raw, image.width, image.height, bpp)?
            } else {
                raw.to_vec()
            };
            let rgba = match format {
                PixelFormat::Rgba5650 => decode_rgba5650(&linear, image.width, image.height),
                PixelFormat::Rgba5551 => decode_rgba5551(&linear, image.width, image.height),
                PixelFormat::Rgba4444 => decode_rgba4444(&linear, image.width, image.height),
                PixelFormat::Rgba8888 => decode_rgba8888(&linear, image.width, image.height),
                PixelFormat::Indexed4 | PixelFormat::Indexed8 => unreachable!(),
            };
            Ok((rgba, size))
        }
    }
}

fn decode_indexed_pixels(
    data: &[u8],
    image: &ImageInfo,
    palette: Option<&ImageInfo>,
) -> Result<(Vec<[u8; 4]>, usize)> {
    let palette = palette.ok_or_else(|| anyhow::anyhow!("indexed GIM is missing palette block"))?;
    let (palette_pixels, _) = decode_pixels(data, palette, None)?;
    let bpp = bits_per_pixel(image.format)?;
    let size = image.width * image.height * bpp / 8;
    let raw = image_bytes(data, image.pixels_offset, size)?;
    let linear = if pixel_order_is_swizzled(image.pixel_order)? {
        unswizzle(raw, image.width, image.height, bpp)?
    } else {
        raw.to_vec()
    };
    let mut rgba = Vec::with_capacity(image.width * image.height);
    match image.format {
        PixelFormat::Indexed4 => {
            for index in 0..(image.width * image.height) {
                let packed = linear[index / 2];
                let palette_index = if index % 2 == 0 {
                    packed & 0x0F
                } else {
                    (packed >> 4) & 0x0F
                } as usize;
                let color = palette_pixels.get(palette_index).ok_or_else(|| {
                    anyhow::anyhow!("GIM palette index {} is out of range", palette_index)
                })?;
                rgba.push(*color);
            }
        }
        PixelFormat::Indexed8 => {
            for palette_index in linear.iter().take(image.width * image.height) {
                let color = palette_pixels.get(*palette_index as usize).ok_or_else(|| {
                    anyhow::anyhow!("GIM palette index {} is out of range", palette_index)
                })?;
                rgba.push(*color);
            }
        }
        _ => unreachable!(),
    }
    Ok((rgba, size))
}

fn image_bytes(data: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    if offset.checked_add(size).is_none_or(|end| end > data.len()) {
        bail!("GIM image data exceeds file size");
    }
    Ok(&data[offset..offset + size])
}

fn find_image_info(data: &[u8], blocks: &[GimBlock], id: u16) -> Result<Option<ImageInfo>> {
    for block in blocks {
        if block.id == id {
            return Ok(Some(parse_image_block_data(data, block)?));
        }
        if let Some(info) = find_image_info(data, &block.children, id)? {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

fn parse_image_block_data(data: &[u8], block: &GimBlock) -> Result<ImageInfo> {
    let base = block.offset + block.data_offset;
    if base + 0x30 > data.len() {
        bail!("GIM image block data exceeds file size");
    }
    let format = pixel_format(ru16(data, base + 0x04)?)?;
    let pixel_order = ru16(data, base + 0x06)?;
    let width = ru16(data, base + 0x08)? as usize;
    let height = ru16(data, base + 0x0A)? as usize;
    let pixels_offset = base + ru32(data, base + 0x1C)? as usize;
    if width == 0 || height == 0 {
        bail!("GIM image dimensions are zero");
    }
    Ok(ImageInfo {
        format,
        pixel_order,
        width,
        height,
        pixels_offset,
    })
}

fn find_blocks(data: &[u8], start: usize, end: usize) -> Result<Vec<GimBlock>> {
    let mut blocks = Vec::new();
    let mut offset = start;
    while offset + 16 <= end {
        let id = ru16(data, offset)?;
        let size = ru32(data, offset + 4)? as usize;
        let data_offset = ru32(data, offset + 12)? as usize;
        if size == 0 {
            bail!("GIM block has zero size");
        }
        let block_end = offset + size;
        if block_end > data.len() || block_end > end {
            bail!("GIM block exceeds parent bounds");
        }
        let children = if id == 0x02 || id == 0x03 {
            let child_start = offset + data_offset;
            if child_start <= offset || child_start >= block_end {
                bail!("invalid GIM child block offset");
            }
            find_blocks(data, child_start, block_end)?
        } else {
            Vec::new()
        };
        blocks.push(GimBlock {
            id,
            offset,
            data_offset,
            children,
        });
        offset += size;
    }
    Ok(blocks)
}

fn pixel_format(format: u16) -> Result<PixelFormat> {
    match format {
        0x00 => Ok(PixelFormat::Rgba5650),
        0x01 => Ok(PixelFormat::Rgba5551),
        0x02 => Ok(PixelFormat::Rgba4444),
        0x03 => Ok(PixelFormat::Rgba8888),
        0x04 => Ok(PixelFormat::Indexed4),
        0x05 => Ok(PixelFormat::Indexed8),
        _ => bail!("unsupported GIM pixel format 0x{format:02X}"),
    }
}

fn bits_per_pixel(format: PixelFormat) -> Result<usize> {
    match format {
        PixelFormat::Indexed4 => Ok(4),
        PixelFormat::Indexed8 => Ok(8),
        PixelFormat::Rgba5650 | PixelFormat::Rgba5551 | PixelFormat::Rgba4444 => Ok(16),
        PixelFormat::Rgba8888 => Ok(32),
    }
}

fn pixel_order_is_swizzled(pixel_order: u16) -> Result<bool> {
    match pixel_order {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("unsupported GIM pixel order {}", pixel_order),
    }
}

fn decode_rgba5650(data: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|i| {
            let c = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
            [
                scale_bits((c & 0x1F) as u32, 31),
                scale_bits(((c >> 5) & 0x3F) as u32, 63),
                scale_bits(((c >> 11) & 0x1F) as u32, 31),
                255,
            ]
        })
        .collect()
}

fn decode_rgba5551(data: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|i| {
            let c = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
            [
                scale_bits((c & 0x1F) as u32, 31),
                scale_bits(((c >> 5) & 0x1F) as u32, 31),
                scale_bits(((c >> 10) & 0x1F) as u32, 31),
                if c & 0x8000 != 0 { 255 } else { 0 },
            ]
        })
        .collect()
}

fn decode_rgba4444(data: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|i| {
            let c = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]);
            [
                scale_bits((c & 0xF) as u32, 15),
                scale_bits(((c >> 4) & 0xF) as u32, 15),
                scale_bits(((c >> 8) & 0xF) as u32, 15),
                scale_bits(((c >> 12) & 0xF) as u32, 15),
            ]
        })
        .collect()
}

fn decode_rgba8888(data: &[u8], width: usize, height: usize) -> Vec<[u8; 4]> {
    (0..width * height)
        .map(|i| {
            let offset = i * 4;
            [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]
        })
        .collect()
}

fn encode_rgba5650(pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 2);
    for [r, g, b, _] in pixels {
        let value = ((*r as u16 * 31 / 255) & 0x1F)
            | (((*g as u16 * 63 / 255) & 0x3F) << 5)
            | (((*b as u16 * 31 / 255) & 0x1F) << 11);
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn encode_rgba5551(pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 2);
    for [r, g, b, a] in pixels {
        let value = ((*r as u16 * 31 / 255) & 0x1F)
            | (((*g as u16 * 31 / 255) & 0x1F) << 5)
            | (((*b as u16 * 31 / 255) & 0x1F) << 10)
            | (if *a >= 128 { 1 << 15 } else { 0 });
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn encode_rgba4444(pixels: &[[u8; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 2);
    for [r, g, b, a] in pixels {
        let value = ((*r as u16 * 15 / 255) & 0xF)
            | (((*g as u16 * 15 / 255) & 0xF) << 4)
            | (((*b as u16 * 15 / 255) & 0xF) << 8)
            | (((*a as u16 * 15 / 255) & 0xF) << 12);
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn encode_rgba8888(pixels: &[[u8; 4]]) -> Vec<u8> {
    pixels.iter().flat_map(|p| *p).collect()
}

fn scale_bits(value: u32, max: u32) -> u8 {
    ((value * 255 + max / 2) / max) as u8
}

fn unswizzle(data: &[u8], width: usize, height: usize, bpp: usize) -> Result<Vec<u8>> {
    let row_bytes = width * bpp / 8;
    let mut out = vec![0u8; data.len()];
    let block_w = 16;
    let block_h = 8;
    let blocks_per_row = row_bytes / block_w;
    if blocks_per_row == 0 || !row_bytes.is_multiple_of(block_w) {
        bail!("unsupported swizzled GIM row width {}", row_bytes);
    }
    for by in (0..height).step_by(block_h) {
        for bx in 0..blocks_per_row {
            let block_index = (by / block_h) * blocks_per_row + bx;
            let src_base = block_index * block_w * block_h;
            for iy in 0..block_h {
                let dst_row = by + iy;
                if dst_row >= height {
                    break;
                }
                let src = src_base + iy * block_w;
                let dst = dst_row * row_bytes + bx * block_w;
                if src + block_w <= data.len() && dst + block_w <= out.len() {
                    out[dst..dst + block_w].copy_from_slice(&data[src..src + block_w]);
                }
            }
        }
    }
    Ok(out)
}

fn swizzle(data: &[u8], width: usize, height: usize, bpp: usize) -> Result<Vec<u8>> {
    let row_bytes = width * bpp / 8;
    let mut out = vec![0u8; data.len()];
    let block_w = 16;
    let block_h = 8;
    let blocks_per_row = row_bytes / block_w;
    if blocks_per_row == 0 || !row_bytes.is_multiple_of(block_w) {
        bail!("unsupported swizzled GIM row width {}", row_bytes);
    }
    if data.len() < row_bytes * height {
        bail!("GIM image data is smaller than swizzled dimensions");
    }
    for by in (0..height).step_by(block_h) {
        for bx in 0..blocks_per_row {
            let block_index = (by / block_h) * blocks_per_row + bx;
            let dst_base = block_index * block_w * block_h;
            for iy in 0..block_h {
                let src_row = by + iy;
                if src_row >= height {
                    break;
                }
                let src = src_row * row_bytes + bx * block_w;
                let dst = dst_base + iy * block_w;
                if src + block_w <= data.len() && dst + block_w <= out.len() {
                    out[dst..dst + block_w].copy_from_slice(&data[src..src + block_w]);
                }
            }
        }
    }
    Ok(out)
}

fn ru16(data: &[u8], offset: usize) -> Result<u16> {
    if offset + 2 > data.len() {
        bail!("read past end of GIM data");
    }
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn ru32(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        bail!("read past end of GIM data");
    }
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}
