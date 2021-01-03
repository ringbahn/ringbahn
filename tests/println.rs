use ringbahn::drive::demo;

#[test]
fn println() {
    futures::executor::block_on(async { ringbahn::println!(demo::driver(), "Hello, world!").await })
}

#[test]
fn print() {
    futures::executor::block_on(async { ringbahn::print!(demo::driver(), "Hello, world!").await })
}

#[test]
fn eprintln() {
    futures::executor::block_on(async {
        ringbahn::eprintln!(demo::driver(), "Hello, world!").await
    })
}

#[test]
fn eprint() {
    futures::executor::block_on(async { ringbahn::eprint!(demo::driver(), "Hello, world!").await })
}
