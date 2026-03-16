use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::{self, AsyncReadExt};


pub async fn calculate_file_hash(path: &Path) -> io::Result<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn calculate_bytes_hash(data: &[u8]) -> io::Result<String> {
    let mut hasher = Sha256::new();

    hasher.update(data);

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}
