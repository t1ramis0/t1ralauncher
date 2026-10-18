use crate::{net, paths};
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::path::PathBuf;

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Список всех версий Mojang. При ошибке сети берётся кэш из meta/.
pub async fn remote_versions(mirror: bool) -> Result<Vec<Value>> {
    let cache = paths::meta().join("version_manifest.json");
    let v = match net::fetch_json(MANIFEST_URL, mirror).await {
        Ok(v) => {
            let _ = std::fs::create_dir_all(paths::meta());
            let _ = std::fs::write(&cache, v.to_string());
            v
        }
        Err(e) => {
            let s = std::fs::read_to_string(&cache).map_err(|_| e)?;
            serde_json::from_str(&s)?
        }
    };
    Ok(v["versions"].as_array().cloned().unwrap_or_default())
}

pub fn version_json_path(id: &str) -> PathBuf {
    paths::versions().join(id).join(format!("{id}.json"))
}

/// JSON ванильной версии: с диска или из манифеста Mojang.
pub async fn vanilla_json(id: &str, mirror: bool) -> Result<Value> {
    let p = version_json_path(id);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(v) = serde_json::from_str(&s) {
            return Ok(v);
        }
    }
    let list = remote_versions(mirror).await?;
    let entry = list
        .iter()
        .find(|e| e["id"] == id)
        .ok_or_else(|| anyhow!("версия {id} не найдена в манифесте"))?;
    let url = entry["url"]
        .as_str()
        .ok_or_else(|| anyhow!("в манифесте нет url для {id}"))?;
    let v = net::fetch_json(url, mirror).await?;
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, serde_json::to_vec_pretty(&v)?)?;
    Ok(v)
}

/// Устанавливает профиль загрузчика (Fabric/Quilt) и возвращает id версии.
pub async fn install_loader(kind: &str, mc: &str, mirror: bool) -> Result<String> {
    let base = match kind {
        "fabric" => "https://meta.fabricmc.net/v2/versions/loader",
        "quilt" => "https://meta.quiltmc.org/v3/versions/loader",
        _ => return Err(anyhow!("неизвестный загрузчик: {kind}")),
    };
    let list = net::fetch_json(&format!("{base}/{mc}"), mirror).await?;
    let arr = list
        .as_array()
        .ok_or_else(|| anyhow!("неожиданный ответ сервера загрузчика"))?;
    let entry = arr
        .iter()
        .find(|e| e["loader"]["stable"] == true)
        .or_else(|| arr.first())
        .ok_or_else(|| anyhow!("для Minecraft {mc} нет загрузчика {kind}"))?;
    let lv = entry["loader"]["version"]
        .as_str()
        .ok_or_else(|| anyhow!("нет версии загрузчика"))?
        .to_string();

    let profile = net::fetch_json(&format!("{base}/{mc}/{lv}/profile/json"), mirror).await?;
    let id = profile["id"]
        .as_str()
        .ok_or_else(|| anyhow!("в профиле нет id"))?
        .to_string();
    let p = version_json_path(&id);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, serde_json::to_vec_pretty(&profile)?)?;
    Ok(id)
}

fn merge(parent: Value, child: Value) -> Value {
    let mut out = parent.clone();
    let o = out.as_object_mut().unwrap();
    if let Some(c) = child.as_object() {
        for (k, v) in c {
            match k.as_str() {
                "inheritsFrom" => {}
                "libraries" => {
                    let mut libs = v.as_array().cloned().unwrap_or_default();
                    libs.extend(parent["libraries"].as_array().cloned().unwrap_or_default());
                    o.insert(k.clone(), Value::Array(libs));
                }
                "arguments" => {
                    let mut args = serde_json::Map::new();
                    for key in ["game", "jvm"] {
                        let mut a = parent["arguments"][key].as_array().cloned().unwrap_or_default();
                        a.extend(v[key].as_array().cloned().unwrap_or_default());
                        args.insert(key.to_string(), Value::Array(a));
                    }
                    o.insert(k.clone(), Value::Object(args));
                }
                _ => {
                    o.insert(k.clone(), v.clone());
                }
            }
        }
    }
    out
}

/// Итоговый JSON версии с учётом inheritsFrom. Поле `_base` — id ванильной версии (откуда берётся client.jar).
pub async fn resolve(id: &str, mirror: bool) -> Result<Value> {
    let p = version_json_path(id);
    let child: Value = match std::fs::read_to_string(&p) {
        Ok(s) => serde_json::from_str(&s)?,
        Err(_) => vanilla_json(id, mirror).await?,
    };
    match child["inheritsFrom"].as_str() {
        Some(pid) => {
            let parent = vanilla_json(pid, mirror).await?;
            let mut m = merge(parent, child.clone());
            m["_base"] = Value::String(pid.to_string());
            Ok(m)
        }
        None => {
            let mut c = child.clone();
            c["_base"] = Value::String(id.to_string());
            Ok(c)
        }
    }
}
