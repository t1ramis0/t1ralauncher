use crate::paths;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Проверка правил Mojang (rules). Правила с features (demo, разрешение и т.п.) считаются невыполненными.
pub fn rules_allow(rules: Option<&Value>) -> bool {
    let Some(arr) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    let mut allowed = false;
    for r in arr {
        let matches = if r.get("features").is_some() {
            false
        } else {
            match r.get("os") {
                None => true,
                Some(os) => {
                    let name_ok = os["name"].as_str().map_or(true, |n| n == os_name());
                    let arch_ok = os["arch"].as_str().map_or(true, |a| match a {
                        "x86" => cfg!(target_arch = "x86"),
                        "arm64" => cfg!(target_arch = "aarch64"),
                        _ => true,
                    });
                    // Правила с os.version в манифестах Mojang: Windows 10+ (разрешаем) и старые macOS 10.x (не подходят)
                    let ver_ok = os.get("version").is_none() || os_name() == "windows";
                    name_ok && arch_ok && ver_ok
                }
            }
        };
        if matches {
            allowed = r["action"].as_str() == Some("allow");
        }
    }
    allowed
}

#[derive(Clone, Debug)]
pub struct Lib {
    pub url: String,
    pub rel: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// true — это jar с нативными библиотеками (старый формат), его нужно распаковать.
    pub extract: bool,
}

fn maven_rel(name: &str) -> String {
    let mut p = name.split(':');
    let g = p.next().unwrap_or("").replace('.', "/");
    let a = p.next().unwrap_or("");
    let v = p.next().unwrap_or("");
    let file = match p.next() {
        Some(c) => format!("{a}-{v}-{c}.jar"),
        None => format!("{a}-{v}.jar"),
    };
    format!("{g}/{a}/{v}/{file}")
}

pub fn collect(json: &Value) -> Vec<Lib> {
    let mut out: Vec<Lib> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut seen_native: HashSet<String> = HashSet::new();

    for lib in json["libraries"].as_array().into_iter().flatten() {
        if !rules_allow(lib.get("rules")) {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");

        // Старый формат нативов: поле natives + downloads.classifiers
        if let Some(cl) = lib["natives"][os_name()].as_str() {
            let cl = cl.replace("${arch}", if cfg!(target_pointer_width = "32") { "32" } else { "64" });
            let a = &lib["downloads"]["classifiers"][cl.as_str()];
            if let Some(rel) = a["path"].as_str() {
                if seen_native.insert(rel.to_string()) {
                    out.push(Lib {
                        url: a["url"].as_str().unwrap_or("").to_string(),
                        rel: rel.to_string(),
                        sha1: a["sha1"].as_str().map(String::from),
                        size: a["size"].as_u64(),
                        extract: true,
                    });
                }
            }
        }

        let mut p = name.split(':');
        let g = p.next().unwrap_or("");
        let a = p.next().unwrap_or("");
        let _v = p.next();
        let c = p.next().unwrap_or("");
        let key = format!("{g}:{a}:{c}");

        let entry = if let Some(art) = lib["downloads"].get("artifact") {
            art["path"].as_str().map(|rel| Lib {
                url: art["url"].as_str().unwrap_or("").to_string(),
                rel: rel.to_string(),
                sha1: art["sha1"].as_str().map(String::from),
                size: art["size"].as_u64(),
                extract: false,
            })
        } else if lib.get("downloads").is_none() {
            // Формат Fabric/Quilt: только name + url репозитория
            let rel = maven_rel(name);
            let mut base = lib["url"]
                .as_str()
                .unwrap_or("https://libraries.minecraft.net/")
                .to_string();
            if !base.ends_with('/') {
                base.push('/');
            }
            Some(Lib {
                url: format!("{base}{rel}"),
                rel,
                sha1: lib["sha1"].as_str().map(String::from),
                size: lib["size"].as_u64(),
                extract: false,
            })
        } else {
            None
        };

        // Дубликаты по group:artifact:classifier — первым побеждает версия загрузчика (child идёт раньше parent).
        if let Some(l) = entry {
            if seen.insert(key) {
                out.push(l);
            }
        }
    }
    out
}

pub fn extract_natives(libs: &[Lib], dest: &Path) -> Result<()> {
    for l in libs.iter().filter(|l| l.extract) {
        let f = std::fs::File::open(paths::libraries().join(&l.rel))?;
        let mut z = zip::ZipArchive::new(f)?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_dir() {
                continue;
            }
            let Some(p) = e.enclosed_name().map(|p| p.to_path_buf()) else { continue };
            if p.starts_with("META-INF") {
                continue;
            }
            let Some(name) = p.file_name() else { continue };
            let target = dest.join(name);
            if std::fs::metadata(&target).map(|m| m.len() == e.size()).unwrap_or(false) {
                continue;
            }
            // Файл может быть занят запущенной игрой (Windows) — тогда оставляем как есть
            if let Ok(mut out) = std::fs::File::create(&target) {
                std::io::copy(&mut e, &mut out)?;
            }
        }
    }
    Ok(())
}
