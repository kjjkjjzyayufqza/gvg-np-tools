use crate::{
    afs::{AfsInventory, AfsPatchPlan},
    pzz,
};
use anyhow::{bail, Result};

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
        let rebuilt_pzz = pzz::rebuild_pzz_from_original(&self.original_pzz, &self.streams)
            .ok_or_else(|| anyhow::anyhow!("failed to rebuild PZZ from original layout"))?;
        let decrypted_body = pzz::decrypt_pzz_body(&rebuilt_pzz)?;
        let rebuilt_tail = if info.has_tail {
            Some(pzz::compute_pzz_tail(&decrypted_body))
        } else {
            None
        };
        let mut validation_messages = Vec::new();
        validation_messages.push(format!("{} streams validated", self.streams.len()));
        if info.has_tail {
            validation_messages.push("PZZ 16-byte tail will be recomputed".to_string());
        }
        Ok(PzzSavePlan {
            original_size: self.original_pzz.len(),
            rebuilt_size: rebuilt_pzz.len(),
            stream_count: self.streams.len(),
            changed_stream_count,
            tail_recomputed: info.has_tail,
            rebuilt_tail,
            decrypted_body,
            rebuilt_pzz,
            validation_messages,
        })
    }

    pub fn plan_stream_archive_rebuild(&self) -> Result<PzzSavePlan> {
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
        let rebuilt_pzz =
            pzz::rebuild_stream_archive_with_original_key(&self.original_pzz, &self.streams)?;
        let decrypted_body = pzz::decrypt_pzz_body(&rebuilt_pzz)?;
        let rebuilt_tail = if info.has_tail {
            Some(pzz::compute_pzz_tail(&decrypted_body))
        } else {
            None
        };
        let mut validation_messages = Vec::new();
        validation_messages.push(format!("{} streams rebuilt", self.streams.len()));
        if info.has_tail {
            validation_messages.push("PZZ 16-byte tail will be recomputed".to_string());
        }
        Ok(PzzSavePlan {
            original_size: self.original_pzz.len(),
            rebuilt_size: rebuilt_pzz.len(),
            stream_count: self.streams.len(),
            changed_stream_count,
            tail_recomputed: info.has_tail,
            rebuilt_tail,
            decrypted_body,
            rebuilt_pzz,
            validation_messages,
        })
    }
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
