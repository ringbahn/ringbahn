# ringbahn - a safe interface to io-uring

The Berlin Ringbahn is a double-tracked commuter rail line which forms a complete ring around the
center of the city of Berlin. Similarly, io-uring is a new interface for asynchronous IO with the
Linux kernel built on a double ring buffer data structure.

ringbahn is an attempt to define a good interface to perform IO on io-uring with these properties:
- 100% memory safe
- Completely non-blocking
- Ergonomic with async/await syntax
- A zero-cost abstraction with minimal overhead
- Abstracted over different patterns for driving the io-uring instance
- Misuse-resistant with a well implemented driver

**The current version of ringbahn is highly experimental and insufficiently hardened for production
use.** You are strongly recommended not to deploy the code under the current version. But tests, bug
reports, user feedback, and other experiments are all welcome at this stage.

Though ringbahn is a prototype, it demonstrates that a safe, ergonomic, efficient, and flexible
interface to io-uring is possible in Rust. It should be a goal for the Rust community not only to
have an adequate interface for io-uring, but to have the *best* interface for io-uring.

## License

ringbahn is licensed under your choice of MIT or Apache-2.0.
