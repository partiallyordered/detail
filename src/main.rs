use std::process::ExitCode;
use tokio::fs::{read_dir, DirEntry};
use thiserror::Error;
use futures::stream::FuturesOrdered;
use futures::StreamExt;

#[derive(Debug)]
struct EntryInfo {
    name: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Failed to read directory contents {0}")]
    FailedToReadDirContents(String, std::io::Error),
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
        let first_res = first_res.unwrap().unwrap();
        print!("\"{}\"", serialize_entry_info(first_res).unwrap());
        while let Some(subsequent_res) = stream.next().await {
            match subsequent_res {
                Ok(Ok(res)) => print!(",\"{}\"", serialize_entry_info(res).unwrap()),
                _ => print!("uh oh"),
            }
        }
    }
    print!("]");
    Ok(())
}

fn serialize_entry_info(entry: EntryInfo) -> Result<String> {
    Ok(format!("{}", entry.name))
}

async fn process_dir_entry(entry: DirEntry) -> Result<EntryInfo> {
    // TODO:
    // - perhaps a DirEntry::Into implementation for EntryInfo? So we can consume it. Is that how
    //   that works?
    // - Improve this nasty as_os_str, to_string_lossy, etc. chain
    let name: String = entry.path().as_os_str().to_string_lossy().to_string();
    Ok(EntryInfo {
        name: name.strip_prefix("./").unwrap_or(&name).to_string()
    })
}
