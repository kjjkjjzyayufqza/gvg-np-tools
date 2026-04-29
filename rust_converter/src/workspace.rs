use crate::{
    afs::{self, AfsEntry, AfsInventory},
    pzz,
};
use anyhow::{bail, Result};
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
}

#[derive(Clone, Debug, Default)]
pub struct ModWorkspace {
    afs_path: Option<PathBuf>,
    afs_bytes: Option<Vec<u8>>,
    afs_file_len: u64,
    inventory: Option<AfsInventory>,
    afs_entries: Vec<AfsEntryNode>,
    open_pzz: Option<PzzWorkspace>,
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
        let pzz_count = afs_entries.iter().filter(|e| e.kind == AssetKind::Pzz).count();
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
            afs_file_len: file_len,
            inventory: Some(inventory),
            afs_entries,
            open_pzz: None,
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
            afs_file_len: 0,
            inventory: None,
            afs_entries: Vec::new(),
            open_pzz: Some(pzz),
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
            afs_file_len: file_len,
            inventory: Some(inventory),
            afs_entries,
            open_pzz: None,
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

    pub fn expanded_pzz_entry(&self) -> Option<usize> {
        self.expanded_pzz_entry
    }

    pub fn operation_log(&self) -> &[String] {
        &self.operations
    }

    pub fn push_log(&mut self, msg: String) {
        self.operations.push(msg);
    }

    pub fn open_pzz_entry(&mut self, entry_index: usize) -> Result<()> {
        eprintln!("[workspace] open_pzz_entry: index={}", entry_index);
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
        let data = if let Some(afs_path) = self.afs_path.as_ref() {
            eprintln!("[workspace]   reading from file: offset=0x{:X}, size={}", offset, size);
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
        eprintln!("[workspace]   raw data read: {} bytes, first_4={:02X?}", data.len(), &data[..data.len().min(4)]);
        let pzz_ws = PzzWorkspace::new(name.clone(), Some(entry_index), data)?;
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
        self.open_pzz = Some(pzz_ws);
        self.expanded_pzz_entry = Some(entry_index);
        self.operations
            .push(format!("Opened AFS entry {} as {}", entry_index, name));
        Ok(())
    }

    pub fn close_open_pzz(&mut self) {
        eprintln!("[workspace] close_open_pzz (was entry {:?})", self.expanded_pzz_entry);
        self.open_pzz = None;
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
}

impl PzzWorkspace {
    pub fn new(name: String, afs_entry_index: Option<usize>, original: Vec<u8>) -> Result<Self> {
        eprintln!("[pzz_ws] PzzWorkspace::new: name={:?}, afs_entry={:?}, raw_len={}", name, afs_entry_index, original.len());
        let result = pzz::extract_pzz_streams_strict(&original);
        match &result {
            Ok(pzz_streams) => {
                eprintln!("[pzz_ws]   extract_pzz_streams_strict OK: {} streams, key=0x{:08X}", pzz_streams.streams.len(), pzz_streams.info.key);
                eprintln!("[pzz_ws]   info: descriptors={}, chunks={}, has_tail={}", pzz_streams.info.descriptor_count, pzz_streams.info.chunk_count, pzz_streams.info.has_tail);
                for (i, s) in pzz_streams.streams.iter().enumerate() {
                    let kind = pzz::classify_stream(s);
                    let magic_hex = if s.len() >= 4 {
                        format!("{:02X} {:02X} {:02X} {:02X}", s[0], s[1], s[2], s[3])
                    } else {
                        format!("{:02X?}", &s[..s.len().min(4)])
                    };
                    eprintln!("[pzz_ws]     stream[{}]: size={}, classify={:?}, magic=[{}]", i, s.len(), kind, magic_hex);
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

    pub fn is_dirty(&self) -> bool {
        self.stream_nodes.iter().any(|s| s.dirty)
    }

    pub fn replace_stream(&mut self, index: usize, data: Vec<u8>) -> Result<()> {
        if index >= self.streams.len() {
            bail!("stream index {} out of range", index);
        }
        self.streams[index] = data;
        self.stream_nodes[index] = stream_to_node(index, None, &self.streams[index], true);
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
