use std::os::unix::io::IntoRawFd;

use ringbahn::event::*;
use ringbahn::drive::{demo, Drive};

use iou::sqe::*;

#[test]
fn test_registered_fd_ops() {
    // open and register file
    let file = std::fs::File::open("props.txt").unwrap();
    let fd = demo::registrar().unwrap()
                  .register_files(&[file.into_raw_fd()]).unwrap().next().unwrap();

    futures::executor::block_on(async move {
        // read file and print contents to stdout
        let buf = vec![0; 1024].into_boxed_slice();
        let (event, result) = demo::driver().submit(Read { fd, buf, offset: 0 }).await;
        let n = result.unwrap() as _;
        let data = String::from_utf8_lossy(&event.buf[..n]).to_owned();
        ringbahn::println!(demo::driver(), "{}", data).await;

        // statx file and print statx to stdout
        let (event, result) = demo::driver().submit(Statx::without_path(
            fd,
            StatxFlags::empty(),
            StatxMode::all(),
        )).await;
        result.unwrap();
        ringbahn::println!(demo::driver(), "{:?}", event.statx).await;

        // close file with ringbahn
        let (_, result) = demo::driver().submit(Close { fd }).await;
        result.unwrap();
    });
}
