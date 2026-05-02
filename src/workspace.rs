use crate::{
    afs::{self, AfsEntry},
    pzz,
};
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Pzz,
    Pmf2,
    Gim,
    Sad,
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryValidation {
    Ok,
    Empty,
    ExceedsBounds,
    Overlapping,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfsEntryNode {
    pub index: usize,
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub kind: AssetKind,
    pub validation: EntryValidation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamNode {
    pub index: usize,
    pub name: String,
    pub size: usize,
    pub kind: AssetKind,
    pub dirty: bool,
}

#[derive(Clone, Debug)]
pub struct PzzWorkspace {
    name: String,
    afs_entry_index: Option<usize>,
    original: Vec<u8>,
    streams: Vec<Vec<u8>>,
    stream_nodes: Vec<StreamNode>,
    revision: u64,
    cached_rebuild: Option<CachedPzzRebuild>,
}

#[derive(Clone, Debug)]
pub struct CachedPzzRebuild {
    revision: u64,
    payload: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ModWorkspace {
    afs_path: Option<PathBuf>,
    afs_bytes: Option<Vec<u8>>,
    afs_entries: Vec<AfsEntryNode>,
    open_pzz: Option<PzzWorkspace>,
    staged_pzz: BTreeMap<usize, PzzWorkspace>,
    expanded_pzz_entry: Option<usize>,
    operations: Vec<String>,
}

impl ModWorkspace {
    pub fn open_afs_file(path: PathBuf) -> Result<Self> {
        eprintln!("[workspace] open_afs_file: {}", path.display());
        let (inventory, file_len) = afs::scan_inventory_from_file(&path)?;
        let afs_entries = inventory
            .entries
            .iter()
            .cloned()
            .map(|entry| entry_to_node(&entry, file_len))
            .collect::<Vec<_>>();
        let count = afs_entries.len();
        let pzz_count = afs_entries
            .iter()
            .filter(|e| e.kind == AssetKind::Pzz)
            .count();
        let empty_count = afs_entries
            .iter()
            .filter(|e| e.validation == EntryValidation::Empty)
            .count();
        let invalid_count = afs_entries
            .iter()
            .filter(|e| e.validation == EntryValidation::ExceedsBounds)
            .count();
        eprintln!(
            "[workspace] AFS loaded: {} entries total, {} PZZ, {} empty, {} invalid, file_len={}",
            count, pzz_count, empty_count, invalid_count, file_len
        );
        Ok(Self {
            afs_path: Some(path),
            afs_bytes: None,
            afs_entries,
            open_pzz: None,
            staged_pzz: BTreeMap::new(),
            expanded_pzz_entry: None,
            operations: vec![format!("Loaded AFS ({} entries)", count)],
        })
    }

    pub fn open_pzz_file(path: PathBuf) -> Result<Self> {
        let data = std::fs::read(&path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("model.pzz")
            .to_string();
        Self::open_pzz_bytes(name, data)
    }

    pub fn open_pzz_bytes(name: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let name = name.into();
        let pzz = PzzWorkspace::new(name.clone(), None, data)?;
        Ok(Self {
            afs_path: None,
            afs_bytes: None,
            afs_entries: Vec::new(),
            open_pzz: Some(pzz),
            staged_pzz: BTreeMap::new(),
            expanded_pzz_entry: None,
            operations: vec![format!("Loaded PZZ {}", name)],
        })
    }

    pub fn open_afs_bytes(name: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let name = name.into();
        let inventory = afs::scan_inventory(&data, Some(name.clone()))?;
        let file_len = data.len() as u64;
        let afs_entries = inventory
            .entries
            .iter()
            .cloned()
            .map(|entry| entry_to_node(&entry, file_len))
            .collect::<Vec<_>>();
        let count = afs_entries.len();
        Ok(Self {
            afs_path: None,
            afs_bytes: Some(data),
            afs_entries,
            open_pzz: None,
            staged_pzz: BTreeMap::new(),
            expanded_pzz_entry: None,
            operations: vec![format!("Loaded AFS ({} entries)", count)],
        })
    }

    pub fn afs_entries(&self) -> &[AfsEntryNode] {
        &self.afs_entries
    }

    pub fn afs_path(&self) -> Option<&PathBuf> {
        self.afs_path.as_ref()
    }

    pub fn open_pzz(&self) -> Option<&PzzWorkspace> {
        self.open_pzz.as_ref()
    }

    pub fn open_pzz_mut(&mut self) -> Option<&mut PzzWorkspace> {
        self.open_pzz.as_mut()
    }

    pub fn expanded_pzz_entry(&self) -> Option<usize> {
        self.expanded_pzz_entry
    }

    pub fn staged_dirty_pzz_entry_count(&self) -> usize {
        self.staged_pzz
            .values()
            .filter(|pzz| pzz.is_dirty())
            .count()
    }

    pub fn is_pzz_entry_dirty(&self, entry_index: usize) -> bool {
        if self.expanded_pzz_entry == Some(entry_index) {
            return self.open_pzz.as_ref().is_some_and(PzzWorkspace::is_dirty);
        }
        self.staged_pzz
            .get(&entry_index)
            .is_some_and(PzzWorkspace::is_dirty)
    }

    pub fn dirty_pzz_entries(&self) -> Vec<(usize, &PzzWorkspace)> {
        let mut entries = self
            .staged_pzz
            .iter()
            .filter_map(|(entry_index, pzz)| pzz.is_dirty().then_some((*entry_index, pzz)))
            .collect::<Vec<_>>();
        if let Some(pzz) = self.open_pzz.as_ref() {
            if let Some(entry_index) = pzz.afs_entry_index() {
                if pzz.is_dirty() && !entries.iter().any(|(index, _)| *index == entry_index) {
                    entries.push((entry_index, pzz));
                }
            }
        }
        entries.sort_by_key(|(entry_index, _)| *entry_index);
        entries
    }

    pub fn dirty_pzz_entry_count(&self) -> usize {
        self.dirty_pzz_entries().len()
    }

    pub fn rebuild_dirty_pzz_entries_with<F>(
        &mut self,
        mut rebuild: F,
    ) -> Result<Vec<(usize, Vec<u8>)>>
    where
        F: FnMut(&mut PzzWorkspace) -> Result<Vec<u8>>,
    {
        let mut rebuilt = Vec::new();
        for (entry_index, pzz) in self.staged_pzz.iter_mut() {
            if pzz.is_dirty() {
                rebuilt.push((*entry_index, rebuild(pzz)?));
            }
        }
        if let Some(pzz) = self.open_pzz.as_mut() {
            if let Some(entry_index) = pzz.afs_entry_index() {
                if pzz.is_dirty() && !rebuilt.iter().any(|(index, _)| *index == entry_index) {
                    rebuilt.push((entry_index, rebuild(pzz)?));
                }
            }
        }
        rebuilt.sort_by_key(|(entry_index, _)| *entry_index);
        Ok(rebuilt)
    }

    pub fn operation_log(&self) -> &[String] {
        &self.operations
    }

    pub fn push_log(&mut self, msg: String) {
        self.operations.push(msg);
    }

    pub fn open_pzz_entry(&mut self, entry_index: usize) -> Result<()> {
        eprintln!("[workspace] open_pzz_entry: index={}", entry_index);
        if self.expanded_pzz_entry == Some(entry_index) {
            return Ok(());
        }
        let entry = self
            .afs_entries
            .iter()
            .find(|e| e.index == entry_index)
            .ok_or_else(|| anyhow::anyhow!("AFS entry {} not found", entry_index))?;
        eprintln!(
            "[workspace]   entry: name={:?}, kind={:?}, validation={:?}, offset=0x{:X}, size={}",
            entry.name, entry.kind, entry.validation, entry.offset, entry.size
        );
        if entry.validation != EntryValidation::Ok {
            bail!(
                "AFS entry {} has validation issue: {:?}",
                entry_index,
                entry.validation
            );
        }
        let name = entry.name.clone();
        let offset = entry.offset;
        let size = entry.size;
        let pzz_ws = if let Some(pzz) = self.staged_pzz.remove(&entry_index) {
            eprintln!("[workspace]   restoring staged PZZ entry {}", entry_index);
            pzz
        } else {
            let data = if let Some(afs_path) = self.afs_path.as_ref() {
                eprintln!(
                    "[workspace]   reading from file: offset=0x{:X}, size={}",
                    offset, size
                );
                afs::read_entry_from_file(afs_path, offset, size)?
            } else if let Some(afs_bytes) = self.afs_bytes.as_ref() {
                let end = offset
                    .checked_add(size)
                    .ok_or_else(|| anyhow::anyhow!("entry size overflow"))?;
                if end > afs_bytes.len() {
                    bail!("AFS entry {} exceeds data bounds", entry_index);
                }
                afs_bytes[offset..end].to_vec()
            } else {
                bail!("no AFS source available");
            };
            eprintln!(
                "[workspace]   raw data read: {} bytes, first_4={:02X?}",
                data.len(),
                &data[..data.len().min(4)]
            );
            PzzWorkspace::new(name.clone(), Some(entry_index), data)?
        };
        eprintln!(
            "[workspace]   PZZ opened: {} streams",
            pzz_ws.streams().len()
        );
        for (i, node) in pzz_ws.streams().iter().enumerate() {
            eprintln!(
                "[workspace]     stream[{}]: name={:?}, kind={:?}, size={}",
                i, node.name, node.kind, node.size
            );
        }
        self.stash_open_pzz_if_dirty();
        self.open_pzz = Some(pzz_ws);
        self.expanded_pzz_entry = Some(entry_index);
        self.operations
            .push(format!("Opened AFS entry {} as {}", entry_index, name));
        Ok(())
    }

    pub fn close_open_pzz(&mut self) {
        eprintln!(
            "[workspace] close_open_pzz (was entry {:?})",
            self.expanded_pzz_entry
        );
        self.stash_open_pzz_if_dirty();
        self.expanded_pzz_entry = None;
    }

    pub fn replace_stream(&mut self, index: usize, data: Vec<u8>) -> Result<()> {
        let pzz = self
            .open_pzz
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no PZZ is open"))?;
        pzz.replace_stream(index, data)?;
        self.operations.push(format!("Replaced stream{:03}", index));
        Ok(())
    }

    fn stash_open_pzz_if_dirty(&mut self) {
        let Some(pzz) = self.open_pzz.take() else {
            return;
        };
        if let Some(entry_index) = pzz.afs_entry_index() {
            if pzz.is_dirty() {
                self.staged_pzz.insert(entry_index, pzz);
                return;
            }
        }
        self.open_pzz = None;
    }
}

impl PzzWorkspace {
    pub fn new(name: String, afs_entry_index: Option<usize>, original: Vec<u8>) -> Result<Self> {
        eprintln!(
            "[pzz_ws] PzzWorkspace::new: name={:?}, afs_entry={:?}, raw_len={}",
            name,
            afs_entry_index,
            original.len()
        );
        let result = pzz::extract_pzz_streams_strict(&original);
        match &result {
            Ok(pzz_streams) => {
                eprintln!(
                    "[pzz_ws]   extract_pzz_streams_strict OK: {} streams, key=0x{:08X}",
                    pzz_streams.streams.len(),
                    pzz_streams.info.key
                );
                eprintln!(
                    "[pzz_ws]   info: descriptors={}, chunks={}, has_tail={}",
                    pzz_streams.info.descriptor_count,
                    pzz_streams.info.chunk_count,
                    pzz_streams.info.has_tail
                );
                for (i, s) in pzz_streams.streams.iter().enumerate() {
                    let kind = pzz::classify_stream(s);
                    let magic_hex = if s.len() >= 4 {
                        format!("{:02X} {:02X} {:02X} {:02X}", s[0], s[1], s[2], s[3])
                    } else {
                        format!("{:02X?}", &s[..s.len().min(4)])
                    };
                    eprintln!(
                        "[pzz_ws]     stream[{}]: size={}, classify={:?}, magic=[{}]",
                        i,
                        s.len(),
                        kind,
                        magic_hex
                    );
                }
            }
            Err(e) => {
                eprintln!("[pzz_ws]   extract_pzz_streams_strict FAILED: {}", e);
            }
        }
        let streams = result?.streams;
        let stream_nodes = streams
            .iter()
            .enumerate()
            .map(|(index, data)| stream_to_node(index, None, data, false))
            .collect();
        Ok(Self {
            name,
            afs_entry_index,
            original,
            streams,
            stream_nodes,
            revision: 0,
            cached_rebuild: None,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn original(&self) -> &[u8] {
        &self.original
    }

    pub fn afs_entry_index(&self) -> Option<usize> {
        self.afs_entry_index
    }

    pub fn stream_data(&self) -> &[Vec<u8>] {
        &self.streams
    }

    pub fn streams(&self) -> &[StreamNode] {
        &self.stream_nodes
    }

    pub fn dirty_stream_indices(&self) -> Vec<usize> {
        self.stream_nodes
            .iter()
            .filter_map(|stream| stream.dirty.then_some(stream.index))
            .collect()
    }

    pub fn is_dirty(&self) -> bool {
        self.stream_nodes.iter().any(|s| s.dirty)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cached_rebuild_payload(&self) -> Option<&[u8]> {
        self.cached_rebuild
            .as_ref()
            .filter(|cache| cache.revision == self.revision)
            .map(|cache| cache.payload.as_slice())
    }

    pub fn cached_rebuild_size(&self) -> Option<usize> {
        self.cached_rebuild_payload().map(|payload| payload.len())
    }

    pub fn store_cached_rebuild(&mut self, payload: Vec<u8>) {
        self.cached_rebuild = Some(CachedPzzRebuild {
            revision: self.revision,
            payload,
        });
    }

    pub fn replace_stream(&mut self, index: usize, data: Vec<u8>) -> Result<()> {
        if index >= self.streams.len() {
            bail!("stream index {} out of range", index);
        }
        self.streams[index] = data;
        self.stream_nodes[index] = stream_to_node(index, None, &self.streams[index], true);
        self.revision = self.revision.wrapping_add(1);
        self.cached_rebuild = None;
        Ok(())
    }
}

fn entry_to_node(entry: &AfsEntry, file_len: u64) -> AfsEntryNode {
    let name = entry
        .name
        .clone()
        .unwrap_or_else(|| format!("entry{:04}.bin", entry.index));
    let validation = if entry.offset == 0 && entry.size == 0 {
        EntryValidation::Empty
    } else if entry
        .offset
        .checked_add(entry.size)
        .is_none_or(|end| end as u64 > file_len)
    {
        EntryValidation::ExceedsBounds
    } else {
        EntryValidation::Ok
    };
    let kind = if name.to_ascii_lowercase().ends_with(".pzz") {
        AssetKind::Pzz
    } else {
        AssetKind::Raw
    };
    AfsEntryNode {
        index: entry.index,
        name,
        offset: entry.offset,
        size: entry.size,
        kind,
        validation,
    }
}

fn stream_to_node(index: usize, name: Option<String>, data: &[u8], dirty: bool) -> StreamNode {
    let kind = stream_kind(data);
    let extension = match kind {
        AssetKind::Pmf2 => "pmf2",
        AssetKind::Gim => "gim",
        AssetKind::Sad => "sad",
        _ => "bin",
    };
    StreamNode {
        index,
        name: name.unwrap_or_else(|| format!("stream{:03}.{}", index, extension)),
        size: data.len(),
        kind,
        dirty,
    }
}

fn stream_kind(data: &[u8]) -> AssetKind {
    match pzz::classify_stream(data) {
        "pmf2" => AssetKind::Pmf2,
        "gim" => AssetKind::Gim,
        "sad" => AssetKind::Sad,
        _ => AssetKind::Raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u32_le(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn align_up(value: usize, alignment: usize) -> usize {
        (value + alignment - 1) & !(alignment - 1)
    }

    fn build_afs(entries: &[Vec<u8>]) -> Vec<u8> {
        let file_count = entries.len();
        let mut offsets = Vec::with_capacity(file_count);
        let mut cursor = 0x800usize;
        for entry in entries {
            offsets.push(cursor);
            cursor += align_up(entry.len(), 2048);
        }

        let mut data = vec![0u8; cursor];
        data[0..4].copy_from_slice(b"AFS\0");
        write_u32_le(&mut data, 4, file_count as u32);
        for (index, entry) in entries.iter().enumerate() {
            let table_pos = 8 + index * 8;
            write_u32_le(&mut data, table_pos, offsets[index] as u32);
            write_u32_le(&mut data, table_pos + 4, entry.len() as u32);
            let offset = offsets[index];
            data[offset..offset + entry.len()].copy_from_slice(entry);
        }
        let name_table_pos = 8 + file_count * 8;
        write_u32_le(&mut data, name_table_pos, 0);
        write_u32_le(&mut data, name_table_pos + 4, 0);
        data
    }

    #[test]
    fn switching_pzz_entries_stages_dirty_workspace_and_restores_it() {
        let pzz0 = pzz::build_pzz(&[b"PMF2_entry0".to_vec()], 0x1234_5678);
        let pzz1 = pzz::build_pzz(&[b"PMF2_entry1".to_vec()], 0x1234_5678);
        let afs = build_afs(&[pzz0, pzz1]);
        let mut workspace = ModWorkspace::open_afs_bytes("test.bin", afs).unwrap();

        workspace.open_pzz_entry(0).unwrap();
        workspace
            .replace_stream(0, b"PMF2_entry0_modified".to_vec())
            .unwrap();
        workspace.open_pzz_entry(1).unwrap();

        assert_eq!(workspace.expanded_pzz_entry(), Some(1));
        assert_eq!(workspace.staged_dirty_pzz_entry_count(), 1);
        assert!(workspace.is_pzz_entry_dirty(0));
        assert!(!workspace.is_pzz_entry_dirty(1));

        workspace.open_pzz_entry(0).unwrap();
        let open = workspace.open_pzz().unwrap();
        assert_eq!(open.stream_data()[0], b"PMF2_entry0_modified");
        assert!(open.is_dirty());
        assert_eq!(workspace.staged_dirty_pzz_entry_count(), 0);
    }

    #[test]
    fn cached_rebuild_is_reused_until_stream_changes() {
        let pzz = pzz::build_pzz(&[b"PMF2_entry0".to_vec()], 0x1234_5678);
        let mut workspace = ModWorkspace::open_pzz_bytes("test.pzz", pzz).unwrap();
        workspace
            .replace_stream(0, b"PMF2_entry0_modified".to_vec())
            .unwrap();

        let open = workspace.open_pzz_mut().unwrap();
        let first = crate::save::rebuild_pzz_payload_cached(open).unwrap();
        assert_eq!(open.cached_rebuild_size(), Some(first.len()));

        let second = crate::save::rebuild_pzz_payload_cached(open).unwrap();
        assert_eq!(second, first);

        open.replace_stream(0, b"PMF2_entry0_modified_again".to_vec())
            .unwrap();
        assert_eq!(open.cached_rebuild_size(), None);
    }
}
