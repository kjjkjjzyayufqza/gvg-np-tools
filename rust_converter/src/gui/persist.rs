//! Persists GUI file-dialog paths and recent AFS opens under the OS config directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const RECENT_AFS_MAX: usize = 20;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct PersistedGuiState {
    #[serde(default)]
    pub last_dir_open_afs: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_open_pzz: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_save_pzz_as: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_patch_afs_entry: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_write_modified_pzz_to_afs: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_cwcheat: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_export_entry_raw: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_export_stream_raw: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_export_stream_dae: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_replace_stream_dae: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_replace_stream_pmf2: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_export_stream_png: Option<PathBuf>,
    #[serde(default)]
    pub last_dir_replace_stream_png: Option<PathBuf>,
    #[serde(default)]
    pub recent_afs_paths: Vec<PathBuf>,
}

pub(crate) fn state_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gvg_converter").join("gui_state.json"))
}

pub(crate) fn load() -> PersistedGuiState {
    let Some(path) = state_file_path() else {
        return PersistedGuiState::default();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return PersistedGuiState::default();
    };
    serde_json::from_slice::<PersistedGuiState>(&bytes).unwrap_or_default()
}

pub(crate) fn save(state: &PersistedGuiState) -> std::io::Result<()> {
    let Some(path) = state_file_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no config directory for this platform",
        ));
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.partial");
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
