use std::io::SeekFrom;
use std::path::{Path};

use anyhow::{Context, Result};
use async_trait::async_trait;
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

#[tokio::main]
async fn main() -> Result<(), std::boxed::Box<(dyn std::error::Error)>> {
    let date_offset = 286;
    let in_file = std::env::args().nth(1).unwrap();
    let renamer_arg = std::env::args().nth(2);
    let renamer: Box<dyn Renamer> = match renamer_arg {
        Some(c) => match c.as_str() {
            "git" => Box::new(GitRenamer::new()),
            _ => Box::new(FileRenamer::new())
        },
        _ => Box::new(FileRenamer::new())
    };

    let filename = Path::new(&in_file);
    let mut f = tokio::fs::File::open(&filename).await.context("Failed to open input file")?;
    let seek_result = f.seek(SeekFrom::Start(date_offset)).await;
    if let Ok(pos) = seek_result {
        eprintln!("seek postion: {}", pos);
        if pos != date_offset {
            eprintln!("failure to seek to date offset");
            return Ok(());  // TODO non-zero exit
        }
    }

    // 2020:02:01 14:32:14.
    let mut data = String::with_capacity(10);
    f.take(10).read_to_string(&mut data).await.context("Failed to read bytes from input file")?;
    eprintln!("Result of metadata read: {:?}", data);
    let mut date_time_vals = data.split_whitespace();
    let date = date_time_vals.next().unwrap_or("");
    let year_month_day = date.split(":").collect::<Vec<&str>>();
    if year_month_day.len() != 3 {
        eprintln!("parsed something that is not a year/month/day: {:?}", year_month_day);
        return Ok(());  // TODO non-zero exit
    }
    let year = year_month_day.get(0).expect("already validated size");
    let month = year_month_day.get(1).expect("already validated size");
    let day = year_month_day.get(2).expect("already validated size");
    let home_var = std::env::var("HOME").context("$HOME env var not available")?;
    let home_dir = Path::new(&home_var);
    let photos_dir = home_dir.join("annex/photos");
    let new_path = format!("{}/{}/{}/{}", year, month, day, filename.file_name().unwrap().to_str().unwrap());
    let dest = photos_dir.join(&new_path);
    let dest_dir = dest.parent().unwrap(); //.unwrap_or(Path::new("~/")).canonicalize().context("Failed to get parent of dest")?;
    eprintln!("input path: {:?}", filename);
    eprintln!("output path: {:?}", dest);
    tokio::fs::create_dir_all(&dest_dir).await.context("Failed to create dest dir")?;
    renamer.rename(&filename, &dest).await.context("Failed to rename file")?;
    Ok(())
}
