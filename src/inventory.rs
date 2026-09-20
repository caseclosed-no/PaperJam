use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;

use crate::model::{InventoryFile, PrintPlan};

pub fn path(state_directory: &Path) -> PathBuf {
    state_directory.join("inventory.json")
}

pub fn load(state_directory: &Path) -> anyhow::Result<InventoryFile> {
    let path = path(state_directory);
    if !path.exists() {
        return Ok(InventoryFile::default());
    }
    let source =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(state_directory: &Path, inventory: &InventoryFile) -> anyhow::Result<()> {
    fs::create_dir_all(state_directory)
        .with_context(|| format!("creating {}", state_directory.display()))?;
    let final_path = path(state_directory);
    let temporary = state_directory.join("inventory.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(inventory)?)
        .with_context(|| format!("writing {}", temporary.display()))?;
    if final_path.exists() {
        fs::remove_file(&final_path)
            .with_context(|| format!("replacing {}", final_path.display()))?;
    }
    fs::rename(&temporary, &final_path)
        .with_context(|| format!("committing {}", final_path.display()))
}

pub fn consume(
    inventory: &mut InventoryFile,
    plan: &PrintPlan,
) -> anyhow::Result<std::collections::HashMap<String, u32>> {
    let mut consumed = std::collections::HashMap::new();
    for req in &plan.requirements {
        if req.consume_from_stock == 0 {
            continue;
        }
        let current = inventory.get(&plan.game_id, &req.element_id);
        anyhow::ensure!(
            current >= req.consume_from_stock,
            "inventory changed for '{}'",
            req.element_name
        );
        inventory
            .counts
            .entry(plan.game_id.clone())
            .or_default()
            .insert(req.element_id.clone(), current - req.consume_from_stock);
        consumed.insert(req.element_id.clone(), req.consume_from_stock);
    }
    if !consumed.is_empty() {
        inventory.revision += 1;
    }
    Ok(consumed)
}

pub fn restore(
    inventory: &mut InventoryFile,
    game_id: &str,
    consumed: &std::collections::HashMap<String, u32>,
) {
    if consumed.is_empty() {
        return;
    }
    let game = inventory.counts.entry(game_id.to_string()).or_default();
    for (element, amount) in consumed {
        *game.entry(element.clone()).or_default() += amount;
    }
    inventory.revision += 1;
}
