use std::convert::TryFrom;
use std::io::{BufRead, Cursor};
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
    #[error("Error converting file date bytes to date string: {0}")]
    DateConvertError(std::string::FromUtf8Error)
}

impl From<std::io::Error> for FileParseError {
    fn from(err: std::io::Error) -> FileParseError {
        FileParseError::FileError(err)
    }
}

impl From<std::string::FromUtf8Error> for FileParseError {
    fn from(err: std::string::FromUtf8Error) -> FileParseError {
        FileParseError::DateConvertError(err)
    }
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
    let mut file_header = [0; 1024];
    let mut f = tokio::fs::File::open(file).await.map_err(|e| FileParseError::FileError(e))?;
    f.read_exact(&mut file_header).await.map_err(|e| FileParseError::FileError(e))?;

    // First handle initial pattern of 'II*' indicating start of file (JPG has some stuff
    // before that pattern, CR2 files appear to start with that pattern). This means that we can't
    // check that there is only a single byte in the read buffer here because in JPG there might be
    // more.
    let mut read = Vec::with_capacity(1024);
    let mut buf  = Cursor::new(&file_header[..]);
    let _ = buf.read_until(0x49u8, &mut read)?;
    read.clear();

    let r = buf.read_until(0x49u8, &mut read)?;
    if r != 1 {
        return Err(FileParseError::FileSeekError(format!("Did not find expected bytes in file while seeking to date. Expected 1 byte 'I' (0x49), found: {:?}", read)));
    }
    read.clear();

    let r = buf.read_until(0x2au8, &mut read)?;
    if r != 1 {
        return Err(FileParseError::FileSeekError(format!("Did not find expected bytes in file while seeking to date. Expected 1 byte '*' (0xau8), found: {:?}", read)));
    }
    read.clear();

    // Should be just after II* at this point
    buf.read_until(0x25u8, &mut read)?;
    read.clear();

    // There is a twice repeated pattern immediately before the date time string starts:
    // 48 00 00 00 01 00 00 00  48 00 00 00 01 00 00 00, That is, an H 3 null bytes, a 1 byte
    // (not ascii 1) and 3 more null bytes. Let's read through that, checking that we got what we
    // expected at the end.
    buf.read_until(0x48u8, &mut read)?;
    read.clear();

    let r = buf.read_until(0x48u8, &mut read)?;
    let expected = [0x00u8, 0x00u8, 0x00u8, 0x01u8, 0x00u8, 0x00u8, 0x00u8, 0x48u8];
    if r != 8 || read.as_slice() != expected {
        return Err(FileParseError::FileSeekError(format!("Did not find expected bytes in file while seeking to date. Expected 8 bytes matching {:?}, found: {:?}", expected, read)));
    }
    read.clear();
    buf.set_position(buf.position() + 7);

    let mut data = [0; 10];
    // For whatever reason the compiler is deciding to use tokio's AsyncRead implementation of this
    // instead of the Cursor Read implementation of read_exact. Seems like having the AsyncRead
    // trait in scope overrides the standard implementation of read_exact since Cursor implements
    // both AsyncRead and Read. Since this is a read on an in-memory buffer, no other reason that
    // it has to be async.
    buf.read_exact(&mut data).await?;
    // 2020:02:01 14:32:14
    let date = String::from_utf8(data.into_iter().map(|b| *b).collect::<Vec<u8>>())?;

    eprintln!("Result of metadata read: {:?}", data);
    Date::try_from(date)
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
