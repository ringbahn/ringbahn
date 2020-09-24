use ringbahn::drive::demo;

#[test]
fn println() {
    futures::executor::block_on(async {
        ringbahn::println!(demo::driver(), "Hello, world!").await
    })
}
