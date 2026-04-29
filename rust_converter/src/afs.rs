use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

fn ru32(d: &[u8], o: usize) -> u32 {
    if o + 4 > d.len() {
        return 0;
    }
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn align_up(v: usize, a: usize) -> usize {
    if a == 0 {
        v
    } else {
        (v + a - 1) & !(a - 1)
    }
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct AfsEntry {
    pub index: usize,
    pub offset: usize,
    pub size: usize,
    pub name: Option<String>,
}

#[derive(Clone, Deserialize, Serialize, Debug, PartialEq, Eq)]
pub struct AfsInventory {
    pub file: Option<String>,
    pub file_count: Option<usize>,
    pub entries: Vec<AfsEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfsShiftedEntry {
    pub index: usize,
    pub old_offset: usize,
    pub new_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfsPatchPlan {
    pub entry_index: usize,
    pub old_offset: usize,
    pub old_size: usize,
    pub new_size: usize,
    pub old_aligned_size: usize,
    pub new_aligned_size: usize,
    pub delta_aligned: isize,
    pub name_table_offset: usize,
    pub shifted_name_table_offset: usize,
    pub shifted_entries: Vec<AfsShiftedEntry>,
}

pub fn load_inventory(path: &Path) -> Result<AfsInventory> {
    let text = std::fs::read_to_string(path)?;
    let inv: AfsInventory = serde_json::from_str(&text)?;
    Ok(inv)
}

pub fn find_entry_by_name<'a>(inv: &'a AfsInventory, name: &str) -> Option<&'a AfsEntry> {
    let lower = name.to_lowercase();
    inv.entries
        .iter()
        .find(|e| e.name.as_deref().map(|n| n.to_lowercase()) == Some(lower.clone()))
}

pub fn read_entry(afs_path: &Path, entry: &AfsEntry) -> Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(afs_path)?;
    f.seek(SeekFrom::Start(entry.offset as u64))?;
    let mut buf = vec![0u8; entry.size];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn scan_inventory(data: &[u8], file_name: Option<String>) -> Result<AfsInventory> {
    if data.len() < 16 {
        bail!("AFS data is too small");
    }
    if &data[0..4] != b"AFS\0" {
        bail!("unsupported AFS magic");
    }
    let file_count = ru32(data, 4) as usize;
    if file_count == 0 {
        bail!("AFS file count is zero");
    }
    let table_offset = 8;
    let name_table_pos = table_offset + file_count * 8;
    if name_table_pos + 8 > data.len() {
        bail!("AFS entry table exceeds file size");
    }
    let name_offset = ru32(data, name_table_pos) as usize;
    let name_size = ru32(data, name_table_pos + 4) as usize;
    let mut entries = Vec::with_capacity(file_count);
    for index in 0..file_count {
        let pos = table_offset + index * 8;
        let offset = ru32(data, pos) as usize;
        let size = ru32(data, pos + 4) as usize;
        if offset.checked_add(size).is_none_or(|end| end > data.len()) {
            bail!("AFS entry {} exceeds file size", index);
        }
        let name = read_name_table_entry(data, name_offset, name_size, file_count, index);
        entries.push(AfsEntry {
            index,
            offset,
            size,
            name,
        });
    }
    Ok(AfsInventory {
        file: file_name,
        file_count: Some(file_count),
        entries,
    })
}

fn read_name_table_entry(
    data: &[u8],
    name_offset: usize,
    name_size: usize,
    file_count: usize,
    index: usize,
) -> Option<String> {
    if name_offset == 0 || name_size < file_count * 0x30 {
        return None;
    }
    let row = name_offset.checked_add(index * 0x30)?;
    if row + 0x30 > data.len() {
        return None;
    }
    let raw = &data[row..row + 0x20];
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    if end == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&raw[..end]).to_string())
}

pub fn plan_patch_entry(data: &[u8], entry_index: usize, new_size: usize) -> Result<AfsPatchPlan> {
    if data.len() < 16 {
        bail!("AFS data is too small");
    }
    if &data[0..4] != b"AFS\0" {
        bail!("unsupported AFS magic");
    }
    let file_count = ru32(data, 4) as usize;
    if file_count == 0 {
        bail!("AFS file count is zero");
    }
    if entry_index >= file_count {
        bail!(
            "Entry index {} out of range (max {})",
            entry_index,
            file_count.saturating_sub(1)
        );
    }
    let table_offset = 8;
    let entry_off_pos = table_offset + entry_index * 8;
    if entry_off_pos + 8 > data.len() {
        bail!("AFS entry table exceeds file size");
    }
    let old_offset = ru32(data, entry_off_pos) as usize;
    let old_size = ru32(data, entry_off_pos + 4) as usize;
    let old_aligned_size = align_up(old_size, 2048);
    let new_aligned_size = align_up(new_size, 2048);
    let delta_aligned = new_aligned_size as isize - old_aligned_size as isize;
    let name_table_pos = table_offset + file_count * 8;
    if name_table_pos + 8 > data.len() {
        bail!("AFS name table descriptor exceeds file size");
    }
    let name_table_offset = ru32(data, name_table_pos) as usize;
    let shifted_name_table_offset = if delta_aligned != 0 && name_table_offset > old_offset {
        (name_table_offset as isize + delta_aligned) as usize
    } else {
        name_table_offset
    };
    let mut shifted_entries = Vec::new();
    for index in 0..file_count {
        let pos = table_offset + index * 8;
        if pos + 8 > data.len() {
            bail!("AFS entry table exceeds file size");
        }
        let offset = ru32(data, pos) as usize;
        let size = ru32(data, pos + 4) as usize;
        let end = offset
            .checked_add(align_up(size, 2048))
            .ok_or_else(|| anyhow::anyhow!("AFS entry {} aligned size overflows", index))?;
        if offset > data.len() || end > data.len() {
            bail!("AFS entry {} exceeds file size", index);
        }
        let new_offset = if index == entry_index {
            old_offset
        } else if delta_aligned != 0 && offset > old_offset {
            (offset as isize + delta_aligned) as usize
        } else {
            offset
        };
        if new_offset != offset {
            shifted_entries.push(AfsShiftedEntry {
                index,
                old_offset: offset,
                new_offset,
            });
        }
    }
    Ok(AfsPatchPlan {
        entry_index,
        old_offset,
        old_size,
        new_size,
        old_aligned_size,
        new_aligned_size,
        delta_aligned,
        name_table_offset,
        shifted_name_table_offset,
        shifted_entries,
    })
}

pub fn patch_afs_entry(
    afs_path: &Path,
    entry_index: usize,
    new_data: &[u8],
    output_path: &Path,
) -> Result<()> {
    let original = std::fs::read(afs_path)?;
    let result = patch_entry_bytes(&original, entry_index, new_data)?;
    std::fs::write(output_path, &result)?;
    eprintln!(
        "Patched AFS: entry {} -> {} bytes at output {}",
        entry_index,
        new_data.len(),
        output_path.display()
    );
    Ok(())
}

pub fn patch_entry_bytes(original: &[u8], entry_index: usize, new_data: &[u8]) -> Result<Vec<u8>> {
    let _plan = plan_patch_entry(original, entry_index, new_data.len())?;
    let file_count = ru32(original, 4) as usize;

    let table_offset = 8;
    let entry_off_pos = table_offset + entry_index * 8;
    let old_offset = ru32(original, entry_off_pos) as usize;
    let old_size = ru32(original, entry_off_pos + 4) as usize;
    let name_table_pos = table_offset + file_count * 8;
    let name_off = ru32(original, name_table_pos) as usize;
    let name_size = ru32(original, name_table_pos + 4) as usize;
    let old_aligned = align_up(old_size, 2048);
    let new_aligned = align_up(new_data.len(), 2048);
    let delta_aligned = new_aligned as isize - old_aligned as isize;

    let mut patched_entry = Vec::with_capacity(new_aligned);
    patched_entry.extend_from_slice(new_data);
    patched_entry.resize(new_aligned, 0);

    let mut result = if delta_aligned == 0 {
        let mut out = original.to_vec();
        let end = old_offset + old_aligned;
        out[old_offset..end].copy_from_slice(&patched_entry);
        out
    } else {
        let mut out = Vec::with_capacity(original.len().saturating_add_signed(delta_aligned));
        let old_end = old_offset + old_aligned;
        out.extend_from_slice(&original[..old_offset]);
        out.extend_from_slice(&patched_entry);
        out.extend_from_slice(&original[old_end..]);
        out
    };

    for i in 0..file_count {
        let pos = table_offset + i * 8;
        let off = ru32(original, pos) as isize;
        let sz = ru32(original, pos + 4) as usize;
        let new_off = if i == entry_index {
            old_offset as isize
        } else if delta_aligned != 0 && off > old_offset as isize {
            off + delta_aligned
        } else {
            off
        };
        result[pos..pos + 4].copy_from_slice(&(new_off as u32).to_le_bytes());
        let new_sz = if i == entry_index { new_data.len() } else { sz };
        result[pos + 4..pos + 8].copy_from_slice(&(new_sz as u32).to_le_bytes());
    }

    let shifted_name_off = if delta_aligned != 0 && name_off > old_offset {
        (name_off as isize + delta_aligned) as usize
    } else {
        name_off
    };
    result[name_table_pos..name_table_pos + 4]
        .copy_from_slice(&(shifted_name_off as u32).to_le_bytes());
    result[name_table_pos + 4..name_table_pos + 8]
        .copy_from_slice(&(name_size as u32).to_le_bytes());

    if shifted_name_off > 0
        && name_size >= file_count * 0x30
        && shifted_name_off + name_size <= result.len()
    {
        let row_off = shifted_name_off + entry_index * 0x30;
        if row_off + 0x30 <= result.len() {
            result[row_off + 0x2C..row_off + 0x30]
                .copy_from_slice(&(new_data.len() as u32).to_le_bytes());
        }
    }

    Ok(result)
}
