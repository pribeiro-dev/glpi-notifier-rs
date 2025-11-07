use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Persisted state between runs (ids of already-notified tickets).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SeenState {
    pub seen_ticket_ids: BTreeSet<i64>,
}

fn state_path() -> Option<PathBuf> {
    let dir = dirs::data_dir()?;
    let p = dir.join("GlpiNotifier").join("state.json");
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    Some(p)
}

pub fn load_state() -> anyhow::Result<SeenState> {
    if let Some(p) = state_path() {
        if p.exists() {
            let data = fs::read(p)?;
            let st: SeenState = serde_json::from_slice(&data)?;
            return Ok(st);
        }
    }
    Ok(SeenState::default())
}

pub fn save_state(st: &SeenState) -> anyhow::Result<()> {
    if let Some(p) = state_path() {
        let data = serde_json::to_vec_pretty(st)?;
        fs::write(p, data)?;
    }
    Ok(())
}
