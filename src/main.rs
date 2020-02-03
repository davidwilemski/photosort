use std::io::SeekFrom;

use tokio::io::{AsyncReadExt};

#[tokio::main]
async fn main() -> std::result::Result<(), std::boxed::Box<(dyn std::error::Error)>> {
    let date_offset = 288 + 10;
    let mut f = tokio::fs::File::open(std::env::args().nth(1).unwrap()).await?;
    let seek_result = f.seek(SeekFrom::Start(date_offset)).await;
    if let Ok(pos) = seek_result {
        println!("seek postion: {}", pos);
        if pos != date_offset {
            eprintln!("failure to seek to date offset");
            return Ok(());
        }
    }

    let mut data = String::with_capacity(10);
    f.take(19).read_to_string(&mut data).await?;
    println!("Result of metadata read: {:?}", data);
    Ok(())
}
