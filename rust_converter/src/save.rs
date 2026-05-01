use crate::{
    afs::{AfsInventory, AfsPatchPlan},
    pzz,
    workspace::PzzWorkspace,
};
use anyhow::{bail, Result};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct PzzSavePlan {
    pub original_size: usize,
    pub rebuilt_size: usize,
    pub stream_count: usize,
    pub changed_stream_count: usize,
    pub tail_recomputed: bool,
    pub rebuilt_tail: Option<[u8; 16]>,
    pub decrypted_body: Vec<u8>,
    pub rebuilt_pzz: Vec<u8>,
    pub validation_messages: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PzzSavePlanner {
    original_pzz: Vec<u8>,
    streams: Vec<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct InventoryAfsSavePlan {
    pub entry_index: usize,
    pub old_size: usize,
    pub new_size: usize,
    pub old_aligned_size: usize,
    pub new_aligned_size: usize,
    pub delta_aligned: isize,
    pub validation_messages: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AfsSavePlanner {
    inventory: AfsInventory,
    entry_index: usize,
    new_size: usize,
}

impl PzzSavePlanner {
    pub fn new(original_pzz: &[u8], streams: Vec<Vec<u8>>) -> Self {
        Self {
            original_pzz: original_pzz.to_vec(),
            streams,
        }
    }

    pub fn plan_preserving_layout(&self) -> Result<PzzSavePlan> {
        let started = Instant::now();
        let info = pzz::inspect_pzz(&self.original_pzz)?;
        if info.stream_count != self.streams.len() {
            bail!(
                "preserving PZZ layout requires {} streams, got {}",
                info.stream_count,
                self.streams.len()
            );
        }
        let original_streams = pzz::extract_pzz_streams(&self.original_pzz);
        let changed_stream_count = original_streams
            .iter()
            .zip(self.streams.iter())
            .filter(|(old, new)| old.as_slice() != new.as_slice())
            .count();
        let rebuilt = pzz::rebuild_pzz_from_original_result(&self.original_pzz, &self.streams)
            .ok_or_else(|| anyhow::anyhow!("failed to rebuild PZZ from original layout"))?;
        let mut validation_messages = Vec::new();
        validation_messages.push(format!("{} streams validated", self.streams.len()));
        if info.has_tail {
            validation_messages.push("PZZ 16-byte tail will be recomputed".to_string());
        }
        eprintln!(
            "[save] plan_preserving_layout: streams={}, changed={}, rebuilt_size={}, elapsed_ms={}",
            self.streams.len(),
            changed_stream_count,
            rebuilt.raw.len(),
            started.elapsed().as_millis()
        );
        Ok(PzzSavePlan {
            original_size: self.original_pzz.len(),
            rebuilt_size: rebuilt.raw.len(),
            stream_count: self.streams.len(),
            changed_stream_count,
            tail_recomputed: info.has_tail,
            rebuilt_tail: rebuilt.tail,
            decrypted_body: rebuilt.decrypted_body,
            rebuilt_pzz: rebuilt.raw,
            validation_messages,
        })
    }

    pub fn plan_stream_archive_rebuild(&self) -> Result<PzzSavePlan> {
        let started = Instant::now();
        let info = pzz::inspect_pzz(&self.original_pzz)?;
        let original_streams = pzz::extract_pzz_streams(&self.original_pzz);
        let changed_stream_count = self
            .streams
            .iter()
            .enumerate()
            .filter(|(index, stream)| {
                original_streams
                    .get(*index)
                    .is_none_or(|old| old.as_slice() != stream.as_slice())
            })
            .count();
        let rebuilt = pzz::rebuild_stream_archive_with_original_key_result(
            &self.original_pzz,
            &self.streams,
        )?;
        let mut validation_messages = Vec::new();
        validation_messages.push(format!("{} streams rebuilt", self.streams.len()));
        if info.has_tail {
            validation_messages.push("PZZ 16-byte tail will be recomputed".to_string());
        }
        eprintln!(
            "[save] plan_stream_archive_rebuild: streams={}, changed={}, rebuilt_size={}, elapsed_ms={}",
            self.streams.len(),
            changed_stream_count,
            rebuilt.raw.len(),
            started.elapsed().as_millis()
        );
        Ok(PzzSavePlan {
            original_size: self.original_pzz.len(),
            rebuilt_size: rebuilt.raw.len(),
            stream_count: self.streams.len(),
            changed_stream_count,
            tail_recomputed: info.has_tail,
            rebuilt_tail: rebuilt.tail,
            decrypted_body: rebuilt.decrypted_body,
            rebuilt_pzz: rebuilt.raw,
            validation_messages,
        })
    }
}

pub fn rebuild_pzz_payload(pzz: &PzzWorkspace) -> Result<Vec<u8>> {
    let started = Instant::now();
    let planner = PzzSavePlanner::new(pzz.original(), pzz.stream_data().to_vec());
    let original_stream_count = pzz::inspect_pzz(pzz.original())?.stream_count;
    let rebuilt = if original_stream_count == pzz.stream_data().len() {
        planner.plan_preserving_layout()?.rebuilt_pzz
    } else {
        planner.plan_stream_archive_rebuild()?.rebuilt_pzz
    };
    eprintln!(
        "[save] rebuild_pzz_payload: name={}, size={}, elapsed_ms={}",
        pzz.name(),
        rebuilt.len(),
        started.elapsed().as_millis()
    );
    Ok(rebuilt)
}

pub fn rebuild_pzz_payload_cached(pzz: &mut PzzWorkspace) -> Result<Vec<u8>> {
    if let Some(payload) = pzz.cached_rebuild_payload() {
        eprintln!(
            "[save] rebuild_pzz_payload_cached: name={}, size={}, cache=hit",
            pzz.name(),
            payload.len()
        );
        return Ok(payload.to_vec());
    }
    let started = Instant::now();
    let original_stream_count = pzz::inspect_pzz(pzz.original())?.stream_count;
    let dirty_stream_indices = pzz.dirty_stream_indices();
    let rebuilt = if original_stream_count == pzz.stream_data().len() {
        pzz::rebuild_pzz_from_original_dirty_result(
            pzz.original(),
            pzz.stream_data(),
            &dirty_stream_indices,
        )
        .ok_or_else(|| anyhow::anyhow!("failed to rebuild PZZ from original layout"))?
        .raw
    } else {
        pzz::rebuild_stream_archive_with_original_key_result(pzz.original(), pzz.stream_data())?.raw
    };
    pzz.store_cached_rebuild(rebuilt.clone());
    eprintln!(
        "[save] rebuild_pzz_payload_cached: name={}, dirty_streams={}, size={}, cache=miss, elapsed_ms={}",
        pzz.name(),
        dirty_stream_indices.len(),
        rebuilt.len(),
        started.elapsed().as_millis()
    );
    Ok(rebuilt)
}

impl AfsSavePlanner {
    pub fn new(inventory: AfsInventory, entry_index: usize, new_size: usize) -> Self {
        Self {
            inventory,
            entry_index,
            new_size,
        }
    }

    pub fn plan(&self) -> Result<InventoryAfsSavePlan> {
        let entry = self
            .inventory
            .entries
            .iter()
            .find(|entry| entry.index == self.entry_index)
            .ok_or_else(|| anyhow::anyhow!("AFS entry {} not found", self.entry_index))?;
        let old_aligned_size = align_up(entry.size, 2048);
        let new_aligned_size = align_up(self.new_size, 2048);
        let delta_aligned = new_aligned_size as isize - old_aligned_size as isize;
        let mut validation_messages = Vec::new();
        validation_messages.push("AFS entry data will be 2048-byte aligned".to_string());
        validation_messages.push("AFS main table size will be updated".to_string());
        validation_messages
            .push("AFS name table size mirror will be updated when present".to_string());
        if delta_aligned != 0 {
            validation_messages.push(format!(
                "Following entries shift by {} aligned bytes",
                delta_aligned
            ));
        }
        Ok(InventoryAfsSavePlan {
            entry_index: self.entry_index,
            old_size: entry.size,
            new_size: self.new_size,
            old_aligned_size,
            new_aligned_size,
            delta_aligned,
            validation_messages,
        })
    }
}

impl From<AfsPatchPlan> for InventoryAfsSavePlan {
    fn from(plan: AfsPatchPlan) -> Self {
        Self {
            entry_index: plan.entry_index,
            old_size: plan.old_size,
            new_size: plan.new_size,
            old_aligned_size: plan.old_aligned_size,
            new_aligned_size: plan.new_aligned_size,
            delta_aligned: plan.delta_aligned,
            validation_messages: vec![
                "AFS entry data will be 2048-byte aligned".to_string(),
                "AFS main table size will be updated".to_string(),
                "AFS name table size mirror will be updated when present".to_string(),
            ],
        }
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        value
    } else {
        (value + alignment - 1) & !(alignment - 1)
    }
}
