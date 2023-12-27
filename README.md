# ring-buffer
Stack-allocated ring buffer in Rust.

`RingBuffer` is a stack-allocated ring buffer that can be used to store a fixed number of elements. It is implemented as a circular array.

It can only hold types that implement `Copy`.

It depends on unstable features, so it can only be used with nightly Rust. It also uses a lot of unsafe so should probably not be used in production.
