use std::fmt::{self, Debug, Formatter};

/// Error returned when trying to push to a full ring buffer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BufferFullError(());

impl Debug for BufferFullError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "ring buffer is full")
    }
}

impl fmt::Display for BufferFullError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BufferFullError")
    }
}

impl std::error::Error for BufferFullError {}

impl BufferFullError {
    pub(crate) const fn new() -> Self {
        Self(())
    }
}
