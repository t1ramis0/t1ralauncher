use crate::{config, install, instances, libs, paths};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

#[derive(Default)]
pub struct AppState {
    /// id сборки -> сигнал остановки процесса
    pub running: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

/// UUID оффлайн-игрока: так его считают ванильные клиент и сервер (md5 от "OfflinePlayer:<ник>", версия 3).
pub fn offline_uuid(name: &str) -> String {
    let mut b = md5::compute(format!("OfflinePlayer:{name}")).0;
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn subst(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = s.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
}

fn collect_args(arr: Option<&Vec<Value>>, vars: &HashMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    for a in arr.into_iter().flatten() {
        match a {
            Value::String(s) => out.push(subst(s, vars)),
            Value::Object(_) => {
                if !libs::rules_allow(a.get("rules")) {
                    continue;
                }
                match &a["value"] {
                    Value::String(s) => out.push(subst(s, vars)),
                    Value::Array(v) => {
                        for x in v {
                            if let Some(s) = x.as_str() {
                                out.push(subst(s, vars));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn log(app: &AppHandle, id: &str, line: impl Into<String>) {
    let _ = app.emit("log", json!({ "id": id, "line": line.into() }));
}

pub fn stop(state: &AppState, id: &str) {
    if let Some(tx) = state.running.lock().unwrap().remove(id) {
        let _ = tx.send(());
    }
}

pub async fn launch(app: AppHandle, state: &AppState, id: String) -> Result<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err(anyhow!("эта сборка уже запущена"));
    }
    let st = config::load();
    let mut inst = instances::load(&id)?;
    let json = install::ensure_installed(&app, &mut inst, &st).await?;

    let gdir = instances::game_dir(&id);
    std::fs::create_dir_all(&gdir)?;
    let version_id = inst.version_id.clone();
    let base = json["_base"].as_str().unwrap_or(&inst.mc_version).to_string();

    let libs_list = libs::collect(&json);
    let natives = paths::versions().join(&version_id).join("natives");
    std::fs::create_dir_all(&natives)?;
    libs::extract_natives(&libs_list, &natives)?;

    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut cp: Vec<String> = libs_list
        .iter()
        .filter(|l| !l.extract)
        .map(|l| paths::libraries().join(&l.rel).to_string_lossy().to_string())
        .collect();
    cp.push(
        paths::versions()
            .join(&base)
            .join(format!("{base}.jar"))
            .to_string_lossy()
            .to_string(),
    );
    let classpath = cp.join(sep);

    let aid = json["assetIndex"]["id"].as_str().unwrap_or("legacy").to_string();
    let idx_file = paths::assets().join("indexes").join(format!("{aid}.json"));
    let is_virtual = std::fs::read_to_string(&idx_file)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .map_or(false, |v| v["virtual"] == true);
    let assets_root = if is_virtual {
        paths::assets().join("virtual").join(&aid)
    } else {
        paths::assets()
    };

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", st.username.clone());
    vars.insert("version_name", version_id.clone());
    vars.insert("game_directory", gdir.to_string_lossy().to_string());
    vars.insert("assets_root", assets_root.to_string_lossy().to_string());
    vars.insert("game_assets", assets_root.to_string_lossy().to_string());
    vars.insert("assets_index_name", aid.clone());
    vars.insert("auth_uuid", offline_uuid(&st.username));
    vars.insert("auth_access_token", "0".into());
    vars.insert("auth_session", "0".into());
    vars.insert("user_type", "legacy".into());
    vars.insert("version_type", json["type"].as_str().unwrap_or("release").to_string());
    vars.insert("clientid", "0".into());
    vars.insert("auth_xuid", "0".into());
    vars.insert("user_properties", "{}".into());
    vars.insert("natives_directory", natives.to_string_lossy().to_string());
    vars.insert("launcher_name", "T1raLauncher".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath", classpath);
    vars.insert("classpath_separator", sep.into());
    vars.insert("library_directory", paths::libraries().to_string_lossy().to_string());

    let jvm_args = match json["arguments"]["jvm"].as_array() {
        Some(a) => collect_args(Some(a), &vars),
        None => vec![
            format!("-Djava.library.path={}", vars["natives_directory"]),
            "-cp".into(),
            vars["classpath"].clone(),
        ],
    };
    let game_args = match json["arguments"]["game"].as_array() {
        Some(a) => collect_args(Some(a), &vars),
        None => json["minecraftArguments"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .map(|s| subst(s, &vars))
            .collect(),
    };
    let main_class = json["mainClass"]
        .as_str()
        .ok_or_else(|| anyhow!("в манифесте нет mainClass"))?
        .to_string();

    let ram = inst.ram_mb.unwrap_or(st.max_ram_mb).clamp(512, 65536);
    let mut args: Vec<String> = vec![format!("-Xmx{ram}M"), format!("-Xms{}M", ram.min(512))];
    args.extend(st.java_args.split_whitespace().map(String::from));
    args.extend(jvm_args);
    args.push(main_class);
    args.extend(game_args);
    for a in args.iter().filter(|a| a.contains("${")) {
        log(&app, &id, format!("[лаунчер] Внимание: не подставлена переменная в аргументе: {a}"));
    }

    let java = if st.java_path.trim().is_empty() {
        "java".to_string()
    } else {
        st.java_path.trim().to_string()
    };

    if let Some(major) = json["javaVersion"]["majorVersion"].as_u64() {
        log(&app, &id, format!("[лаунчер] Для этой версии нужна Java {major}+"));
    }
    log(&app, &id, format!("[лаунчер] Запуск: {java}, игрок {}", st.username));

    let mut cmd = tokio::process::Command::new(&java);
    cmd.args(&args)
        .current_dir(&gdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = cmd.spawn().map_err(|e| {
        anyhow!("не удалось запустить Java («{java}»): {e}. Установи Java или укажи путь к ней в настройках")
    })?;

    if let Some(out) = child.stdout.take() {
        let (app, id) = (app.clone(), id.clone());
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log(&app, &id, line);
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let (app, id) = (app.clone(), id.clone());
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log(&app, &id, line);
            }
        });
    }

    let (tx, rx) = oneshot::channel::<()>();
    state.running.lock().unwrap().insert(id.clone(), tx);
    let _ = app.emit("started", json!({ "id": id }));

    let app2 = app.clone();
    let id2 = id.clone();
    tauri::async_runtime::spawn(async move {
        let code = tokio::select! {
            r = child.wait() => r.ok().and_then(|s| s.code()),
            _ = rx => {
                let _ = child.kill().await;
                None
            }
        };
        app2.state::<AppState>().running.lock().unwrap().remove(&id2);
        let _ = app2.emit("exited", json!({ "id": id2, "code": code }));
    });

    Ok(())
}
