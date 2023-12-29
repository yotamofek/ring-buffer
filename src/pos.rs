#[derive(Debug, Clone, Copy)]
pub(super) struct Pos<const CAP: usize> {
    // Invariant: `len` <= `CAP`
    len: usize,
    // Invariant: `at` < `CAP` (or `CAP` == 0 and `at` == 0)
    at: usize,
}

impl<const CAP: usize> Pos<CAP> {
    /// Creates a new `Pos` with the given length and starting index.
    ///
    /// # Safety
    /// The following invariants must be held:
    /// - `len` <= `CAP`
    /// - `at` < `CAP` (or `CAP` == 0 and `at` == 0)
    pub const unsafe fn new_unchecked(len: usize, at: usize) -> Self {
        debug_assert!(len <= CAP);
        debug_assert!(at < CAP || (CAP == 0 && at == 0));
        Self { len, at }
    }

    pub const fn zero() -> Self {
        Self { len: 0, at: 0 }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// # Safety
    /// The following invariant must be held:
    /// - `len` <= `CAP`
    #[inline(always)]
    pub unsafe fn set_len(&mut self, len: usize) {
        debug_assert!(len <= CAP);
        self.len = len;
    }

    #[inline(always)]
    pub fn clear_len(&mut self) {
        // SAFETY: `0` is always `<= CAP`
        unsafe { self.set_len(0) };
    }

    #[inline(always)]
    pub const fn at(&self) -> usize {
        self.at
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub const fn is_contiguous(&self) -> bool {
        self.at + self.len <= CAP
    }

    #[inline(always)]
    pub const fn front_len(&self) -> usize {
        self.len - self.back_len()
    }

    #[inline(always)]
    pub const fn back_len(&self) -> usize {
        if CAP > 0 {
            self.logical_index(self.len)
        } else {
            0
        }
    }

    /// Returns the index in the underlying buffer corresponding to the given logical index.
    /// The returned index is guaranteed to be in bounds (i.e. < `CAP`), but the indexed item not necessarily initialized.
    ///
    /// # Panics
    /// Panics if `CAP == 0`.
    #[inline(always)]
    #[track_caller]
    pub const fn logical_index(&self, index: usize) -> usize {
        (self.at + index) % CAP
    }

    pub fn advance(&mut self, n: usize) {
        self.at = self.logical_index(n);
    }
}
