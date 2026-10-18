use crate::paths;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    /// "vanilla" | "fabric" | "quilt"
    pub loader: String,
    /// id версии, которую реально запускаем (для загрузчиков заполняется при установке)
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub ram_mb: Option<u32>,
    #[serde(default)]
    pub created: u64,
}

fn safe_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow!("некорректный id сборки"));
    }
    Ok(())
}

pub fn dir(id: &str) -> PathBuf {
    paths::instances().join(id)
}

pub fn game_dir(id: &str) -> PathBuf {
    dir(id).join(".minecraft")
}

pub fn load(id: &str) -> Result<Instance> {
    safe_id(id)?;
    let s = std::fs::read_to_string(dir(id).join("instance.json"))
        .map_err(|_| anyhow!("сборка «{id}» не найдена"))?;
    Ok(serde_json::from_str(&s)?)
}

pub fn save(i: &Instance) -> Result<()> {
    safe_id(&i.id)?;
    std::fs::create_dir_all(game_dir(&i.id))?;
    std::fs::write(dir(&i.id).join("instance.json"), serde_json::to_vec_pretty(i)?)?;
    Ok(())
}

pub fn list() -> Vec<Instance> {
    let mut v: Vec<Instance> = std::fs::read_dir(paths::instances())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("instance.json")).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    v.sort_by_key(|i| i.created);
    v
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "instance".into()
    } else {
        s
    }
}

pub fn create(name: &str, mc_version: &str, loader: &str) -> Result<Instance> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("введи название сборки"));
    }
    if mc_version.trim().is_empty() {
        return Err(anyhow!("выбери версию Minecraft"));
    }
    if !["vanilla", "fabric", "quilt"].contains(&loader) {
        return Err(anyhow!("неизвестный загрузчик: {loader}"));
    }
    let base = slug(name);
    let mut id = base.clone();
    let mut n = 2;
    while dir(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let inst = Instance {
        id,
        name: name.to_string(),
        mc_version: mc_version.trim().to_string(),
        loader: loader.to_string(),
        version_id: String::new(),
        ram_mb: None,
        created,
    };
    save(&inst)?;
    Ok(inst)
}

pub fn delete(id: &str) -> Result<()> {
    safe_id(id)?;
    std::fs::remove_dir_all(dir(id))?;
    Ok(())
}
