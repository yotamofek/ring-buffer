#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Pos<const CAP: usize> {
    // Invariant: `len` <= `CAP`
    len: usize,
    // Invariant: `at` < `CAP` (or `CAP` == 0 and `at` == 0)
    at: usize,
}

impl<const CAP: usize> Pos<CAP> {
    pub const fn new(len: usize, at: usize) -> Self {
        debug_assert!(len <= CAP);
        debug_assert!(at < CAP || (CAP == 0 && at == 0));
        Self { len, at }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn set_len(&mut self, len: usize) {
        debug_assert!(len <= CAP);
        self.len = len;
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
    pub const fn is_full(&self) -> bool {
        self.len == CAP
    }

    /// Returns the index in the underlying buffer corresponding to the given logical index.
    /// The returned index is guaranteed to be in bounds (i.e. < `CAP`), but the indexed item not necessarily initialized.
    #[inline(always)]
    pub fn logical_index(&self, index: usize) -> usize {
        (self.at + index) % CAP
    }

    pub fn advance(&mut self, n: usize) {
        self.at = self.logical_index(n);
    }
}
