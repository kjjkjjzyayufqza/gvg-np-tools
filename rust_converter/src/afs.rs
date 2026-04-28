use anyhow::{bail, Result};
use serde::Deserialize;
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

#[derive(Deserialize, Debug)]
pub struct AfsEntry {
    pub index: usize,
    pub offset: usize,
    pub size: usize,
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct AfsInventory {
    pub file: Option<String>,
    pub file_count: Option<usize>,
    pub entries: Vec<AfsEntry>,
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

pub fn patch_afs_entry(
    afs_path: &Path,
    entry_index: usize,
    new_data: &[u8],
    output_path: &Path,
) -> Result<()> {
    let original = std::fs::read(afs_path)?;
    let file_count = ru32(&original, 4) as usize;
    if entry_index >= file_count {
        bail!(
            "Entry index {} out of range (max {})",
            entry_index,
            file_count - 1
        );
    }

    let table_offset = 8;
    let entry_off_pos = table_offset + entry_index * 8;
    let old_offset = ru32(&original, entry_off_pos) as usize;
    let old_size = ru32(&original, entry_off_pos + 4) as usize;
    let name_table_pos = table_offset + file_count * 8;
    let name_off = ru32(&original, name_table_pos) as usize;
    let name_size = ru32(&original, name_table_pos + 4) as usize;
    let old_aligned = align_up(old_size, 2048);
    let new_aligned = align_up(new_data.len(), 2048);
    let delta_aligned = new_aligned as isize - old_aligned as isize;

    let mut patched_entry = Vec::with_capacity(new_aligned);
    patched_entry.extend_from_slice(new_data);
    patched_entry.resize(new_aligned, 0);

    let mut result = if delta_aligned == 0 {
        let mut out = original.clone();
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
        let off = ru32(&original, pos) as isize;
        let sz = ru32(&original, pos + 4) as usize;
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

    std::fs::write(output_path, &result)?;
    eprintln!(
        "Patched AFS: entry {} -> {} bytes at output {}",
        entry_index,
        new_data.len(),
        output_path.display()
    );
    Ok(())
}
