use anyhow::{bail, Result};
use std::{collections::BTreeMap, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba5650,
    Rgba5551,
    Rgba4444,
    Rgba8888,
    Indexed4,
    Indexed8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GimReplaceFormat {
    Auto,
    Indexed8,
    Rgba5650,
    Rgba4444,
    Rgba8888,
}

impl GimReplaceFormat {
    pub fn label(self) -> &'static str {
        match self {
            GimReplaceFormat::Auto => "Auto (RGBA4444)",
            GimReplaceFormat::Indexed8 => "Indexed8 + RGBA5551 palette",
            GimReplaceFormat::Rgba5650 => "RGBA5650",
            GimReplaceFormat::Rgba4444 => "RGBA4444",
            GimReplaceFormat::Rgba8888 => "RGBA8888",
        }
    }

    pub fn all() -> &'static [GimReplaceFormat] {
        &[
            GimReplaceFormat::Auto,
            GimReplaceFormat::Indexed8,
            GimReplaceFormat::Rgba4444,
            GimReplaceFormat::Rgba5650,
            GimReplaceFormat::Rgba8888,
        ]
    }

    pub fn key(self) -> &'static str {
        match self {
            GimReplaceFormat::Auto => "auto",
            GimReplaceFormat::Indexed8 => "indexed8",
            GimReplaceFormat::Rgba5650 => "rgba5650",
            GimReplaceFormat::Rgba4444 => "rgba4444",
            GimReplaceFormat::Rgba8888 => "rgba8888",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "auto" => Some(GimReplaceFormat::Auto),
            "indexed8" => Some(GimReplaceFormat::Indexed8),
            "rgba5650" => Some(GimReplaceFormat::Rgba5650),
            "rgba4444" => Some(GimReplaceFormat::Rgba4444),
            "rgba8888" => Some(GimReplaceFormat::Rgba8888),
            _ => None,
        }
    }
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
    size: usize,
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

#[derive(Clone, Debug)]
struct Pl0aGimLayout {
    block_02: GimBlock,
    block_03: GimBlock,
    image_block: GimBlock,
    palette_block: GimBlock,
    image_info: ImageInfo,
    palette_info: ImageInfo,
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

    pub fn replace_png_bytes_resized(&self, png: &[u8]) -> Result<Vec<u8>> {
        self.replace_png_bytes_with_format(png, GimReplaceFormat::Indexed8)
    }

    pub fn replace_png_bytes_with_format(
        &self,
        png: &[u8],
        format: GimReplaceFormat,
    ) -> Result<Vec<u8>> {
        let png_decode_started = Instant::now();
        let image = image::load_from_memory(png)?.to_rgba8();
        let width = image.width() as usize;
        let height = image.height() as usize;
        eprintln!(
            "[texture] decoded replacement PNG {}x{} in {:?}",
            width,
            height,
            png_decode_started.elapsed()
        );
        let collect_started = Instant::now();
        let pixels = image
            .pixels()
            .map(|p| [p.0[0], p.0[1], p.0[2], p.0[3]])
            .collect::<Vec<_>>();
        eprintln!(
            "[texture] collected {} replacement pixels in {:?}",
            pixels.len(),
            collect_started.elapsed()
        );

        if self.metadata.format != PixelFormat::Indexed8 {
            if width != self.metadata.width || height != self.metadata.height {
                bail!(
                    "resized PNG replacement is only supported for pl0a-style Indexed8 GIM textures"
                );
            }
            return self.replace_rgba(&pixels);
        }

        let rebuild_started = Instant::now();
        let resolved_format = match format {
            GimReplaceFormat::Auto => GimReplaceFormat::Rgba4444,
            other => other,
        };
        let rebuilt = match resolved_format {
            GimReplaceFormat::Auto => unreachable!(),
            GimReplaceFormat::Indexed8 => {
                rebuild_pl0a_indexed8_gim(&self.original, width, height, &pixels)?
            }
            GimReplaceFormat::Rgba5650 => rebuild_pl0a_direct_color_gim(
                &self.original,
                width,
                height,
                &pixels,
                PixelFormat::Rgba5650,
            )?,
            GimReplaceFormat::Rgba4444 => rebuild_pl0a_direct_color_gim(
                &self.original,
                width,
                height,
                &pixels,
                PixelFormat::Rgba4444,
            )?,
            GimReplaceFormat::Rgba8888 => rebuild_pl0a_direct_color_gim(
                &self.original,
                width,
                height,
                &pixels,
                PixelFormat::Rgba8888,
            )?,
        };
        eprintln!(
            "[texture] rebuilt {:?} GIM replacement in {:?}",
            resolved_format,
            rebuild_started.elapsed()
        );
        Ok(rebuilt)
    }
}

fn rebuild_pl0a_indexed8_gim(
    original: &[u8],
    width: usize,
    height: usize,
    pixels: &[[u8; 4]],
) -> Result<Vec<u8>> {
    let total_started = Instant::now();
    if pixels.len() != width * height {
        bail!(
            "replacement pixel count {} does not match PNG dimensions {}x{}",
            pixels.len(),
            width,
            height
        );
    }
    if width == 0 || height == 0 || width > u16::MAX as usize || height > u16::MAX as usize {
        bail!(
            "PNG dimensions {}x{} are not supported by GIM u16 fields",
            width,
            height
        );
    }
    if !width.is_multiple_of(16) || !height.is_multiple_of(8) {
        bail!(
            "PNG dimensions {}x{} are not swizzle-safe for this GIM; width must be multiple of 16 and height multiple of 8",
            width,
            height
        );
    }

    let layout_started = Instant::now();
    let layout = parse_pl0a_gim_layout(original)?;
    eprintln!(
        "[texture] parsed pl0a Indexed8 GIM layout in {:?}",
        layout_started.elapsed()
    );
    if layout.image_info.format != PixelFormat::Indexed8
        || !pixel_order_is_swizzled(layout.image_info.pixel_order)?
    {
        bail!("unsupported GIM image layout; expected swizzled Indexed8");
    }
    if layout.image_block.offset != 0x30
        || layout.image_block.data_offset != 0x10
        || layout.image_info.pixels_offset != layout.image_block.offset + 0x10 + 0x40
    {
        bail!("unsupported GIM image layout; expected pl0a-style image offsets");
    }
    if layout.palette_info.format != PixelFormat::Rgba5551
        || pixel_order_is_swizzled(layout.palette_info.pixel_order)?
        || layout.palette_info.width != 256
        || layout.palette_info.height != 1
    {
        bail!("unsupported GIM palette layout; expected linear 256-color RGBA5551");
    }
    if layout.palette_block.data_offset != 0x10
        || layout.palette_info.pixels_offset != layout.palette_block.offset + 0x10 + 0x40
        || layout.palette_block.size != 0x250
    {
        bail!("unsupported GIM palette layout; expected pl0a-style palette offsets");
    }

    let quantize_started = Instant::now();
    let quantized = quantize_indexed8_rgba5551(pixels);
    eprintln!(
        "[texture] quantized {} Indexed8 pixels in {:?}",
        pixels.len(),
        quantize_started.elapsed()
    );
    let swizzle_started = Instant::now();
    let swizzled_indices = swizzle(&quantized.indices, width, height, 8)?;
    let palette_bytes = encode_rgba5551(&quantized.palette);
    eprintln!(
        "[texture] swizzled indices and encoded palette in {:?}",
        swizzle_started.elapsed()
    );

    let build_started = Instant::now();
    let image_pixel_size = width * height;
    let image_data_size = 0x40 + image_pixel_size;
    let image_block_size = 0x10 + image_data_size;
    let palette_block_size = 0x250usize;
    let block_03_size = 0x10 + image_block_size + palette_block_size;
    let block_02_size = 0x10 + block_03_size;
    let file_size = 0x10 + block_02_size;
    let palette_offset = 0x30 + image_block_size;
    let mut out = vec![0u8; file_size];

    out[0..0x10].copy_from_slice(&original[0..0x10]);

    write_u16(&mut out, 0x10, layout.block_02.id as u16);
    write_u16(
        &mut out,
        0x12,
        ru16(original, layout.block_02.offset + 0x02)?,
    );
    write_u32(&mut out, 0x14, block_02_size as u32);
    write_u32(&mut out, 0x18, block_02_size as u32);
    write_u32(&mut out, 0x1C, layout.block_02.data_offset as u32);

    write_u16(&mut out, 0x20, layout.block_03.id as u16);
    write_u16(
        &mut out,
        0x22,
        ru16(original, layout.block_03.offset + 0x02)?,
    );
    write_u32(&mut out, 0x24, block_03_size as u32);
    write_u32(&mut out, 0x28, block_03_size as u32);
    write_u32(&mut out, 0x2C, layout.block_03.data_offset as u32);

    write_u16(&mut out, 0x30, layout.image_block.id as u16);
    write_u16(
        &mut out,
        0x32,
        ru16(original, layout.image_block.offset + 0x02)?,
    );
    write_u32(&mut out, 0x34, image_block_size as u32);
    write_u32(&mut out, 0x38, image_block_size as u32);
    write_u32(&mut out, 0x3C, layout.image_block.data_offset as u32);

    let image_base = 0x40;
    copy_image_header(original, &layout.image_block, &mut out, image_base)?;
    write_u16(&mut out, image_base + 0x08, width as u16);
    write_u16(&mut out, image_base + 0x0A, height as u16);
    write_u32(&mut out, image_base + 0x20, image_data_size as u32);
    let image_pixels_start = image_base + 0x40;
    out[image_pixels_start..image_pixels_start + swizzled_indices.len()]
        .copy_from_slice(&swizzled_indices);

    write_u16(&mut out, palette_offset, layout.palette_block.id as u16);
    write_u16(
        &mut out,
        palette_offset + 0x02,
        ru16(original, layout.palette_block.offset + 0x02)?,
    );
    write_u32(&mut out, palette_offset + 0x04, palette_block_size as u32);
    write_u32(&mut out, palette_offset + 0x08, palette_block_size as u32);
    write_u32(
        &mut out,
        palette_offset + 0x0C,
        layout.palette_block.data_offset as u32,
    );

    let palette_base = palette_offset + 0x10;
    copy_image_header(original, &layout.palette_block, &mut out, palette_base)?;
    out[palette_base + 0x40..palette_base + 0x40 + palette_bytes.len()]
        .copy_from_slice(&palette_bytes);
    eprintln!(
        "[texture] assembled pl0a Indexed8 GIM blocks in {:?} (total {:?})",
        build_started.elapsed(),
        total_started.elapsed()
    );

    Ok(out)
}

fn rebuild_pl0a_direct_color_gim(
    original: &[u8],
    width: usize,
    height: usize,
    pixels: &[[u8; 4]],
    format: PixelFormat,
) -> Result<Vec<u8>> {
    let total_started = Instant::now();
    if pixels.len() != width * height {
        bail!(
            "replacement pixel count {} does not match PNG dimensions {}x{}",
            pixels.len(),
            width,
            height
        );
    }
    if width == 0 || height == 0 || width > u16::MAX as usize || height > u16::MAX as usize {
        bail!(
            "PNG dimensions {}x{} are not supported by GIM u16 fields",
            width,
            height
        );
    }
    let bpp = bits_per_pixel(format)?;
    if format == PixelFormat::Indexed4 || format == PixelFormat::Indexed8 {
        bail!("direct-color GIM rebuild requires an RGBA pixel format");
    }
    if !width.is_multiple_of(16) || !height.is_multiple_of(8) {
        bail!(
            "PNG dimensions {}x{} are not swizzle-safe for this GIM; width must be multiple of 16 and height multiple of 8",
            width,
            height
        );
    }

    let layout = parse_pl0a_gim_layout(original)?;
    if layout.image_info.format != PixelFormat::Indexed8
        || !pixel_order_is_swizzled(layout.image_info.pixel_order)?
    {
        bail!("unsupported GIM image layout; expected swizzled Indexed8 source");
    }
    if layout.image_block.offset != 0x30
        || layout.image_block.data_offset != 0x10
        || layout.image_info.pixels_offset != layout.image_block.offset + 0x10 + 0x40
    {
        bail!("unsupported GIM image layout; expected pl0a-style image offsets");
    }

    let encode_started = Instant::now();
    let mut encoded = match format {
        PixelFormat::Rgba5650 => encode_rgba5650(pixels),
        PixelFormat::Rgba4444 => encode_rgba4444(pixels),
        PixelFormat::Rgba8888 => encode_rgba8888(pixels),
        PixelFormat::Rgba5551 => encode_rgba5551(pixels),
        PixelFormat::Indexed4 | PixelFormat::Indexed8 => unreachable!(),
    };
    encoded = swizzle(&encoded, width, height, bpp)?;
    eprintln!(
        "[texture] encoded and swizzled {:?} pixels in {:?}",
        format,
        encode_started.elapsed()
    );

    let image_pixel_size = encoded.len();
    let image_data_size = 0x40 + image_pixel_size;
    let image_block_size = 0x10 + image_data_size;
    let block_03_size = 0x10 + image_block_size;
    let block_02_size = 0x10 + block_03_size;
    let file_size = 0x10 + block_02_size;
    let mut out = vec![0u8; file_size];

    out[0..0x10].copy_from_slice(&original[0..0x10]);

    write_u16(&mut out, 0x10, layout.block_02.id as u16);
    write_u16(
        &mut out,
        0x12,
        ru16(original, layout.block_02.offset + 0x02)?,
    );
    write_u32(&mut out, 0x14, block_02_size as u32);
    write_u32(&mut out, 0x18, block_02_size as u32);
    write_u32(&mut out, 0x1C, layout.block_02.data_offset as u32);

    write_u16(&mut out, 0x20, layout.block_03.id as u16);
    write_u16(
        &mut out,
        0x22,
        ru16(original, layout.block_03.offset + 0x02)?,
    );
    write_u32(&mut out, 0x24, block_03_size as u32);
    write_u32(&mut out, 0x28, block_03_size as u32);
    write_u32(&mut out, 0x2C, layout.block_03.data_offset as u32);

    write_u16(&mut out, 0x30, layout.image_block.id as u16);
    write_u16(
        &mut out,
        0x32,
        ru16(original, layout.image_block.offset + 0x02)?,
    );
    write_u32(&mut out, 0x34, image_block_size as u32);
    write_u32(&mut out, 0x38, image_block_size as u32);
    write_u32(&mut out, 0x3C, layout.image_block.data_offset as u32);

    let image_base = 0x40;
    copy_image_header(original, &layout.image_block, &mut out, image_base)?;
    write_u16(&mut out, image_base + 0x04, pixel_format_id(format));
    write_u16(&mut out, image_base + 0x08, width as u16);
    write_u16(&mut out, image_base + 0x0A, height as u16);
    write_u16(&mut out, image_base + 0x0C, bpp as u16);
    write_u32(&mut out, image_base + 0x20, image_data_size as u32);
    let image_pixels_start = image_base + 0x40;
    out[image_pixels_start..image_pixels_start + encoded.len()].copy_from_slice(&encoded);

    eprintln!(
        "[texture] assembled pl0a {:?} GIM blocks in {:?}",
        format,
        total_started.elapsed()
    );
    Ok(out)
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
            size,
            data_offset,
            children,
        });
        offset += size;
    }
    Ok(blocks)
}

fn parse_pl0a_gim_layout(data: &[u8]) -> Result<Pl0aGimLayout> {
    if data.len() < 0x10 || &data[0..4] != b"MIG." {
        bail!("unsupported GIM magic");
    }
    let blocks = find_blocks(data, 0x10, data.len())?;
    if blocks.len() != 1 || blocks[0].id != 0x02 {
        bail!("unsupported GIM block tree; expected root block 0x02");
    }
    let block_02 = blocks[0].clone();
    if block_02.children.len() != 1 || block_02.children[0].id != 0x03 {
        bail!("unsupported GIM block tree; expected block 0x03 under block 0x02");
    }
    let block_03 = block_02.children[0].clone();
    if block_03.children.len() != 2 {
        bail!("unsupported GIM block tree; expected image and palette blocks");
    }
    let image_block = block_03
        .children
        .iter()
        .find(|block| block.id == 0x04)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GIM image block 0x04 not found"))?;
    let palette_block = block_03
        .children
        .iter()
        .find(|block| block.id == 0x05)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GIM palette block 0x05 not found"))?;
    let image_info = parse_image_block_data(data, &image_block)?;
    let palette_info = parse_image_block_data(data, &palette_block)?;
    Ok(Pl0aGimLayout {
        block_02,
        block_03,
        image_block,
        palette_block,
        image_info,
        palette_info,
    })
}

fn copy_image_header(
    original: &[u8],
    source_block: &GimBlock,
    out: &mut [u8],
    destination_base: usize,
) -> Result<()> {
    let source_base = source_block.offset + source_block.data_offset;
    if source_base + 0x40 > original.len() || destination_base + 0x40 > out.len() {
        bail!("GIM image header exceeds file size");
    }
    out[destination_base..destination_base + 0x40]
        .copy_from_slice(&original[source_base..source_base + 0x40]);
    Ok(())
}

struct QuantizedIndexed8 {
    indices: Vec<u8>,
    palette: Vec<[u8; 4]>,
}

fn quantize_indexed8_rgba5551(pixels: &[[u8; 4]]) -> QuantizedIndexed8 {
    let mut buckets = BTreeMap::<[u8; 4], (u64, u64, u64, u64, u32)>::new();
    for [r, g, b, a] in pixels {
        let key = [
            (*r >> 3) << 3,
            (*g >> 3) << 3,
            (*b >> 3) << 3,
            if *a >= 128 { 255 } else { 0 },
        ];
        let entry = buckets.entry(key).or_insert((0, 0, 0, 0, 0));
        entry.0 += *r as u64;
        entry.1 += *g as u64;
        entry.2 += *b as u64;
        entry.3 += *a as u64;
        entry.4 += 1;
    }

    let mut palette = buckets
        .values()
        .map(|(r, g, b, a, count)| {
            [
                (r / *count as u64) as u8,
                (g / *count as u64) as u8,
                (b / *count as u64) as u8,
                if a / *count as u64 >= 128 { 255 } else { 0 },
            ]
        })
        .collect::<Vec<_>>();

    if palette.len() > 256 {
        palette.sort_by_key(|[r, g, b, a]| {
            (
                if *a >= 128 { 1u8 } else { 0u8 },
                *r >> 5,
                *g >> 5,
                *b >> 6,
                *r,
                *g,
                *b,
            )
        });
        palette.dedup_by_key(|color| {
            (
                if color[3] >= 128 { 1u8 } else { 0u8 },
                color[0] >> 5,
                color[1] >> 5,
                color[2] >> 6,
            )
        });
        palette.truncate(256);
    }

    if palette.is_empty() {
        palette.push([0, 0, 0, 0]);
    }
    while palette.len() < 256 {
        palette.push([0, 0, 0, 0]);
    }

    let mut index_cache = PaletteIndexCache::default();
    let indices = pixels
        .iter()
        .map(|pixel| index_cache.nearest_index(pixel, &palette))
        .collect::<Vec<_>>();
    QuantizedIndexed8 { indices, palette }
}

#[derive(Default)]
struct PaletteIndexCache {
    indices: BTreeMap<[u8; 4], u8>,
}

impl PaletteIndexCache {
    fn nearest_index(&mut self, pixel: &[u8; 4], palette: &[[u8; 4]]) -> u8 {
        let key = rgba5551_cache_key(pixel);
        if let Some(index) = self.indices.get(&key) {
            return *index;
        }
        let representative = rgba5551_cache_representative(&key);
        let index = nearest_palette_index(&representative, palette);
        self.indices.insert(key, index);
        index
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.indices.len()
    }
}

fn rgba5551_cache_key([r, g, b, a]: &[u8; 4]) -> [u8; 4] {
    [
        (*r as u16 * 31 / 255) as u8,
        (*g as u16 * 31 / 255) as u8,
        (*b as u16 * 31 / 255) as u8,
        if *a >= 128 { 255 } else { 0 },
    ]
}

fn rgba5551_cache_representative([r, g, b, a]: &[u8; 4]) -> [u8; 4] {
    [
        scale_bits(*r as u32, 31),
        scale_bits(*g as u32, 31),
        scale_bits(*b as u32, 31),
        *a,
    ]
}

fn nearest_palette_index(pixel: &[u8; 4], palette: &[[u8; 4]]) -> u8 {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, color)| {
            let dr = pixel[0] as i32 - color[0] as i32;
            let dg = pixel[1] as i32 - color[1] as i32;
            let db = pixel[2] as i32 - color[2] as i32;
            let da = pixel[3] as i32 - color[3] as i32;
            dr * dr + dg * dg + db * db + da * da
        })
        .map(|(index, _)| index as u8)
        .unwrap_or(0)
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

fn pixel_format_id(format: PixelFormat) -> u16 {
    match format {
        PixelFormat::Rgba5650 => 0x00,
        PixelFormat::Rgba5551 => 0x01,
        PixelFormat::Rgba4444 => 0x02,
        PixelFormat::Rgba8888 => 0x03,
        PixelFormat::Indexed4 => 0x04,
        PixelFormat::Indexed8 => 0x05,
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

fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
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

fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgba};
    use std::io::Cursor;

    fn encode_rgba5551_color([r, g, b, a]: [u8; 4]) -> [u8; 2] {
        let value = ((r as u16 * 31 / 255) & 0x1F)
            | (((g as u16 * 31 / 255) & 0x1F) << 5)
            | (((b as u16 * 31 / 255) & 0x1F) << 10)
            | (if a >= 128 { 1 << 15 } else { 0 });
        value.to_le_bytes()
    }

    fn make_pl0a_style_gim(width: usize, height: usize) -> Vec<u8> {
        let image_pixels = width * height;
        let image_block_size = 0x50 + image_pixels;
        let palette_block_size = 0x250usize;
        let block_03_size = 0x10 + image_block_size + palette_block_size;
        let block_02_size = 0x10 + block_03_size;
        let file_size = 0x10 + block_02_size;
        let palette_offset = 0x30 + image_block_size;
        let mut data = vec![0u8; file_size];

        data[0..11].copy_from_slice(b"MIG.00.1PSP");
        write_u16(&mut data, 0x10, 0x02);
        write_u32(&mut data, 0x14, block_02_size as u32);
        write_u32(&mut data, 0x18, block_02_size as u32);
        write_u32(&mut data, 0x1C, 0x10);
        write_u16(&mut data, 0x20, 0x03);
        write_u32(&mut data, 0x24, block_03_size as u32);
        write_u32(&mut data, 0x28, block_03_size as u32);
        write_u32(&mut data, 0x2C, 0x10);
        write_u16(&mut data, 0x30, 0x04);
        write_u32(&mut data, 0x34, image_block_size as u32);
        write_u32(&mut data, 0x38, image_block_size as u32);
        write_u32(&mut data, 0x3C, 0x10);

        let image = 0x40;
        write_u32(&mut data, image, 0x30);
        write_u16(&mut data, image + 0x04, 0x05);
        write_u16(&mut data, image + 0x06, 0x01);
        write_u16(&mut data, image + 0x08, width as u16);
        write_u16(&mut data, image + 0x0A, height as u16);
        write_u16(&mut data, image + 0x0C, 0x08);
        write_u16(&mut data, image + 0x0E, 0x10);
        write_u16(&mut data, image + 0x10, 0x08);
        write_u32(&mut data, image + 0x18, 0x30);
        write_u32(&mut data, image + 0x1C, 0x40);
        write_u32(&mut data, image + 0x20, (0x40 + image_pixels) as u32);
        write_u16(&mut data, image + 0x28, 0x01);
        write_u16(&mut data, image + 0x2A, 0x01);
        write_u16(&mut data, image + 0x2C, 0x03);
        write_u16(&mut data, image + 0x2E, 0x01);
        write_u32(&mut data, image + 0x30, 0x40);

        for i in 0..image_pixels {
            data[0x80 + i] = (i % 4) as u8;
        }

        write_u16(&mut data, palette_offset, 0x05);
        write_u32(&mut data, palette_offset + 0x04, palette_block_size as u32);
        write_u32(&mut data, palette_offset + 0x08, palette_block_size as u32);
        write_u32(&mut data, palette_offset + 0x0C, 0x10);
        let palette = palette_offset + 0x10;
        write_u32(&mut data, palette, 0x30);
        write_u16(&mut data, palette + 0x04, 0x01);
        write_u16(&mut data, palette + 0x06, 0x00);
        write_u16(&mut data, palette + 0x08, 0x100);
        write_u16(&mut data, palette + 0x0A, 0x01);
        write_u16(&mut data, palette + 0x0C, 0x10);
        write_u16(&mut data, palette + 0x0E, 0x10);
        write_u16(&mut data, palette + 0x10, 0x01);
        write_u32(&mut data, palette + 0x18, 0x30);
        write_u32(&mut data, palette + 0x1C, 0x40);
        write_u32(&mut data, palette + 0x20, 0x240);
        write_u16(&mut data, palette + 0x28, 0x02);
        write_u16(&mut data, palette + 0x2A, 0x01);
        write_u16(&mut data, palette + 0x2C, 0x03);
        write_u16(&mut data, palette + 0x2E, 0x01);
        write_u32(&mut data, palette + 0x30, 0x40);
        let colors = [
            [0, 0, 0, 0],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
        ];
        for (index, color) in colors.iter().enumerate() {
            let encoded = encode_rgba5551_color(*color);
            let offset = palette + 0x40 + index * 2;
            data[offset..offset + 2].copy_from_slice(&encoded);
        }

        data
    }

    fn make_png(width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| {
            let color: [u8; 4] = match (x + y) % 4 {
                0 => [255, 0, 0, 255],
                1 => [0, 255, 0, 255],
                2 => [0, 0, 255, 255],
                _ => [255, 255, 0, 255],
            };
            Rgba(color)
        });
        let mut png = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        png
    }

    #[test]
    fn indexed8_palette_index_cache_reuses_quantized_color_keys() {
        let palette = vec![
            [0, 0, 0, 255],
            [8, 8, 8, 255],
            [248, 0, 0, 255],
            [0, 0, 0, 0],
        ];
        let pixels = [
            [1, 2, 3, 200],
            [7, 7, 7, 255],
            [8, 8, 8, 255],
            [15, 15, 15, 255],
            [255, 0, 0, 255],
        ];

        let mut cache = PaletteIndexCache::default();
        let indices = pixels
            .iter()
            .map(|pixel| cache.nearest_index(pixel, &palette))
            .collect::<Vec<_>>();
        let expected = pixels
            .iter()
            .map(|pixel| {
                let key = rgba5551_cache_key(pixel);
                nearest_palette_index(&rgba5551_cache_representative(&key), &palette)
            })
            .collect::<Vec<_>>();

        assert_eq!(indices, expected);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn indexed8_palette_cache_key_matches_rgba5551_encoder_quantization() {
        let same_encoded_pixels = [[0, 0, 0, 255], [8, 8, 8, 255]];
        assert_eq!(
            encode_rgba5551(&same_encoded_pixels[0..1]),
            encode_rgba5551(&same_encoded_pixels[1..2])
        );
        assert_eq!(
            rgba5551_cache_key(&same_encoded_pixels[0]),
            rgba5551_cache_key(&same_encoded_pixels[1])
        );

        let different_encoded_pixels = [[8, 8, 8, 255], [9, 9, 9, 255]];
        assert_ne!(
            encode_rgba5551(&different_encoded_pixels[0..1]),
            encode_rgba5551(&different_encoded_pixels[1..2])
        );
        assert_ne!(
            rgba5551_cache_key(&different_encoded_pixels[0]),
            rgba5551_cache_key(&different_encoded_pixels[1])
        );
    }

    #[test]
    fn indexed8_palette_cache_compares_rgba5551_key_in_8bit_color_space() {
        let palette = vec![[0, 0, 0, 255], [255, 255, 255, 255]];
        let mut cache = PaletteIndexCache::default();

        assert_eq!(cache.nearest_index(&[255, 255, 255, 255], &palette), 1);
        assert_eq!(cache.nearest_index(&[250, 250, 250, 255], &palette), 1);
    }

    #[test]
    fn resized_png_rebuilds_pl0a_style_indexed8_gim_blocks() {
        let gim = make_pl0a_style_gim(16, 8);
        let image = GimImage::decode(&gim).unwrap();
        let rebuilt = image.replace_png_bytes_resized(&make_png(32, 16)).unwrap();

        let rebuilt_image = GimImage::decode(&rebuilt).unwrap();
        assert_eq!(rebuilt_image.metadata.width, 32);
        assert_eq!(rebuilt_image.metadata.height, 16);
        assert_eq!(rebuilt_image.metadata.format, PixelFormat::Indexed8);
        assert!(rebuilt_image.metadata.swizzled);
        assert_eq!(rebuilt_image.rgba.len(), 32 * 16);

        let image_block_size = 0x50 + 32 * 16;
        let palette_offset = 0x30 + image_block_size;
        let block_03_size = 0x10 + image_block_size + 0x250;
        let block_02_size = 0x10 + block_03_size;
        assert_eq!(ru32(&rebuilt, 0x14).unwrap() as usize, block_02_size);
        assert_eq!(ru32(&rebuilt, 0x18).unwrap() as usize, block_02_size);
        assert_eq!(ru32(&rebuilt, 0x24).unwrap() as usize, block_03_size);
        assert_eq!(ru32(&rebuilt, 0x28).unwrap() as usize, block_03_size);
        assert_eq!(ru32(&rebuilt, 0x34).unwrap() as usize, image_block_size);
        assert_eq!(ru32(&rebuilt, 0x38).unwrap() as usize, image_block_size);
        assert_eq!(ru16(&rebuilt, 0x48).unwrap(), 32);
        assert_eq!(ru16(&rebuilt, 0x4A).unwrap(), 16);
        assert_eq!(ru32(&rebuilt, 0x60).unwrap(), 0x40 + 32 * 16);
        assert_eq!(ru16(&rebuilt, palette_offset).unwrap(), 0x05);
        assert_eq!(ru32(&rebuilt, palette_offset + 0x04).unwrap(), 0x250);
        assert_eq!(rebuilt.len(), 0x10 + block_02_size);
    }

    #[test]
    fn resized_png_can_rebuild_pl0a_as_rgba8888_without_palette_block() {
        let gim = make_pl0a_style_gim(16, 8);
        let image = GimImage::decode(&gim).unwrap();
        let rebuilt = image
            .replace_png_bytes_with_format(&make_png(32, 16), GimReplaceFormat::Rgba8888)
            .unwrap();

        let rebuilt_image = GimImage::decode(&rebuilt).unwrap();
        assert_eq!(rebuilt_image.metadata.width, 32);
        assert_eq!(rebuilt_image.metadata.height, 16);
        assert_eq!(rebuilt_image.metadata.format, PixelFormat::Rgba8888);
        assert!(rebuilt_image.metadata.swizzled);
        assert_eq!(rebuilt_image.rgba[0], [255, 0, 0, 255]);
        assert_eq!(rebuilt_image.rgba[1], [0, 255, 0, 255]);
        assert_eq!(rebuilt_image.rgba[2], [0, 0, 255, 255]);

        let image_data_size = 0x40 + 32 * 16 * 4;
        let image_block_size = 0x10 + image_data_size;
        let block_03_size = 0x10 + image_block_size;
        let block_02_size = 0x10 + block_03_size;
        assert_eq!(ru32(&rebuilt, 0x14).unwrap() as usize, block_02_size);
        assert_eq!(ru32(&rebuilt, 0x24).unwrap() as usize, block_03_size);
        assert_eq!(ru32(&rebuilt, 0x34).unwrap() as usize, image_block_size);
        assert_eq!(ru16(&rebuilt, 0x44).unwrap(), 0x03);
        assert_eq!(ru16(&rebuilt, 0x48).unwrap(), 32);
        assert_eq!(ru16(&rebuilt, 0x4A).unwrap(), 16);
        assert_eq!(ru16(&rebuilt, 0x4C).unwrap(), 32);
        assert_eq!(ru16(&rebuilt, 0x50).unwrap(), ru16(&gim, 0x50).unwrap());
        assert_eq!(ru32(&rebuilt, 0x60).unwrap() as usize, image_data_size);
        assert_eq!(rebuilt.len(), 0x10 + block_02_size);
        assert!(find_image_info(
            &rebuilt,
            &find_blocks(&rebuilt, 0x10, rebuilt.len()).unwrap(),
            0x05
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn resized_png_can_rebuild_pl0a_as_rgba4444_with_alpha() {
        let gim = make_pl0a_style_gim(16, 8);
        let image = GimImage::decode(&gim).unwrap();
        let rebuilt = image
            .replace_png_bytes_with_format(&make_png(32, 16), GimReplaceFormat::Rgba4444)
            .unwrap();

        let rebuilt_image = GimImage::decode(&rebuilt).unwrap();
        assert_eq!(rebuilt_image.metadata.width, 32);
        assert_eq!(rebuilt_image.metadata.height, 16);
        assert_eq!(rebuilt_image.metadata.format, PixelFormat::Rgba4444);
        assert!(rebuilt_image.metadata.swizzled);
        assert_eq!(rebuilt_image.rgba[0][3], 255);
        assert!(rebuilt_image.rgba[0][0] > 230);

        let image_data_size = 0x40 + 32 * 16 * 2;
        let image_block_size = 0x10 + image_data_size;
        let block_03_size = 0x10 + image_block_size;
        let block_02_size = 0x10 + block_03_size;
        assert_eq!(ru32(&rebuilt, 0x34).unwrap() as usize, image_block_size);
        assert_eq!(ru16(&rebuilt, 0x44).unwrap(), 0x02);
        assert_eq!(ru16(&rebuilt, 0x4C).unwrap(), 16);
        assert_eq!(ru16(&rebuilt, 0x50).unwrap(), ru16(&gim, 0x50).unwrap());
        assert_eq!(ru32(&rebuilt, 0x60).unwrap() as usize, image_data_size);
        assert_eq!(rebuilt.len(), 0x10 + block_02_size);
    }

    #[test]
    fn resized_png_can_rebuild_pl0a_as_rgba5650() {
        let gim = make_pl0a_style_gim(16, 8);
        let image = GimImage::decode(&gim).unwrap();
        let rebuilt = image
            .replace_png_bytes_with_format(&make_png(32, 16), GimReplaceFormat::Rgba5650)
            .unwrap();

        let rebuilt_image = GimImage::decode(&rebuilt).unwrap();
        assert_eq!(rebuilt_image.metadata.width, 32);
        assert_eq!(rebuilt_image.metadata.height, 16);
        assert_eq!(rebuilt_image.metadata.format, PixelFormat::Rgba5650);
        assert!(rebuilt_image.metadata.swizzled);
        assert_eq!(rebuilt_image.rgba[0][3], 255);
        assert!(rebuilt_image.rgba[0][0] > 240);

        let image_data_size = 0x40 + 32 * 16 * 2;
        let image_block_size = 0x10 + image_data_size;
        assert_eq!(ru32(&rebuilt, 0x34).unwrap() as usize, image_block_size);
        assert_eq!(ru16(&rebuilt, 0x44).unwrap(), 0x00);
        assert_eq!(ru32(&rebuilt, 0x60).unwrap() as usize, image_data_size);
    }

    #[test]
    fn auto_rebuilds_pl0a_indexed8_as_rgba4444() {
        let gim = make_pl0a_style_gim(16, 8);
        let image = GimImage::decode(&gim).unwrap();
        let rebuilt = image
            .replace_png_bytes_with_format(&make_png(32, 16), GimReplaceFormat::Auto)
            .unwrap();

        let rebuilt_image = GimImage::decode(&rebuilt).unwrap();
        assert_eq!(rebuilt_image.metadata.format, PixelFormat::Rgba4444);
        assert_eq!(rebuilt_image.metadata.width, 32);
        assert_eq!(rebuilt_image.metadata.height, 16);
    }
}
