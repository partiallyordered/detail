use std::process::ExitCode;
use tokio::fs::read_dir;
use thiserror::Error;

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
    let mut dir_entries = read_dir(path).await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))?;
    while let Some(dir_entry) = dir_entries.next_entry().await.map_err(|e| Error::FailedToReadDirContents(path.to_string(), e))? {
        println!("{:?}", dir_entry);
    }
    Ok(())
}
