use crate::{
    afs::{self, AfsEntry, AfsInventory},
    pzz,
};
use anyhow::{bail, Result};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    AfsEntry,
    Pzz,
    Pmf2,
    Gim,
    Sad,
    Raw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AfsEntryNode {
    pub index: usize,
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub kind: AssetKind,
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
    source_path: Option<PathBuf>,
    afs_data: Option<Vec<u8>>,
    inventory: Option<AfsInventory>,
    afs_entries: Vec<AfsEntryNode>,
    open_pzz: Option<PzzWorkspace>,
    operations: Vec<String>,
}

impl ModWorkspace {
    pub fn from_inventory(inventory: AfsInventory) -> Self {
        let afs_entries = inventory
            .entries
            .iter()
            .cloned()
            .map(|entry| entry_to_node(entry, None))
            .collect::<Vec<_>>();
        Self {
            source_path: inventory.file.clone().map(PathBuf::from),
            afs_data: None,
            inventory: Some(inventory),
            afs_entries,
            open_pzz: None,
            operations: Vec::new(),
        }
    }

    pub fn open_afs_bytes(name: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let name = name.into();
        let inventory = afs::scan_inventory(&data, Some(name.clone()))?;
        let afs_entries = inventory
            .entries
            .iter()
            .cloned()
            .map(|entry| {
                let payload = data.get(entry.offset..entry.offset + entry.size);
                entry_to_node(entry, payload)
            })
            .collect::<Vec<_>>();
        Ok(Self {
            source_path: Some(PathBuf::from(name)),
            afs_data: Some(data),
            inventory: Some(inventory),
            afs_entries,
            open_pzz: None,
            operations: Vec::new(),
        })
    }

    pub fn open_pzz_bytes(name: impl Into<String>, data: Vec<u8>) -> Result<Self> {
        let pzz = PzzWorkspace::new(name.into(), None, data)?;
        let mut workspace = Self::default();
        workspace
            .operations
            .push(format!("Opened PZZ {}", pzz.name));
        workspace.open_pzz = Some(pzz);
        Ok(workspace)
    }

    pub fn afs_entries(&self) -> &[AfsEntryNode] {
        &self.afs_entries
    }

    pub fn source_path(&self) -> Option<&PathBuf> {
        self.source_path.as_ref()
    }

    pub fn open_pzz(&self) -> Option<&PzzWorkspace> {
        self.open_pzz.as_ref()
    }

    pub fn open_pzz_mut(&mut self) -> Option<&mut PzzWorkspace> {
        self.open_pzz.as_mut()
    }

    pub fn close_open_pzz(&mut self) {
        self.open_pzz = None;
    }

    pub fn operation_log(&self) -> &[String] {
        &self.operations
    }

    pub fn open_pzz_entry(&mut self, entry_index: usize) -> Result<()> {
        let data = self
            .afs_data
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace was not opened from AFS bytes"))?;
        let inventory = self
            .inventory
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("workspace has no AFS inventory"))?;
        let entry = inventory
            .entries
            .iter()
            .find(|entry| entry.index == entry_index)
            .ok_or_else(|| anyhow::anyhow!("AFS entry {} not found", entry_index))?;
        let name = entry
            .name
            .clone()
            .unwrap_or_else(|| format!("entry{:04}.pzz", entry.index));
        if !name.to_ascii_lowercase().ends_with(".pzz") {
            bail!("AFS entry {} is not a PZZ file", entry_index);
        }
        let end = entry
            .offset
            .checked_add(entry.size)
            .ok_or_else(|| anyhow::anyhow!("AFS entry size overflows"))?;
        if end > data.len() {
            bail!("AFS entry {} exceeds source data", entry_index);
        }
        self.open_pzz = Some(PzzWorkspace::new(
            name.clone(),
            Some(entry.index),
            data[entry.offset..end].to_vec(),
        )?);
        self.operations
            .push(format!("Opened AFS entry {} as {}", entry_index, name));
        Ok(())
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

    pub fn add_stream(&mut self, name: impl Into<String>, data: Vec<u8>) -> Result<()> {
        let pzz = self
            .open_pzz
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no PZZ is open"))?;
        let name = name.into();
        pzz.add_stream(name.clone(), data)?;
        self.operations.push(format!("Added stream {}", name));
        Ok(())
    }
}

impl PzzWorkspace {
    pub fn new(name: String, afs_entry_index: Option<usize>, original: Vec<u8>) -> Result<Self> {
        let streams = pzz::extract_pzz_streams_strict(&original)?.streams;
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

    pub fn add_stream(&mut self, name: String, data: Vec<u8>) -> Result<()> {
        let index = self.streams.len();
        self.streams.push(data);
        self.stream_nodes.push(stream_to_node(
            index,
            Some(name),
            &self.streams[index],
            true,
        ));
        Ok(())
    }
}

fn entry_to_node(entry: AfsEntry, payload: Option<&[u8]>) -> AfsEntryNode {
    let name = entry
        .name
        .unwrap_or_else(|| format!("entry{:04}.bin", entry.index));
    let kind = if name.to_ascii_lowercase().ends_with(".pzz")
        || payload.map(is_pzz_payload).unwrap_or(false)
    {
        AssetKind::Pzz
    } else {
        AssetKind::AfsEntry
    };
    AfsEntryNode {
        index: entry.index,
        name,
        offset: entry.offset,
        size: entry.size,
        kind,
    }
}

fn is_pzz_payload(data: &[u8]) -> bool {
    pzz::inspect_pzz(data).is_ok()
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
