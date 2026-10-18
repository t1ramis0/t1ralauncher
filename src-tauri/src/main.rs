#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod install;
mod instances;
mod launch;
mod libs;
mod net;
mod paths;
mod versions;

use config::Settings;
use instances::Instance;
use serde_json::Value;
use tauri::{AppHandle, State};

type R<T> = Result<T, String>;

fn e(x: anyhow::Error) -> String {
    format!("{x:#}")
}

#[tauri::command]
fn get_settings() -> Settings {
    config::load()
}

#[tauri::command]
fn save_settings(s: Settings) -> R<()> {
    config::save(&s).map_err(e)
}

#[tauri::command]
async fn remote_versions() -> R<Vec<Value>> {
    let st = config::load();
    versions::remote_versions(st.use_mirror).await.map_err(e)
}

#[tauri::command]
fn list_instances() -> Vec<Instance> {
    instances::list()
}

#[tauri::command]
async fn create_instance(app: AppHandle, name: String, mc_version: String, loader: String) -> R<Instance> {
    let st = config::load();
    let mut inst = instances::create(&name, &mc_version, &loader).map_err(e)?;
    if let Err(err) = install::ensure_installed(&app, &mut inst, &st).await {
        let _ = instances::delete(&inst.id);
        return Err(e(err));
    }
    Ok(inst)
}

#[tauri::command]
fn delete_instance(state: State<'_, launch::AppState>, id: String) -> R<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err("сначала останови игру".into());
    }
    instances::delete(&id).map_err(e)
}

#[tauri::command]
fn set_instance_ram(id: String, ram_mb: Option<u32>) -> R<()> {
    let mut inst = instances::load(&id).map_err(e)?;
    inst.ram_mb = ram_mb.map(|r| r.clamp(512, 65536));
    instances::save(&inst).map_err(e)
}

#[tauri::command]
async fn launch_instance(app: AppHandle, state: State<'_, launch::AppState>, id: String) -> R<()> {
    launch::launch(app, state.inner(), id).await.map_err(e)
}

#[tauri::command]
fn stop_instance(state: State<'_, launch::AppState>, id: String) {
    launch::stop(state.inner(), &id);
}

#[tauri::command]
fn open_instance_folder(id: String) -> R<()> {
    let inst = instances::load(&id).map_err(e)?;
    let dir = instances::game_dir(&inst.id);
    std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
    let prog = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(prog)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|x| x.to_string())
}

fn main() {
    let _ = paths::ensure_all();
    tauri::Builder::default()
        .manage(launch::AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            remote_versions,
            list_instances,
            create_instance,
            delete_instance,
            set_instance_ram,
            launch_instance,
            stop_instance,
            open_instance_folder
        ])
        .run(tauri::generate_context!())
        .expect("ошибка запуска T1raLauncher");
}
