use crate::RingBuffer;

pub struct VacantEntry<'buf, A: Copy, const CAP: usize>(
    // Invariant: `buf.has_remaining()`
    &'buf mut RingBuffer<A, CAP>,
);

impl<'buf, A: Copy, const CAP: usize> VacantEntry<'buf, A, CAP> {
    pub(super) unsafe fn new_unchecked(buf: &'buf mut RingBuffer<A, CAP>) -> Self {
        debug_assert!(buf.has_remaining());
        Self(buf)
    }
}

impl<A: Copy, const CAP: usize> VacantEntry<'_, A, CAP> {
    pub fn write(&mut self, item: A) {
        // SAFETY: the invariant of `Self` is that `self.has_remaining()`
        unsafe { self.0.push_unchecked(item) };
    }
}
