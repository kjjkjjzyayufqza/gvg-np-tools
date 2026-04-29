use anyhow::{bail, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

pub fn xor_decrypt(data: &[u8], key: u32) -> Vec<u8> {
    let kb = key.to_le_bytes();
    let mut out = vec![0u8; data.len()];
    let chunks = data.len() / 4;
    for i in 0..chunks {
        let off = i * 4;
        out[off] = data[off] ^ kb[0];
        out[off + 1] = data[off + 1] ^ kb[1];
        out[off + 2] = data[off + 2] ^ kb[2];
        out[off + 3] = data[off + 3] ^ kb[3];
    }
    for i in (chunks * 4)..data.len() {
        out[i] = data[i] ^ kb[i % 4];
    }
    out
}

fn ru32(d: &[u8], o: usize) -> u32 {
    if o + 4 > d.len() {
        return 0;
    }
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn rb32(d: &[u8], o: usize) -> u32 {
    if o + 4 > d.len() {
        return 0;
    }
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn align_up(v: usize, a: usize) -> usize {
    if a == 0 {
        v
    } else {
        (v + a - 1) & !(a - 1)
    }
}

#[derive(Clone)]
struct PzzLayout {
    key: u32,
    descriptors: Vec<u32>,
    chunks: Vec<Vec<u8>>,
    tail: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PzzStreams {
    pub info: PzzInfo,
    pub streams: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PzzInfo {
    pub key: u32,
    pub descriptor_count: usize,
    pub chunk_count: usize,
    pub stream_count: usize,
    pub has_tail: bool,
    pub body_size: usize,
}

fn parse_layout(raw: &[u8]) -> Option<PzzLayout> {
    let key = find_pzz_key(raw)?;
    let dec = xor_decrypt(raw, key);
    if dec.len() < 8 {
        return None;
    }
    let entry_count = ru32(&dec, 0) as usize;
    if entry_count == 0 || entry_count > 0x4000 {
        return None;
    }
    let table_bytes = (1 + entry_count) * 4;
    let data_start = align_up(table_bytes, 0x800);
    if data_start > dec.len() {
        return None;
    }
    let mut descriptors = Vec::with_capacity(entry_count);
    for i in 0..entry_count {
        descriptors.push(ru32(&dec, 4 + i * 4));
    }
    let mut chunks = Vec::with_capacity(entry_count);
    let mut cursor = data_start;
    for desc in &descriptors {
        let units = (desc & 0x3FFF_FFFF) as usize;
        let chunk_size = units.saturating_mul(128);
        if cursor + chunk_size > dec.len() {
            return None;
        }
        chunks.push(dec[cursor..cursor + chunk_size].to_vec());
        cursor += chunk_size;
    }
    Some(PzzLayout {
        key,
        descriptors,
        chunks,
        tail: dec[cursor..].to_vec(),
    })
}

fn decode_stream_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
    if chunk.len() < 8 {
        return None;
    }
    let comp_len = rb32(chunk, 0) as usize;
    let raw_len = rb32(chunk, 4) as usize;
    if comp_len == 0 || 8 + comp_len > chunk.len() {
        return None;
    }
    let out = try_zlib_decompress(&chunk[8..8 + comp_len]).ok()?;
    if out.len() != raw_len {
        return None;
    }
    Some(out)
}

fn stream_chunk_indices(layout: &PzzLayout) -> Vec<usize> {
    layout
        .descriptors
        .iter()
        .enumerate()
        .filter_map(|(i, desc)| {
            if desc & 0x4000_0000 == 0 {
                return None;
            }
            decode_stream_chunk(&layout.chunks[i]).map(|_| i)
        })
        .collect()
}

pub fn find_pzz_key(raw: &[u8]) -> Option<u32> {
    let sz = raw.len();
    if sz < 8 {
        return None;
    }
    let raw_w0 = ru32(raw, 0);
    for fc in 1u32..200 {
        let key = raw_w0 ^ fc;
        let limit = sz.min(0x4000);
        let dec_partial = xor_decrypt(&raw[..limit], key);
        let d0 = ru32(&dec_partial, 0);
        if d0 != fc {
            continue;
        }
        let table_bytes = ((1 + fc) * 4) as usize;
        let padding_end = ((table_bytes + 0x7FF) & !0x7FF).min(sz);
        let mut ok = true;
        let mut off = table_bytes;
        while off < padding_end {
            if off + 4 > dec_partial.len() {
                break;
            }
            if ru32(&dec_partial, off) != 0 {
                ok = false;
                break;
            }
            off += 4;
        }
        if ok {
            return Some(key);
        }
    }
    None
}

pub fn harvest_zlib(dec: &[u8]) -> Vec<Vec<u8>> {
    let headers: &[&[u8]] = &[b"\x78\x9c", b"\x78\x01", b"\x78\xda", b"\x78\x5e"];
    let mut offsets = std::collections::BTreeSet::new();
    for hdr in headers {
        let mut start = 0;
        while let Some(pos) = find_bytes(dec, hdr, start) {
            offsets.insert(pos);
            start = pos + 1;
        }
    }
    let mut results = Vec::new();
    for &off in &offsets {
        if let Ok(data) = try_zlib_decompress(&dec[off..]) {
            if data.len() >= 16 {
                results.push(data);
            }
        }
    }
    results
}

fn find_bytes(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start + needle.len() > haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

fn try_zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

pub fn extract_pzz_streams(pzz_data: &[u8]) -> Vec<Vec<u8>> {
    if let Some(layout) = parse_layout(pzz_data) {
        let mut out = Vec::new();
        for i in stream_chunk_indices(&layout) {
            if let Some(stream) = decode_stream_chunk(&layout.chunks[i]) {
                out.push(stream);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let key = match find_pzz_key(pzz_data) {
        Some(k) => k,
        None => return vec![],
    };
    let dec = xor_decrypt(pzz_data, key);
    harvest_zlib(&dec)
}

pub fn compress_stream(data: &[u8]) -> Vec<u8> {
    compress_stream_level(data, 6)
}

fn compress_stream_level(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

pub fn build_pzz(streams: &[Vec<u8>], key: u32) -> Vec<u8> {
    build_pzz_with_tail(streams, key, false)
}

pub fn build_pzz_with_tail(streams: &[Vec<u8>], key: u32, include_tail: bool) -> Vec<u8> {
    let dec = build_decrypted_stream_archive(streams);
    let mut encrypted = xor_decrypt(&dec, key);
    if include_tail {
        encrypted.extend_from_slice(&compute_pzz_tail(&dec));
    }
    encrypted
}

fn build_decrypted_stream_archive(streams: &[Vec<u8>]) -> Vec<u8> {
    let mut descriptors = Vec::new();
    let mut chunks = Vec::new();
    for s in streams {
        let compressed = compress_stream(s);
        let mut chunk = Vec::with_capacity(8 + compressed.len());
        chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&(s.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&compressed);
        let padded = align_up(chunk.len(), 128);
        chunk.resize(padded, 0);
        let units = (padded / 128) as u32;
        descriptors.push(0x4000_0000 | units);
        chunks.push(chunk);
    }
    descriptors.push(0);
    let entry_count = descriptors.len();
    let table_bytes = (1 + entry_count) * 4;
    let data_start = align_up(table_bytes, 0x800);
    let mut total_size = data_start;
    for c in &chunks {
        total_size += c.len();
    }
    let mut dec = vec![0u8; total_size];
    dec[0..4].copy_from_slice(&(entry_count as u32).to_le_bytes());
    for (i, d) in descriptors.iter().enumerate() {
        let pos = 4 + i * 4;
        dec[pos..pos + 4].copy_from_slice(&d.to_le_bytes());
    }
    let mut cursor = data_start;
    for c in &chunks {
        dec[cursor..cursor + c.len()].copy_from_slice(c);
        cursor += c.len();
    }
    dec
}

pub fn inspect_pzz(raw: &[u8]) -> Result<PzzInfo> {
    let layout = parse_layout(raw).ok_or_else(|| anyhow::anyhow!("failed to parse PZZ layout"))?;
    let stream_count = stream_chunk_indices(&layout).len();
    let chunk_size: usize = layout.chunks.iter().map(Vec::len).sum();
    let entry_count = layout.descriptors.len();
    let table_bytes = (1 + entry_count) * 4;
    let data_start = align_up(table_bytes, 0x800);
    Ok(PzzInfo {
        key: layout.key,
        descriptor_count: entry_count,
        chunk_count: layout.chunks.len(),
        stream_count,
        has_tail: layout.tail.len() == 16,
        body_size: data_start + chunk_size,
    })
}

pub fn extract_pzz_streams_strict(raw: &[u8]) -> Result<PzzStreams> {
    let layout = parse_layout(raw).ok_or_else(|| anyhow::anyhow!("failed to parse PZZ layout"))?;
    let mut streams = Vec::new();
    for chunk_index in stream_chunk_indices(&layout) {
        let stream = decode_stream_chunk(&layout.chunks[chunk_index])
            .ok_or_else(|| anyhow::anyhow!("failed to decode PZZ stream chunk {}", chunk_index))?;
        streams.push(stream);
    }
    if streams.is_empty() {
        bail!("PZZ contains no decodable streams");
    }
    let info = inspect_pzz(raw)?;
    Ok(PzzStreams { info, streams })
}

pub fn decrypt_pzz_body(raw: &[u8]) -> Result<Vec<u8>> {
    let info = inspect_pzz(raw)?;
    if info.body_size > raw.len() {
        bail!("PZZ body size exceeds file size");
    }
    Ok(xor_decrypt(&raw[..info.body_size], info.key))
}

pub fn rebuild_stream_archive_with_original_key(
    original_pzz: &[u8],
    streams: &[Vec<u8>],
) -> Result<Vec<u8>> {
    let info = inspect_pzz(original_pzz)?;
    let expected_stream_archive_chunks = info.stream_count + 1;
    if info.chunk_count != expected_stream_archive_chunks {
        bail!(
            "cannot change stream count while preserving {} non-stream chunks",
            info.chunk_count.saturating_sub(info.stream_count)
        );
    }
    Ok(build_pzz_with_tail(streams, info.key, info.has_tail))
}

pub fn rebuild_pzz_from_original(original_pzz: &[u8], streams: &[Vec<u8>]) -> Option<Vec<u8>> {
    let mut layout = parse_layout(original_pzz)?;
    let stream_chunks = stream_chunk_indices(&layout);
    if stream_chunks.len() != streams.len() {
        return None;
    }
    for (stream_idx, chunk_idx) in stream_chunks.into_iter().enumerate() {
        if let Some(old_decoded) = decode_stream_chunk(&layout.chunks[chunk_idx]) {
            if old_decoded == streams[stream_idx] {
                continue;
            }
        }
        let old_units = (layout.descriptors[chunk_idx] & 0x3FFF_FFFF) as usize;
        let old_capacity = old_units * 128;
        let stream_data = &streams[stream_idx];
        let mut best_compressed = compress_stream(stream_data);
        if 8 + best_compressed.len() > old_capacity {
            for level in 7..=9 {
                let alt = compress_stream_level(stream_data, level);
                if alt.len() < best_compressed.len() {
                    best_compressed = alt;
                }
                if 8 + best_compressed.len() <= old_capacity {
                    break;
                }
            }
        }
        let mut chunk = Vec::with_capacity(8 + best_compressed.len());
        chunk.extend_from_slice(&(best_compressed.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&(stream_data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&best_compressed);
        let padded = align_up(chunk.len(), 128);
        let new_units = padded / 128;
        let target_units = if new_units <= old_units {
            old_units
        } else {
            eprintln!(
                "WARNING: stream {} compressed size exceeds original chunk ({} > {} bytes)",
                stream_idx,
                8 + best_compressed.len(),
                old_capacity
            );
            new_units
        };
        chunk.resize(target_units * 128, 0);
        let flags = layout.descriptors[chunk_idx] & 0xC000_0000;
        layout.descriptors[chunk_idx] = flags | (target_units as u32);
        layout.chunks[chunk_idx] = chunk;
    }
    let entry_count = layout.descriptors.len();
    let table_bytes = (1 + entry_count) * 4;
    let data_start = align_up(table_bytes, 0x800);
    let mut body_size = data_start;
    for c in &layout.chunks {
        body_size += c.len();
    }
    let has_tail = layout.tail.len() == 16;
    let mut dec = vec![0u8; body_size];
    dec[0..4].copy_from_slice(&(entry_count as u32).to_le_bytes());
    for (i, d) in layout.descriptors.iter().enumerate() {
        let pos = 4 + i * 4;
        dec[pos..pos + 4].copy_from_slice(&d.to_le_bytes());
    }
    let mut cursor = data_start;
    for c in &layout.chunks {
        dec[cursor..cursor + c.len()].copy_from_slice(c);
        cursor += c.len();
    }
    let encrypted_body = xor_decrypt(&dec, layout.key);
    if has_tail {
        let new_tail = compute_pzz_tail(&dec);
        let mut result = encrypted_body;
        result.extend_from_slice(&new_tail);
        Some(result)
    } else {
        Some(encrypted_body)
    }
}

#[cfg(test)]
const SBOX1: [u8; 256] = [
    0x1e, 0x65, 0xc2, 0x22, 0x20, 0xc5, 0x6c, 0xf1, 0xb7, 0x07, 0x73, 0x2a, 0x31, 0x43, 0x48, 0x3d,
    0x75, 0x30, 0x1b, 0x78, 0x09, 0x2d, 0xc7, 0xad, 0x0a, 0xf6, 0x3c, 0xac, 0x5a, 0x7e, 0xdd, 0x0d,
    0x5b, 0x25, 0x00, 0xfd, 0x9b, 0x02, 0xbd, 0x52, 0x08, 0x93, 0x8b, 0x9d, 0x46, 0x11, 0x34, 0xb2,
    0xbb, 0xcd, 0xd0, 0xc4, 0x84, 0xc6, 0xd4, 0x28, 0x6e, 0xcf, 0x32, 0x9e, 0x19, 0xeb, 0xe2, 0x40,
    0xca, 0xc9, 0xc1, 0xa0, 0x1c, 0x60, 0xe0, 0x39, 0x4c, 0x56, 0x45, 0x69, 0xe3, 0x3e, 0x9f, 0x05,
    0x35, 0xcc, 0xb0, 0x13, 0x0f, 0xda, 0xf8, 0x26, 0xfe, 0x99, 0x54, 0xd8, 0xae, 0x92, 0x29, 0xe4,
    0x72, 0x2b, 0xc0, 0x04, 0x23, 0x15, 0x37, 0xa3, 0xf4, 0x49, 0xa5, 0x5d, 0xbf, 0x7c, 0x38, 0xc8,
    0x06, 0x89, 0xbe, 0xdb, 0xb1, 0xa1, 0x27, 0x74, 0x4d, 0x4b, 0x03, 0x51, 0x16, 0x01, 0x77, 0xf0,
    0x55, 0x5e, 0x97, 0xd3, 0x0e, 0x50, 0xed, 0x63, 0x6d, 0xd5, 0xc3, 0x4f, 0x82, 0xbc, 0x91, 0x80,
    0xa9, 0xce, 0x3b, 0x36, 0xec, 0x79, 0x1d, 0x5c, 0x24, 0x98, 0x8e, 0xdf, 0xe8, 0x4a, 0xaa, 0xf9,
    0xe1, 0xef, 0xfc, 0x9c, 0xe9, 0x17, 0xee, 0xb8, 0xa4, 0xf2, 0xaf, 0x83, 0x1f, 0xfa, 0x58, 0x18,
    0xa8, 0x6f, 0x71, 0x8c, 0x95, 0xe6, 0x85, 0xf7, 0x64, 0xf5, 0xb5, 0x33, 0xd7, 0x12, 0xe7, 0x7f,
    0xff, 0x86, 0x5f, 0x9a, 0x62, 0x8f, 0x2f, 0x68, 0xd6, 0xa6, 0xb4, 0x53, 0x3a, 0x76, 0xd1, 0x7a,
    0x7d, 0xab, 0x21, 0x90, 0x6a, 0xba, 0xea, 0xfb, 0x44, 0x59, 0xb6, 0x87, 0xe5, 0x0b, 0x1a, 0x67,
    0x8d, 0xb3, 0x14, 0xa7, 0xa2, 0x3f, 0xd9, 0x0c, 0x8a, 0x81, 0x10, 0x7b, 0xdc, 0xcb, 0xf3, 0x66,
    0x2e, 0x57, 0xb9, 0x47, 0x61, 0x4e, 0x2c, 0x42, 0xde, 0x96, 0x6b, 0x41, 0x94, 0x88, 0x70, 0xd2,
];

#[cfg(test)]
const SBOX2: [u8; 256] = [
    0x74, 0x97, 0x27, 0x1e, 0x65, 0xfe, 0xf5, 0x09, 0x71, 0x78, 0x1d, 0x54, 0x7b, 0xd3, 0x16, 0x98,
    0x87, 0x4c, 0xe9, 0x33, 0xb9, 0x82, 0x8f, 0x6c, 0x3e, 0x5d, 0x24, 0x55, 0x23, 0x7e, 0xee, 0xd9,
    0x32, 0xe2, 0xeb, 0x94, 0x2f, 0x9c, 0x31, 0x7a, 0x02, 0x10, 0x2b, 0xcf, 0x56, 0xa7, 0xce, 0x6b,
    0xc6, 0x67, 0x06, 0x4e, 0xb8, 0xb4, 0xcc, 0xae, 0x8e, 0xd1, 0xe5, 0xc8, 0x59, 0xcd, 0x8c, 0x49,
    0x51, 0x03, 0xbf, 0x89, 0x4f, 0x95, 0x07, 0x25, 0x4b, 0xc4, 0xe7, 0xd2, 0xfd, 0x44, 0x96, 0x91,
    0x66, 0x05, 0x80, 0xc3, 0x19, 0x0d, 0xff, 0x20, 0xa8, 0x2a, 0xd8, 0x79, 0xd5, 0x5b, 0x84, 0x9d,
    0xc0, 0x36, 0x6a, 0x9e, 0xa0, 0x9f, 0x3b, 0xa4, 0xe0, 0x21, 0x2c, 0x5f, 0x53, 0xbe, 0x11, 0x81,
    0x28, 0x47, 0xa1, 0x88, 0x12, 0x26, 0x39, 0xb0, 0xfb, 0x3a, 0x50, 0xbd, 0x5a, 0xf4, 0xbc, 0xab,
    0x40, 0x04, 0xb3, 0xf6, 0x9b, 0xcb, 0xe1, 0x3f, 0xbb, 0x1c, 0xde, 0x73, 0x0f, 0x08, 0x01, 0x15,
    0x13, 0x42, 0x72, 0x4d, 0x0c, 0x1a, 0xb7, 0x7d, 0xf7, 0xec, 0xac, 0x48, 0x62, 0x34, 0xfa, 0xba,
    0xa6, 0xdf, 0x7c, 0x92, 0x8a, 0xad, 0xb5, 0x75, 0x64, 0x69, 0xc2, 0x5c, 0xda, 0x90, 0x68, 0x43,
    0xa5, 0xaa, 0xf9, 0xe6, 0x41, 0x63, 0x57, 0x6d, 0x14, 0x93, 0x6e, 0x61, 0x83, 0xc5, 0x17, 0x52,
    0x4a, 0x30, 0xf2, 0x2d, 0x22, 0xe8, 0x35, 0x76, 0xd7, 0x45, 0xf1, 0xdc, 0xb6, 0xc7, 0xca, 0xdb,
    0xed, 0xd4, 0xf3, 0xd0, 0xaf, 0x60, 0xb2, 0x18, 0x38, 0xc1, 0xea, 0xa3, 0xdd, 0xa2, 0x3c, 0x0e,
    0x8b, 0x9a, 0x3d, 0x1f, 0xa9, 0x00, 0x0b, 0xef, 0xe3, 0x5e, 0x46, 0xb1, 0x99, 0x29, 0x85, 0x1b,
    0xf8, 0x86, 0x37, 0x58, 0xc9, 0x77, 0xfc, 0x8d, 0x2e, 0xf0, 0x0a, 0xd6, 0xe4, 0x70, 0x7f, 0x6f,
];

#[cfg(test)]
fn sbox_lookup(idx: u8, step: u8, table: &[u8; 256]) -> u32 {
    let mut i = idx;
    let b0 = table[i as usize];
    i = i.wrapping_add(step);
    let b1 = table[i as usize];
    i = i.wrapping_add(step);
    let b2 = table[i as usize];
    i = i.wrapping_add(step);
    let b3 = table[i as usize];
    (b0 as u32) << 24 | (b1 as u32) << 16 | (b2 as u32) << 8 | b3 as u32
}

#[cfg(test)]
fn derive_xor_key_from_size(body_size: usize) -> u32 {
    let sz = body_size as u32;
    let mut nibble = 0u8;
    let mut shift = 3u32;
    for _ in 0..10 {
        let n = ((sz >> shift) & 0xF) as u8;
        if n != 0 {
            nibble = n;
            break;
        }
        shift += 3;
    }
    let derived = ((nibble as u16 + 3) * 13) as u8;
    let key1 = sbox_lookup(nibble, 3, &SBOX1);
    let key2 = sbox_lookup(derived, 2, &SBOX2);
    key1 ^ key2
}

pub fn compute_pzz_tail(decrypted_body: &[u8]) -> [u8; 16] {
    let body_size = decrypted_body.len();
    let init_sum_lo = ((body_size as u64).wrapping_mul(7) >> 1) as u32;
    let mut sum_lo: u32 = init_sum_lo;
    let mut sum_hi: u32 = 0;
    let mut xor_lo: u32 = 0xFFFF_FFFF;
    let mut xor_hi: u32 = 0xFFFF_FFFF;

    let word_count = body_size / 4;
    for i in 0..word_count {
        let off = i * 4;
        let word = u32::from_le_bytes([
            decrypted_body[off],
            decrypted_body[off + 1],
            decrypted_body[off + 2],
            decrypted_body[off + 3],
        ]);
        let sum64 = (sum_lo as u64) + (word as u64);
        sum_lo = sum64 as u32;
        sum_hi = sum_hi.wrapping_add((sum64 >> 32) as u32);
        xor_lo ^= sum_lo;
        xor_hi ^= sum_hi;
    }

    let remainder = body_size % 4;
    if remainder > 0 {
        let off = word_count * 4;
        let mut partial: u32 = 0;
        for j in 0..remainder {
            partial |= (decrypted_body[off + j] as u32) << (j * 8);
        }
        let masks = [0u32, 0x0000_00FF, 0x0000_FFFF, 0x00FF_FFFF];
        partial &= masks[remainder];
        let sum64 = (sum_lo as u64) + (partial as u64);
        sum_lo = sum64 as u32;
        sum_hi = sum_hi.wrapping_add((sum64 >> 32) as u32);
        xor_lo ^= sum_lo;
        xor_hi ^= sum_hi;
    }

    let mut tail = [0u8; 16];
    tail[0..4].copy_from_slice(&sum_lo.to_le_bytes());
    tail[4..8].copy_from_slice(&sum_hi.to_le_bytes());
    tail[8..12].copy_from_slice(&xor_lo.to_le_bytes());
    tail[12..16].copy_from_slice(&xor_hi.to_le_bytes());
    tail
}

pub fn classify_stream(data: &[u8]) -> &'static str {
    if data.len() < 4 {
        return "unknown";
    }
    match &data[..4] {
        b"PMF2" => "pmf2",
        b"SAD " => "sad",
        b"MIG." | b"GIM\x00" => "gim",
        _ => {
            if data.len() >= 11 && &data[..11] == b"MIG.00.1PSP" {
                "gim"
            } else {
                "unknown"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_pzz() -> (Vec<u8>, Vec<u8>) {
        let key = 0x1234_5678;
        let s0 = b"PMF2_test_stream_0".to_vec();
        let s1 = b"MIG.00.1PSP_stream_1".to_vec();
        let raw_tail_chunk = vec![0xAB; 256];
        let mut descriptors = Vec::new();
        let mut chunks = Vec::new();
        for s in [&s0, &s1] {
            let compressed = compress_stream(s);
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
            chunk.extend_from_slice(&(s.len() as u32).to_be_bytes());
            chunk.extend_from_slice(&compressed);
            let padded = align_up(chunk.len(), 128);
            chunk.resize(padded, 0);
            descriptors.push(0x4000_0000 | ((padded / 128) as u32));
            chunks.push(chunk);
        }
        descriptors.push((raw_tail_chunk.len() / 128) as u32);
        chunks.push(raw_tail_chunk.clone());
        let entry_count = descriptors.len();
        let table_bytes = (1 + entry_count) * 4;
        let data_start = align_up(table_bytes, 0x800);
        let total_size = data_start + chunks.iter().map(|c| c.len()).sum::<usize>();
        let mut dec = vec![0u8; total_size];
        dec[0..4].copy_from_slice(&(entry_count as u32).to_le_bytes());
        for (i, d) in descriptors.iter().enumerate() {
            let pos = 4 + i * 4;
            dec[pos..pos + 4].copy_from_slice(&d.to_le_bytes());
        }
        let mut cursor = data_start;
        for c in &chunks {
            dec[cursor..cursor + c.len()].copy_from_slice(c);
            cursor += c.len();
        }
        (xor_decrypt(&dec, key), raw_tail_chunk)
    }

    #[test]
    fn rebuild_from_original_preserves_non_stream_chunk() {
        let (original, raw_tail_chunk) = make_test_pzz();
        let new_streams = vec![
            b"PMF2_new_stream_0".to_vec(),
            b"MIG.00.1PSP_new_stream_1".to_vec(),
        ];
        let rebuilt = rebuild_pzz_from_original(&original, &new_streams).unwrap();
        let original_layout = parse_layout(&original).unwrap();
        let rebuilt_layout = parse_layout(&rebuilt).unwrap();
        assert_eq!(
            original_layout.descriptors.len(),
            rebuilt_layout.descriptors.len()
        );
        assert_eq!(
            original_layout.descriptors.last().copied().unwrap_or(0),
            rebuilt_layout.descriptors.last().copied().unwrap_or(1)
        );
        assert_eq!(rebuilt_layout.chunks.last().unwrap(), &raw_tail_chunk);
        let extracted = extract_pzz_streams(&rebuilt);
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0], new_streams[0]);
        assert_eq!(extracted[1], new_streams[1]);
    }

    #[test]
    fn derive_xor_key_matches_known_pzz() {
        assert_eq!(derive_xor_key_from_size(631168), 0x4AB70B80);
        assert_eq!(derive_xor_key_from_size(609664), 0x4AB70B80);
        assert_eq!(derive_xor_key_from_size(861952), 0x1CD56D68);
    }

    #[test]
    fn tail_hash_round_trip_preserves_original() {
        let body_size = 256usize;
        let mut dec_body = vec![0u8; body_size];
        dec_body[0..4].copy_from_slice(&2u32.to_le_bytes());
        dec_body[4..8].copy_from_slice(&(0x4000_0001u32).to_le_bytes());
        dec_body[8..12].copy_from_slice(&(0x4000_0001u32).to_le_bytes());
        let tail = compute_pzz_tail(&dec_body);
        assert_eq!(tail.len(), 16);
        let tail2 = compute_pzz_tail(&dec_body);
        assert_eq!(tail, tail2);
    }

    #[test]
    fn rebuild_from_original_keeps_descriptor_units_when_new_stream_shrinks() {
        let stream0_old: Vec<u8> = (0..=255).cycle().take(65536).collect();
        let stream1_old = b"MIG.00.1PSP_stream_1".to_vec();
        let original = build_pzz(&[stream0_old, stream1_old], 0x1234_5678);
        let old_layout = parse_layout(&original).unwrap();
        let old_stream_indices = stream_chunk_indices(&old_layout);
        let old_units0 = old_layout.descriptors[old_stream_indices[0]] & 0x3FFF_FFFF;

        let new_streams = vec![b"PMF2_small".to_vec(), b"MIG.00.1PSP_stream_1".to_vec()];
        let rebuilt = rebuild_pzz_from_original(&original, &new_streams).unwrap();
        let new_layout = parse_layout(&rebuilt).unwrap();
        let new_stream_indices = stream_chunk_indices(&new_layout);
        let new_units0 = new_layout.descriptors[new_stream_indices[0]] & 0x3FFF_FFFF;
        assert_eq!(new_units0, old_units0);

        let extracted = extract_pzz_streams(&rebuilt);
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0], new_streams[0]);
    }
}
