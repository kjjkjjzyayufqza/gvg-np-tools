use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::Path;

fn ru32(d: &[u8], o: usize) -> u32 {
    if o + 4 > d.len() {
        return 0;
    }
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
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

    let mut result = original.clone();

    if new_data.len() <= old_size {
        result[old_offset..old_offset + new_data.len()].copy_from_slice(new_data);
        for i in new_data.len()..old_size {
            result[old_offset + i] = 0;
        }
    } else {
        let mut new_offset = result.len();
        while new_offset % 2048 != 0 {
            new_offset += 1;
        }
        result.resize(new_offset, 0);
        result.extend_from_slice(new_data);
        let padding = (2048 - (result.len() % 2048)) % 2048;
        result.resize(result.len() + padding, 0);

        result[entry_off_pos..entry_off_pos + 4]
            .copy_from_slice(&(new_offset as u32).to_le_bytes());
        result[entry_off_pos + 4..entry_off_pos + 8]
            .copy_from_slice(&(new_data.len() as u32).to_le_bytes());
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
