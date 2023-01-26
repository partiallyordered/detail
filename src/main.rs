use std::process::ExitCode;
use serde::Serialize;
use tokio::fs::{read_dir, DirEntry};
use thiserror::Error;
use futures::stream::FuturesOrdered;
use futures::StreamExt;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum EntryType {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Serialize)]
struct EntryInfo {
    name: String,
    #[serde(rename = "type")]
    file_type: EntryType,
    accessed: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Failed to read directory contents {0}")]
    FailedToReadDirContents(String, std::io::Error),
    #[error("Failed to serialize path {0}: {1}")]
    FailedToSerializePath(String, serde_json::Error),
    #[error("Failed to retrieve file type for {0}")]
    FailedToRetrieveFileType(String),
    #[error("Failed to retrieve file metadata for {0}: {1}")]
    FailedToRetrieveFileMetadata(String, std::io::Error),
    #[error("Failed to retrieve file last access time for {0}: {1}")]
    FailedToRetrieveFileAccessTime(String, std::io::Error),
}
type Result<T> = std::result::Result<T, Error>;

#[tokio::main]
async fn main() -> ExitCode {
    match _main().await {
        Err(_) => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS
    }
}

async fn _main() -> Result<()> {
    let path = ".";
    let mut stream = FuturesOrdered::new();
    let mut dir_entries = read_dir(path).await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))?;
    print!("[");
    while let Some(dir_entry) = dir_entries.next_entry().await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))? {
        stream.push_back(
            tokio::spawn(async move {
                process_dir_entry(dir_entry).await
            })
        );
    }
    if let Some(first_res) = stream.next().await {
        let first_res = first_res.unwrap()?;
        print!("{}", serialize_entry_info(first_res).unwrap());
        while let Some(subsequent_res) = stream.next().await {
            match subsequent_res {
                Ok(res) => print!(",{}", serialize_entry_info(res?).unwrap()),
                _ => print!("uh oh"),
            }
        }
    }
    print!("]");
    Ok(())
}

fn serialize_entry_info(entry: EntryInfo) -> Result<String> {
    serde_json::to_string(&entry).map_err(|e| Error::FailedToSerializePath(entry.name, e))
}

async fn process_dir_entry(entry: DirEntry) -> Result<EntryInfo> {
    // TODO:
    // - perhaps a DirEntry::Into implementation for EntryInfo? So we can consume it. Is that how
    //   that works?
    // - Improve this nasty as_os_str, to_string_lossy, etc. chain
    let name: String = entry.path().as_os_str().to_string_lossy().to_string();
    let metadata = entry.metadata().await.map_err(|e| Error::FailedToRetrieveFileMetadata(name.clone(), e))?;

    // TODO: is there an existing enum for file type? Can the scenario where no type is detected
    // occur?
    let file_type = if metadata.file_type().is_dir() {
        EntryType::Dir
    } else if metadata.is_file() {
        EntryType::File
    } else if metadata.is_symlink() {
        EntryType::Symlink
    } else {
        return Err(Error::FailedToRetrieveFileType(name))
    };

    let accessed = {
        let accessed = metadata.accessed().map_err(|e| Error::FailedToRetrieveFileAccessTime(name.clone(), e))?;
        let accessed: DateTime<Utc> = accessed.into();
        accessed.to_rfc3339()
    };

    Ok(EntryInfo {
        name: name.strip_prefix("./").unwrap_or(&name).to_string(),
        file_type,
        accessed,
    })
}
