use ringbahn::*;
use std::fs::{metadata, File};
use std::io;

fn main() -> io::Result<()> {
    let driver = drive::demo::driver();
    let meta = metadata("props.txt")?;
    let mut file = File::open("props.txt")?;
    let event = event::Read::new(&mut file, vec![0; meta.len() as usize]);
    let submission = Submission::new(event, driver);
    let content = futures::executor::block_on(async move {
        let (event, _, result) = submission.await;
        let bytes_read = result?;
        let s = String::from_utf8_lossy(&event.buf[0..bytes_read]).to_string();
        io::Result::Ok(s)
    })?;
    println!("{}", content);
    Ok(())
}
