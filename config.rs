use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub username: String,
    pub max_ram_mb: u32,
    pub java_path: String,
    pub java_args: String,
    pub use_mirror: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            username: "Player".into(),
            max_ram_mb: 4096,
            java_path: String::new(),
            java_args: "-XX:+UseG1GC -XX:+UnlockExperimentalVMOptions -XX:G1NewSizePercent=20 -XX:G1ReservePercent=20 -XX:MaxGCPauseMillis=50 -XX:G1HeapRegionSize=32M".into(),
            use_mirror: false,
        }
    }
}

pub fn load() -> Settings {
    std::fs::read_to_string(paths::settings_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<()> {
    let mut s = s.clone();
    s.username = s
        .username
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(16)
        .collect();
    if s.username.len() < 3 {
        s.username = "Player".into();
    }
    s.max_ram_mb = s.max_ram_mb.clamp(512, 65536);
    std::fs::create_dir_all(paths::root())?;
    std::fs::write(paths::settings_file(), serde_json::to_vec_pretty(&s)?)?;
    Ok(())
}
