use anyhow::Result;
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
    for fc in 2u32..200 {
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
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

pub fn build_pzz(streams: &[Vec<u8>], key: u32) -> Vec<u8> {
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
    xor_decrypt(&dec, key)
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
        let compressed = compress_stream(&streams[stream_idx]);
        let mut chunk = Vec::with_capacity(8 + compressed.len());
        chunk.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        chunk.extend_from_slice(&(streams[stream_idx].len() as u32).to_be_bytes());
        chunk.extend_from_slice(&compressed);
        let padded = align_up(chunk.len(), 128);
        let new_units = padded / 128;
        let target_units = if new_units <= old_units {
            old_units
        } else {
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
    let mut total_size = data_start + layout.tail.len();
    for c in &layout.chunks {
        total_size += c.len();
    }
    let mut dec = vec![0u8; total_size];
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
    if !layout.tail.is_empty() {
        dec[cursor..cursor + layout.tail.len()].copy_from_slice(&layout.tail);
    }
    Some(xor_decrypt(&dec, layout.key))
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
        let new_streams = vec![b"PMF2_new_stream_0".to_vec(), b"MIG.00.1PSP_new_stream_1".to_vec()];
        let rebuilt = rebuild_pzz_from_original(&original, &new_streams).unwrap();
        let original_layout = parse_layout(&original).unwrap();
        let rebuilt_layout = parse_layout(&rebuilt).unwrap();
        assert_eq!(original_layout.descriptors.len(), rebuilt_layout.descriptors.len());
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
