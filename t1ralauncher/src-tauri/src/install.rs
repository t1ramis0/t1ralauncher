use crate::{
    config::Settings,
    instances::{self, Instance},
    libs,
    net::{self, Task},
    paths, versions,
};
use anyhow::{anyhow, Result};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::{AppHandle, Emitter};

fn emit(app: &AppHandle, stage: &str, done: usize, total: usize) {
    let _ = app.emit("progress", json!({ "stage": stage, "done": done, "total": total }));
}

async fn run_tasks(app: &AppHandle, stage: &str, tasks: Vec<Task>, mirror: bool) -> Result<()> {
    // Без дубликатов, иначе два потока будут писать в один файл
    let mut seen = HashSet::new();
    let tasks: Vec<Task> = tasks.into_iter().filter(|t| seen.insert(t.dest.clone())).collect();
    let total = tasks.len();
    if total == 0 {
        return Ok(());
    }
    emit(app, stage, 0, total);
    let mut done = 0usize;
    let mut s = stream::iter(tasks)
        .map(move |t| async move { net::download(&t, mirror).await })
        .buffer_unordered(16);
    while let Some(r) = s.next().await {
        r?;
        done += 1;
        if done % 10 == 0 || done == total {
            emit(app, stage, done, total);
        }
    }
    Ok(())
}

/// Гарантирует, что всё нужное для запуска скачано. Уже скачанные файлы пропускаются по размеру.
pub async fn ensure_installed(app: &AppHandle, inst: &mut Instance, st: &Settings) -> Result<Value> {
    let mirror = st.use_mirror;

    if inst.loader != "vanilla" {
        let have = !inst.version_id.is_empty() && versions::version_json_path(&inst.version_id).exists();
        if !have {
            emit(app, "Загрузчик", 0, 1);
            inst.version_id = versions::install_loader(&inst.loader, &inst.mc_version, mirror).await?;
            instances::save(inst)?;
        }
    } else if inst.version_id.is_empty() {
        inst.version_id = inst.mc_version.clone();
        instances::save(inst)?;
    }

    emit(app, "Манифест", 0, 1);
    let json = versions::resolve(&inst.version_id, mirror).await?;
    let base = json["_base"]
        .as_str()
        .ok_or_else(|| anyhow!("нет базовой версии"))?
        .to_string();

    // client.jar + библиотеки
    let mut tasks: Vec<Task> = Vec::new();
    let client = &json["downloads"]["client"];
    tasks.push(Task {
        url: client["url"]
            .as_str()
            .ok_or_else(|| anyhow!("в манифесте нет client.jar"))?
            .to_string(),
        dest: paths::versions().join(&base).join(format!("{base}.jar")),
        sha1: client["sha1"].as_str().map(String::from),
        size: client["size"].as_u64(),
        verify: true,
    });
    for l in libs::collect(&json) {
        tasks.push(Task {
            url: l.url,
            dest: paths::libraries().join(&l.rel),
            sha1: l.sha1,
            size: l.size,
            verify: true,
        });
    }
    run_tasks(app, "Библиотеки", tasks, mirror).await?;

    // Ассеты
    let ai = &json["assetIndex"];
    let aid = ai["id"].as_str().unwrap_or("legacy").to_string();
    let idx_path = paths::assets().join("indexes").join(format!("{aid}.json"));
    net::download(
        &Task {
            url: ai["url"]
                .as_str()
                .ok_or_else(|| anyhow!("в манифесте нет assetIndex"))?
                .to_string(),
            dest: idx_path.clone(),
            sha1: ai["sha1"].as_str().map(String::from),
            size: ai["size"].as_u64(),
            verify: true,
        },
        mirror,
    )
    .await?;
    let idx: Value = serde_json::from_slice(&tokio::fs::read(&idx_path).await?)?;
    let objects = idx["objects"]
        .as_object()
        .ok_or_else(|| anyhow!("повреждён индекс ассетов"))?;

    let mut tasks: Vec<Task> = Vec::new();
    for (_name, o) in objects {
        let Some(h) = o["hash"].as_str() else { continue };
        if h.len() < 2 {
            continue;
        }
        let pre = &h[..2];
        tasks.push(Task {
            url: format!("https://resources.download.minecraft.net/{pre}/{h}"),
            dest: paths::assets().join("objects").join(pre).join(h),
            sha1: Some(h.to_string()),
            size: o["size"].as_u64(),
            verify: false,
        });
    }
    run_tasks(app, "Ресурсы", tasks, mirror).await?;

    // Старые версии (до 1.7) читают ассеты по реальным именам файлов
    if idx["virtual"] == true {
        for (name, o) in objects {
            let Some(h) = o["hash"].as_str() else { continue };
            if h.len() < 2 {
                continue;
            }
            let src = paths::assets().join("objects").join(&h[..2]).join(h);
            let dst = paths::assets().join("virtual").join(&aid).join(name);
            if !dst.exists() {
                if let Some(p) = dst.parent() {
                    std::fs::create_dir_all(p)?;
                }
                std::fs::copy(&src, &dst)?;
            }
        }
    }

    emit(app, "Готово", 1, 1);
    Ok(json)
}
