use std::process::ExitCode;
use serde::Serialize;
use tokio::fs::{read_dir, DirEntry};
use thiserror::Error;
use futures::stream::FuturesOrdered;
use futures::StreamExt;
use chrono::{DateTime, Utc};
use git2::Repository;
use clap::{Parser, ValueEnum};
use derive_more::Display;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
/// A full-fat implementation of ls
struct Args {
    /// Output data type
    #[arg(short, long, default_value="json")]
    encoding: OutputEncoding,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Copy)]
enum OutputEncoding {
    /// Produce Nuon (Nushell data)
    #[cfg(feature = "nu")]
    Nuon,
    Json,
    Jsonl,
}

#[derive(Debug, Serialize, Display)]
#[serde(rename_all = "lowercase")]
enum EntryType {
    File,
    Dir,
    Symlink,
}

// TODO: investigate whether it's possible to provide a serialize implementation for
// git2::FileStatus (or whatever the type is) instead of mapping to this enum
// TODO: this is a bitfield in actuality. exa appears to have two columns to display the git
// status. Why's that? Index and worktree?
// Ref: https://github.com/nushell/nushell/blob/main/crates/nu_plugin_gstat/src/gstat.rs
#[derive(Debug, Serialize, Display)]
enum FileGitStatus {
    #[serde(rename = "M")]
    #[display(fmt = "M")]
    Modified,
    #[serde(rename = "C")]
    #[display(fmt = "C")]
    Current,
    #[serde(rename = "N")]
    #[display(fmt = "N")]
    New,
    #[serde(rename = "I")]
    #[display(fmt = "I")]
    Ignored,
    #[serde(rename = "!")]
    #[display(fmt = "!")]
    Conflict,
    #[serde(rename = "D")]
    #[display(fmt = "D")]
    Deleted, // I *think* it's possible to have a file deleted in the repo but not in the file system
    #[serde(rename = "R")]
    #[display(fmt = "R")]
    Renamed,
}

#[derive(Debug, Serialize)]
struct EntryInfo {
    // Fields are serialized in the order they're listed here.

    name: String,

    // rename "type" because it's probably best to avoid the name "type" in the codebase
    #[serde(rename = "type")]
    file_type: EntryType,

    size: u64,
    modified: String,

    // rename "file_git_status" to "git" because it's probably best to minimize the name "git" in
    // the codebase
    #[serde(rename = "git")]
    file_git_status: Option<FileGitStatus>,

    accessed: String,

    // TODO: created? not always available
}

#[derive(Error, Debug)]
enum Error {
    #[error("Failed to read directory contents {0}")]
    DirContentsRead(String, std::io::Error),
    #[error("Failed to serialize path {0}: {1}")]
    EntrySerialize(String, serde_json::Error),
    #[error("Failed to retrieve file type for {0}")]
    FileTypeRetrieve(String),
    #[error("Failed to retrieve file metadata for {0}: {1}")]
    MetadataRetrieve(String, std::io::Error),
    #[error("Failed to retrieve file last access time for {0}: {1}")]
    AccessTimeRetrieve(String, std::io::Error),
    #[error("Failed to canonicalize path {0}: {1}")]
    PathCanonicalize(String, std::io::Error),
}
type Result<T> = std::result::Result<T, Error>;

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    match run(args).await {
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS
    }
}

async fn run(args: Args) -> Result<()> {
    let path = ".";
    let mut stream = FuturesOrdered::new();
    let mut dir_entries = read_dir(path).await.map_err(|e| Error::DirContentsRead(path.to_string(), e))?;

    match args.encoding {
        OutputEncoding::Json => { print!("[") },
        OutputEncoding::Nuon => {
            print!("[[name, type, size, modified, git, accessed]; ");
        },
        OutputEncoding::Jsonl => {},
    }

    while let Some(dir_entry) = dir_entries.next_entry().await.map_err(|e| Error::DirContentsRead(path.to_string(), e))? {
        stream.push_back(
            tokio::spawn(async move {
                process_dir_entry(dir_entry).await
            })
        );
    }

    if let Some(first_res) = stream.next().await {
        let first_res = first_res.unwrap()?;
        print!("{}", serialize_entry_info(first_res, args.encoding).unwrap());
        while let Some(subsequent_res) = stream.next().await {
            match subsequent_res {
                Ok(res) => {
                    let res = res?;
                    match args.encoding {
                        // TODO: JSON does not require spaces, does nuon? Presumably not
                        // should/could we omit them?
                        OutputEncoding::Json => { print!(","); },
                        OutputEncoding::Nuon => { print!(", "); },
                        OutputEncoding::Jsonl => { print!("\n"); },
                    }
                    print!("{}", serialize_entry_info(res, args.encoding).unwrap())
                },
                // TODO: this is really top-notch..
                _ => print!("uh oh"),
            }
        }
    }

    match args.encoding {
        OutputEncoding::Json => { print!("]") },
        OutputEncoding::Nuon => {
            print!("]");
        },
        OutputEncoding::Jsonl => {},
    }

    Ok(())
}

fn serialize_entry_info(entry: EntryInfo, encoding: OutputEncoding) -> Result<String> {
    match encoding {
        OutputEncoding::Jsonl | OutputEncoding::Json => {
            serde_json::to_string(&entry).map_err(|e| Error::EntrySerialize(entry.name, e))
        },
        OutputEncoding::Nuon => {
            Ok(format!(
                "[{}, {}, {}, {}, {}, {}]",
                entry.name,
                entry.file_type,
                entry.size,
                entry.modified,
                entry.file_git_status.map(|st| st.to_string()).unwrap_or("\"\"".to_string()),
                entry.accessed,
            ))
        },
    }
}

async fn process_dir_entry(entry: DirEntry) -> Result<EntryInfo> {
    // TODO:
    // - perhaps a DirEntry::Into implementation for EntryInfo? So we can consume it. Is that how
    //   that works?
    // - Improve this nasty as_os_str, to_string_lossy, etc. chain
    let path = entry.path();
    let name: String = path.as_os_str().to_string_lossy().to_string();
    let metadata = entry.metadata().await.map_err(|e| Error::MetadataRetrieve(name.clone(), e))?;

    // TODO: is there an existing enum for file type? Can the scenario where no type is detected
    // occur?
    let file_type = if metadata.file_type().is_dir() {
        EntryType::Dir
    } else if metadata.is_file() {
        EntryType::File
    } else if metadata.is_symlink() {
        EntryType::Symlink
    } else {
        return Err(Error::FileTypeRetrieve(name))
    };

    let accessed = {
        let accessed = metadata.accessed().map_err(|e| Error::AccessTimeRetrieve(name.clone(), e))?;
        let accessed: DateTime<Utc> = accessed.into();
        accessed.to_rfc3339()
    };

    let modified = {
        let modified = metadata.modified().map_err(|e| Error::AccessTimeRetrieve(name.clone(), e))?;
        let modified: DateTime<Utc> = modified.into();
        modified.to_rfc3339()
    };

    // TODO: does git_status work on symlinks? Symlink directories?
    let git_status = if metadata.is_dir() {
        // TODO: recurse into directory for status (optionally?). Other tools, e.g. exa seem to
        // show status on directories.
        None
    } else {
        let path_abs = path.canonicalize().map_err(|e| Error::PathCanonicalize(name.clone(), e))?;
        let path = path.strip_prefix("./").unwrap_or(&path);
        match Repository::discover(path)
            .and_then(|repo| {
                // TODO: re-examine the "common path" logic when it's not 1am. There's something
                // funky about it and I can't figure out what.
                let path_rel = path_abs.strip_prefix(repo.path().parent().unwrap()).unwrap();
                repo.status_file(&path_rel)
            }) {
                Ok(status) => {
                    // TODO: thorough investigation whether the current mappings here are
                    // sensible. I don't know the difference between the WT_ and INDEX_
                    // prefixes, for example. Also, this is a bitfield. Probably we should just map
                    // the bitfield into a string or record/object in the response.
                    if status == git2::Status::CURRENT { None }
                    else if status.is_wt_new() { Some(FileGitStatus::New) }
                    else if status.is_ignored() { Some(FileGitStatus::Ignored) }
                    else if status.is_conflicted() { Some(FileGitStatus::Conflict) }
                    else if status.is_index_new() { Some(FileGitStatus::New) }
                    else if status.is_wt_deleted() { Some(FileGitStatus::Deleted) }
                    else if status.is_wt_renamed() { Some(FileGitStatus::Renamed) }
                    else if status.is_wt_modified() { Some(FileGitStatus::Modified) }
                    else if status.is_index_modified() { Some(FileGitStatus::Modified) }
                    else if status.is_index_deleted() { Some(FileGitStatus::Deleted) }
                    // TODO: what is typechange? Should they be None?
                    else if status.is_wt_typechange() { None }
                    else if status.is_index_typechange() { None }
                    else { None }
                }
                Err(e) => {
                    // TODO: actually examine the errors returned here, sometimes it might be
                    // appropriate to return an error to the user instead of just returning no status
                    // information
                    None
                }
            }
    };

    Ok(EntryInfo {
        accessed,
        file_type,
        modified,
        name: name.strip_prefix("./").unwrap_or(&name).to_string(),
        size: metadata.len(),

        file_git_status: git_status,
    })
}
