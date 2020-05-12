# ringbahn - a safe interface to io-uring

ringbahn is a crate integrating an io-uring reactor into Rust's async/await and futures ecosystem.
It provides an interface to perform IO on top of io-uring which is efficient, zero cost, and
completely memory safe.

The Berlin Ringbahn is a double-tracked commuter rail line which form complete ring around the
center of the city of Berlin. Similarly, io-uring is a new interface for asynchronous IO with the
Linux kernel built on top of a double ring buffer data structure.

io-uring is the future of high performance IO on Linux. It is the only realistic way to perform
asynchronous file IO on Linux, and benchmarks demonstrate that it is substantially more performant
than other interfaces of asynchronous IO of all types. The interface supports all the major forms of
IO supported by Linux, and the set of operations it supports continues to grow rapidly.

Rust needs a safe, zero-cost, easy to use interface for io-uring. This can become a major
differentiator for Rust as a systems language for high performance network services. Unfortunately
io-uring presents some unique challenges when creating a safe and correct interface. This is because
unlike epoll, io-uring is a *completion* based API, not a *readiness* based API. That presents
unique safety challenges when integrating with Rust's async/await ecosystem.

ringbahn demonstrates that it is possible to provide a safe interface for io-uring in Rust.
Currently it is a prototype; more complete, advanced, and performant APIs are certainly possible.
However, I hope that this library acts as a stepping stone toward a correct interface for io-uring
in Rust.

## Warning

This is currently a very experimental prototype. It contains a lot of unsafe code which has not been
sufficiently tested. Please do not use it in production. Tests, bug reports, feedback, and
experiments are all very welcome at this stage.

## License

ringbahn is licensed under your choice of MIT or Apache-2.0.
