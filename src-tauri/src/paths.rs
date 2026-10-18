use std::path::PathBuf;

/// Корень данных лаунчера. Можно переопределить переменной T1RA_HOME (портативный режим).
pub fn root() -> PathBuf {
    if let Ok(p) = std::env::var("T1RA_HOME") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("T1raLauncher")
}

pub fn libraries() -> PathBuf {
    root().join("libraries")
}
pub fn assets() -> PathBuf {
    root().join("assets")
}
pub fn versions() -> PathBuf {
    root().join("versions")
}
pub fn meta() -> PathBuf {
    root().join("meta")
}
pub fn instances() -> PathBuf {
    root().join("instances")
}
pub fn settings_file() -> PathBuf {
    root().join("launcher.json")
}

pub fn ensure_all() -> std::io::Result<()> {
    for p in [libraries(), assets(), versions(), meta(), instances()] {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}
