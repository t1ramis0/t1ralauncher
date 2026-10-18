use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("T1raLauncher/0.1")
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("http client")
    })
}

const BMCL: &str = "https://bmclapi2.bangbang93.com";

fn to_mirror(url: &str) -> Option<String> {
    let table: Vec<(&str, String)> = vec![
        ("https://launchermeta.mojang.com", BMCL.to_string()),
        ("https://piston-meta.mojang.com", BMCL.to_string()),
        ("https://piston-data.mojang.com", BMCL.to_string()),
        ("https://launcher.mojang.com", BMCL.to_string()),
        ("https://libraries.minecraft.net", format!("{BMCL}/maven")),
        ("https://resources.download.minecraft.net", format!("{BMCL}/assets")),
    ];
    for (from, to) in table.iter() {
        if let Some(rest) = url.strip_prefix(from) {
            return Some(format!("{to}{rest}"));
        }
    }
    None
}

/// Список адресов для попытки: зеркало (если включено) и затем оригинал.
pub fn candidates(url: &str, mirror: bool) -> Vec<String> {
    let mut v = Vec::new();
    if mirror {
        if let Some(m) = to_mirror(url) {
            v.push(m);
        }
    }
    v.push(url.to_string());
    v
}

pub async fn fetch_json(url: &str, mirror: bool) -> Result<Value> {
    let mut last = anyhow!("нет адреса для запроса");
    for u in candidates(url, mirror) {
        match client().get(&u).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => match r.json::<Value>().await {
                Ok(v) => return Ok(v),
                Err(e) => last = anyhow!("{u}: {e}"),
            },
            Err(e) => last = anyhow!("{u}: {e}"),
        }
    }
    Err(last)
}

#[derive(Clone, Debug)]
pub struct Task {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// true — проверять SHA1 уже скачанного файла при каждом запуске (библиотеки, client.jar)
    pub verify: bool,
}

async fn is_present(t: &Task) -> bool {
    let metadata = match tokio::fs::metadata(&t.dest).await {
        Ok(m) if m.is_file() => m,
        _ => return false,
    };

    if let Some(expected_size) = t.size {
        if metadata.len() != expected_size {
            return false;
        }
    } else if metadata.len() == 0 {
        return false;
    }

    let Some(expected_sha1) = t.sha1.as_deref().filter(|_| t.verify) else {
        return true;
    };

    // Не доверяем одному размеру: повреждённый файл может иметь ту же длину.
    let mut file = match tokio::fs::File::open(&t.dest).await {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buffer).await {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected_sha1)
}

pub async fn download(t: &Task, mirror: bool) -> Result<()> {
    if is_present(t).await {
        return Ok(());
    }
    if let Some(p) = t.dest.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let mut last = anyhow!("нет адреса для загрузки");
    for attempt in 0..3u64 {
        for u in candidates(&t.url, mirror) {
            match try_once(&u, t).await {
                Ok(()) => return Ok(()),
                Err(e) => last = anyhow!("{u}: {e}"),
            }
        }
        tokio::time::sleep(Duration::from_millis(400 * (attempt + 1))).await;
    }
    Err(last)
}

async fn try_once(u: &str, t: &Task) -> Result<()> {
    let mut name = t
        .dest
        .file_name()
        .ok_or_else(|| anyhow!("пустое имя файла"))?
        .to_os_string();
    name.push(".part");
    let tmp = t.dest.with_file_name(name);
    let r = fetch_to(u, t, &tmp).await;
    if r.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    r
}

async fn fetch_to(u: &str, t: &Task, tmp: &std::path::Path) -> Result<()> {
    let resp = client().get(u).send().await?.error_for_status()?;
    let mut f = tokio::fs::File::create(tmp).await?;
    let mut hasher = Sha1::new();
    let mut stream = resp.bytes_stream();
    let mut written: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let c = chunk?;
        hasher.update(&c);
        written += c.len() as u64;
        f.write_all(&c).await?;
    }
    f.flush().await?;
    drop(f);

    if let Some(sz) = t.size {
        if written != sz {
            return Err(anyhow!("размер файла не совпал: получено {written}, ожидалось {sz}"));
        }
    }
    if let Some(exp) = &t.sha1 {
        let got = hex::encode(hasher.finalize());
        if !got.eq_ignore_ascii_case(exp) {
            return Err(anyhow!("не совпала контрольная сумма SHA1"));
        }
    }
    tokio::fs::rename(tmp, &t.dest).await?;
    Ok(())
}
