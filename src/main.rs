use std::process::ExitCode;
use tokio::fs::{read_dir, DirEntry};
use thiserror::Error;
use futures::stream::FuturesOrdered;
use futures::StreamExt;

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
    if let Some(dir_entry) = dir_entries.next_entry().await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))? {
        stream.push_back(
            tokio::spawn(async move {
                process_dir_entry(dir_entry).await
            })
        );
        while let Some(dir_entry) = dir_entries.next_entry().await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))? {
            stream.push_back(
                tokio::spawn(async move {
                    process_dir_entry(dir_entry).await.map(|s| format!(",{s}"))
                })
            );
        }
    }
    while let Some(res) = stream.next().await {
        print!("{:?}", res);
    }
    print!("]");
    Ok(())
}

async fn process_dir_entry(entry: DirEntry) -> Result<String> {
    Ok(format!("{:?}", entry))
}
