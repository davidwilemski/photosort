use std::convert::TryFrom;
use std::io::SeekFrom;
use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use thiserror::Error;
use tokio::io::{AsyncReadExt};

#[async_trait]
trait Renamer {
    async fn rename(&self, source: &Path, dest: &Path) -> std::io::Result<()>;
}

struct FileRenamer;

impl FileRenamer {
    fn new() -> Self {
        Self{}
    }
}

#[async_trait]
impl Renamer for FileRenamer {
    async fn rename(&self, source: &Path, dest: &Path) -> std::io::Result<()> {
        tokio::fs::rename(source, dest).await
    }
}

struct GitRenamer;

impl GitRenamer {
    fn new() -> Self {
        Self{}
    }
}

#[async_trait]
impl Renamer for GitRenamer {
    async fn rename(&self, source: &Path, dest: &Path) -> std::io::Result<()> {
        let status = tokio::process::Command::new("git")
            .arg("mv")
            .args(&[source.as_os_str(), dest.as_os_str()])
            .status()
            .await?;
        if status.success() {
            Ok(())
        } else {
            // XXX - should replace interface with custom Error/Result
            Err(std::io::Error::new(std::io::ErrorKind::Other, "git mv failed"))
        }
    }
}

fn get_renamer(arg: &Option<String>) -> Box<dyn Renamer> {
    match arg {
        Some(c) => match c.as_str() {
            "git" => Box::new(GitRenamer::new()),
            _ => Box::new(FileRenamer::new())
        },
        None => Box::new(FileRenamer::new())
    }
}

#[derive(Error, Debug)]
enum FileParseError {
    #[error("An error occured operating on a file: {0}")]
    FileError(std::io::Error),
    #[error("Error seeking: {0}")]
    FileSeekError(String),
    #[error("Error parsing date from file: {0}")]
    DateParseError(String),
}

struct Date {
    _src: String,
}

impl TryFrom<String> for Date {
    type Error = FileParseError;

    fn try_from(src: String) -> Result<Self, FileParseError> {
        let mut date_time_vals = src.split_whitespace();
        let date = date_time_vals.next().unwrap_or("");
        let year_month_day = date.split(":").collect::<Vec<&str>>();
        if year_month_day.len() != 3 {
            return Err(FileParseError::DateParseError("Read something that is not a date".into()));
        }

        Ok(Date {_src: date.into() })
    }
}

impl Date {
    fn year(&self) -> &str {
        self._src.split(":").nth(0).unwrap()
    }

    fn month(&self) -> &str {
        self._src.split(":").nth(1).unwrap()
    }

    fn day(&self) -> &str {
        self._src.split(":").nth(2).unwrap()
    }
}

async fn get_date_from_file(file: &Path) -> Result<Date, FileParseError> {
    let date_offset = 286;
    let mut f = tokio::fs::File::open(file).await.map_err(|e| FileParseError::FileError(e))?;
    let seek_result = f.seek(SeekFrom::Start(date_offset)).await;
    if let Ok(pos) = seek_result {
        eprintln!("seek postion: {}", pos);
        if pos != date_offset {
            return Err(FileParseError::FileSeekError("Failure to seek to date offset".into()));
        }
    }

    // 2020:02:01 14:32:14.
    let mut data = String::with_capacity(10);
    f.take(10).read_to_string(&mut data).await.map_err(|e| FileParseError::FileError(e))?;
    eprintln!("Result of metadata read: {:?}", data);
    Date::try_from(data)
}

#[tokio::main]
async fn main() -> Result<(), std::boxed::Box<(dyn std::error::Error)>> {
    let in_file = std::env::args().nth(1).unwrap();
    let renamer_arg = std::env::args().nth(2);
    let renamer = get_renamer(&renamer_arg);
    eprintln!("photosort {:?}", in_file);

    let filename = Path::new(&in_file);
    let date = get_date_from_file(&filename).await.context("Error in reading date out of input file")?;

    let home_var = std::env::var("HOME").context("$HOME env var not available")?;
    let home_dir = Path::new(&home_var);
    let photos_dir = home_dir.join("annex/photos");
    let new_path = format!("{}/{}/{}/{}", date.year(), date.month(), date.day(), filename.file_name().unwrap().to_str().unwrap());
    let dest = photos_dir.join(&new_path);
    let dest_dir = dest.parent().unwrap(); //.unwrap_or(Path::new("~/")).canonicalize().context("Failed to get parent of dest")?;
    eprintln!("input path: {:?}", filename);
    eprintln!("output path: {:?}", dest);
    tokio::fs::create_dir_all(&dest_dir).await.context("Failed to create dest dir")?;
    renamer.rename(&filename, &dest).await.context("Failed to rename file")?;
    Ok(())
}
