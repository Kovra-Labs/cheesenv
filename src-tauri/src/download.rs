use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

/// Downloads `url` into `dir/<sha256>`, resuming a previous `.part` file via
/// HTTP Range, then verifies SHA-256. A mismatching file is deleted.
pub async fn fetch_verified(
    client: &reqwest::Client,
    url: &str,
    sha256: &str,
    dir: &Path,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> AppResult<PathBuf> {
    let sha256 = sha256.to_ascii_lowercase();
    tokio::fs::create_dir_all(dir).await?;
    let done = dir.join(&sha256);
    let part = dir.join(format!("{sha256}.part"));

    if done.exists() {
        if hash_file(&done).await? == sha256 {
            return Ok(done);
        }
        tokio::fs::remove_file(&done).await?;
    }

    let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let resp = req.send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Part file is already complete (or corrupt); verification decides.
        let _ = tokio::fs::rename(&part, &done).await;
        return verify(done, &sha256).await;
    }
    let resp = resp.error_for_status()?;
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut downloaded = if resumed { have } else { 0 };
    let total = resp.content_length().map(|len| len + downloaded);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .await?;
    let mut stream = resp.bytes_stream();
    on_progress(downloaded, total);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total);
    }
    file.flush().await?;
    drop(file);

    tokio::fs::rename(&part, &done).await?;
    verify(done, &sha256).await
}

async fn verify(path: PathBuf, expected: &str) -> AppResult<PathBuf> {
    if hash_file(&path).await? == expected {
        Ok(path)
    } else {
        let _ = tokio::fs::remove_file(&path).await;
        Err(AppError::Checksum)
    }
}

async fn hash_file(path: &Path) -> AppResult<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> AppResult<String> {
        let mut f = std::fs::File::open(path)?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h)?;
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
}
