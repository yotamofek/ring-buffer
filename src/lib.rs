#![allow(incomplete_features)]
#![feature(
    allocator_api,
    const_maybe_uninit_uninit_array,
    generic_const_exprs,
    iter_advance_by,
    maybe_uninit_array_assume_init,
    maybe_uninit_slice,
    maybe_uninit_uninit_array,
    min_specialization,
    new_uninit,
    slice_ptr_get,
    slice_ptr_len,
    slice_split_at_unchecked,
    trusted_len,
    trusted_random_access
)]

pub mod entry;
pub mod error;
pub mod iter;
mod pos;

use std::{
    alloc::{Allocator, Global, Layout},
    fmt::{Debug, Formatter},
    hash::{Hash, Hasher},
    hint::unreachable_unchecked,
    mem::{self, MaybeUninit},
    ptr,
};

pub use self::{
    error::BufferFullError,
    iter::{Iter, IterMut},
};

use self::{entry::VacantEntry, pos::Pos};

/// Very simple ring buffer that can hold up to `CAP` items of type `A`.
#[derive(Clone, Copy)]
pub struct RingBuffer<A: Copy, const CAP: usize> {
    // Invariant: at least `len` items are initialized, starting from `at` and
    // circling back to the beginning of the buf if overflowing `CAP`
    buf: [MaybeUninit<A>; CAP],
    pos: Pos<CAP>,
}

impl<A: Copy, const CAP: usize> Default for RingBuffer<A, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Copy, const CAP: usize> RingBuffer<A, CAP> {
    /// Creates a new empty ring buffer.
    pub const fn new() -> Self {
        Self {
            buf: MaybeUninit::uninit_array(),
            pos: Pos::zero(),
        }
    }

    /// Returns the number of items in the ring buffer.
    pub const fn len(&self) -> usize {
        self.pos.len()
    }

    /// Returns `true` if the ring buffer is empty.
    pub const fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    /// Returns `true` if the ring buffer is full.
    ///
    /// Note that it is equivalent to `!self.has_remaining()`, unless `CAP` is 0 in which case both `self.has_remaining()` and `self.is_full()` will return false.
    pub const fn is_full(&self) -> bool {
        self.pos.is_full()
    }

    /// Returns the number of items that can be added to the ring buffer before it is full.
    ///
    /// Same as `CAP - self.len()`.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// assert_eq!(buf.remaining(), 3);
    /// buf.extend([0, 1]);
    /// assert_eq!(buf.remaining(), 1);
    /// buf.push(2);
    /// assert_eq!(buf.remaining(), 0);
    /// assert_eq!(buf.pop_first(), Some(0));
    /// assert_eq!(buf.remaining(), 1);
    /// ```
    pub const fn remaining(&self) -> usize {
        CAP - self.len()
    }

    /// Returns `true` if the ring buffer has capacity for at least one more item.
    ///
    /// Equivalent to `self.remaining() > 0`.
    ///
    /// Note that it is equivalent to `!self.is_full()`, unless `CAP` is 0 in which case both `self.has_remaining()` and `self.is_full()` will return false.
    pub const fn has_remaining(&self) -> bool {
        self.remaining() > 0
    }

    /// Returns a reference to the first, possibly uninitialized item.
    ///
    /// # Safety
    /// `CAP` must be greater than 0.
    /// It *is* safe to call this method on an empty ring buffer.
    #[inline(always)]
    unsafe fn first_item(&mut self) -> &mut MaybeUninit<A> {
        debug_assert!(CAP > 0);
        self.buf.get_unchecked_mut(self.pos.at())
    }

    /// Returns a reference to the item past the last item, where the next item would be written.
    ///
    /// # Safety
    /// `CAP` must be greater than 0.
    /// It *is* safe to call this method on an empty ring buffer.
    #[inline(always)]
    unsafe fn next_item(&mut self) -> &mut MaybeUninit<A> {
        debug_assert!(CAP > 0);
        let index = self.pos.logical_index(self.len());
        self.buf.get_unchecked_mut(index)
    }

    /// Returns a reference to the item at the given index without doing bounds checks. Also assumes that the item is initialized.
    ///
    /// # Safety
    /// The given index must be less than `self.len()`.
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> &A {
        debug_assert!(index < self.len());
        let index = self.pos.logical_index(index);
        self.buf.get_unchecked(index).assume_init_ref()
    }

    /// Returns a mutable reference to the item at the given index without doing bounds checks. Also assumes that the item is initialized.
    ///
    /// # Safety
    /// The given index must be less than `self.len()`.
    #[inline]
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut A {
        debug_assert!(index < self.len());
        let index = self.pos.logical_index(index);
        self.buf.get_unchecked_mut(index).assume_init_mut()
    }

    /// Returns a reference to the item at the given index, or `None` if the index is out of bounds.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let buf = RingBuffer::<_, 3>::from([0, 1]);
    /// assert_eq!(buf.get(0), Some(&0));
    /// assert_eq!(buf.get(1), Some(&1));
    /// assert_eq!(buf.get(2), None);
    /// ````
    pub fn get(&self, index: usize) -> Option<&A> {
        if index >= self.len() {
            None
        } else {
            Some(unsafe { self.get_unchecked(index) })
        }
    }

    /// Returns a mutable reference to the item at the given index, or `None` if the index is out of bounds.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([1, 2]);
    /// *buf.get_mut(0).unwrap() *= 2;
    /// *buf.get_mut(1).unwrap() *= 3;
    /// assert_eq!(buf.get_mut(2), None);
    /// assert_eq!(buf, [2, 6]);
    /// ```
    pub fn get_mut(&mut self, index: usize) -> Option<&mut A> {
        if index >= self.len() {
            None
        } else {
            Some(unsafe { self.get_unchecked_mut(index) })
        }
    }

    /// Removes the first item from the ring buffer and returns it, or `None` if the buffer [is empty](Self::is_empty).
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// assert_eq!(buf.pop_first(), None);
    /// buf.extend([0, 1]);
    /// assert_eq!(buf.pop_first(), Some(0));
    /// assert_eq!(buf.pop_first(), Some(1));
    /// assert_eq!(buf.pop_first(), None);
    /// ```
    #[inline]
    pub fn pop_first(&mut self) -> Option<A> {
        if self.is_empty() {
            return None;
        }

        let item = mem::replace(
            // SAFETY: `CAP` > 0, otherwise `self.is_empty()` would have returned `true`
            unsafe { self.first_item() },
            MaybeUninit::uninit(),
        );
        // SAFETY: buffer is non-empty, so has at least one item
        let item = unsafe { item.assume_init() };
        self.pos.advance(1);
        // SAFETY: `0 < self.len() <= CAP`, so `self.len() - 1` will not underflow and is in bounds
        unsafe { self.pos.set_len(self.len() - 1) };
        Some(item)
    }

    /// If the ring [has remaining capacity](Self::has_remaining), returns a [`VacantEntry`] that can be used to push a new item to the end of the ring buffer.
    ///
    /// See also [`try_push`](Self::try_push) and [`try_push_with`](Self::try_push_with).
    pub fn with_vacancy(&mut self) -> Option<VacantEntry<A, CAP>> {
        if self.has_remaining() {
            // SAFETY: `self.has_remaining()` returned `true`, so `self.len() < CAP`
            Some(unsafe { VacantEntry::new_unchecked(self) })
        } else {
            None
        }
    }

    /// Adds an item to the end of the ring buffer, removing the first item if the buffer [is full](Self::is_full).
    /// Returns the removed item if the buffer was full, otherwise `None`.
    ///
    /// # Panics
    /// Panics if `CAP` is 0.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// assert_eq!(buf.pop_push(0), None);
    /// assert_eq!(buf, [0]);
    /// assert_eq!(buf.pop_push(1), None);
    /// assert_eq!(buf, [0, 1]);
    /// assert_eq!(buf.pop_push(2), None);
    /// assert_eq!(buf, [0, 1, 2]);
    /// assert_eq!(buf.pop_push(3), Some(0));
    /// assert_eq!(buf, [1, 2, 3]);
    /// assert_eq!(buf.pop_push(4), Some(1));
    /// assert_eq!(buf, [2, 3, 4]);
    /// ```
    #[track_caller]
    #[inline]
    pub fn pop_push(&mut self, item: A) -> Option<A> {
        assert!(CAP > 0);

        if self.is_full() {
            let item = mem::replace(
                // SAFETY: `CAP > 0` was asserted above
                unsafe { self.first_item() },
                MaybeUninit::new(item),
            );
            // SAFETY: buffer is full, so has at least one item
            let item = unsafe { item.assume_init() };
            self.pos.advance(1);
            Some(item)
        } else {
            // SAFETY `CAP > 0` was asserted above
            unsafe { self.next_item() }.write(item);
            // SAFETY: `self.len() < CAP` (since `self.is_full()` returned false), so `self.len() + 1` will not overflow and is in bounds
            unsafe { self.pos.set_len(self.len() + 1) };
            None
        }
    }

    /// Tries to add an item to the end of the ring buffer, if the buffer [has remaining capacity](Self::has_remaining).
    /// Returns `Ok(())` if successful, otherwise returns `Err(BufferFullError)`.
    ///
    /// The item is constructed by calling the given closure, which is only called if the buffer has remaining capacity.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1]);
    /// let mut call_count = 0;
    /// assert!(
    ///     buf.try_push_with(|| {
    ///         call_count += 1;
    ///         2
    ///     }
    /// ).is_ok());
    /// assert_eq!(call_count, 1);
    /// assert_eq!(buf, [0, 1, 2]);
    /// assert!(
    ///     buf.try_push_with(|| {
    ///         call_count += 1;
    ///         3
    ///     }
    /// ).is_err());
    /// assert_eq!(call_count, 1);
    /// ```
    #[inline]
    pub fn try_push_with<F: FnOnce() -> A>(&mut self, f: F) -> Result<(), BufferFullError> {
        self.with_vacancy()
            .ok_or(BufferFullError::new())?
            .write(f());
        Ok(())
    }

    /// Adds an item to the end of the ring buffer, assuming the buffer [has remaining capacity](Self::has_remaining).
    ///
    /// # Safety
    /// The following invariant must be held:
    /// - `self.has_remaining()` (which implies `CAP > 0`)
    #[inline]
    pub unsafe fn push_unchecked(&mut self, item: A) {
        debug_assert!(self.has_remaining());
        self.next_item().write(item);
        self.pos.set_len(self.len() + 1);
    }

    /// Tries to add an item to the end of the ring buffer, if the buffer [has remaining capacity](Self::has_remaining).
    /// Returns `Ok(())` if successful, otherwise returns `Err(BufferFullError)`.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([1, 2]);
    /// assert_eq!(buf.try_push(3), Ok(()));
    /// assert_eq!(buf, [1, 2, 3]);
    /// assert!(buf.try_push(4).is_err());
    /// ```
    #[inline]
    pub fn try_push(&mut self, item: A) -> Result<(), BufferFullError> {
        self.try_push_with(|| item)
    }

    /// Adds an item to the end of the ring buffer.
    ///
    /// # Panics
    /// Panics if the buffer [has no remaining capacity](Self::has_remaining).
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// buf.push(0);
    /// assert_eq!(buf, [0]);
    /// buf.push(1);
    /// assert_eq!(buf, [0, 1]);
    /// buf.push(2);
    /// assert_eq!(buf, [0, 1, 2]);
    /// ```
    ///
    /// ```should_panic
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1, 2]);
    /// buf.push(3);
    /// ```
    #[track_caller]
    #[inline]
    pub fn push(&mut self, item: A) {
        self.try_push(item).unwrap()
    }

    /// Returns an iterator over the items in the ring buffer.
    pub fn iter(&self) -> Iter<'_, A, CAP> {
        Iter::new(&self.buf, self.pos)
    }

    /// Returns an iterator over the mutable references to the items in the ring buffer.
    pub fn iter_mut(&mut self) -> IterMut<'_, A, CAP> {
        IterMut::new(&mut self.buf, self.pos)
    }

    /// Removes all items from the ring buffer.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Copies the contents of the ring buffer into `dst` in a contiguous manner.
    ///
    /// # Safety
    /// It must be valid to copy `self.len` items of type `A` into `dst`.
    /// `dst` must not overlap with `self.buf`.
    #[inline(always)]
    unsafe fn copy_into(&self, dst: *mut [MaybeUninit<A>]) {
        debug_assert!(dst.len() >= self.len());
        let front = self.front_slice();
        let back = self.back_slice();

        dst.as_mut_ptr()
            .copy_from_nonoverlapping(front.as_ptr().cast(), front.len());
        dst.as_mut_ptr()
            .add(front.len())
            .copy_from_nonoverlapping(back.as_ptr().cast(), back.len());
    }

    /// Returns a slice containing the contents of the ring buffer, assuming ring buffer is layed out contiguously in memory.
    ///
    /// # Safety
    /// The following invariant must be held:
    /// - `self.pos.is_contiguous()`
    #[inline]
    pub unsafe fn as_slice_unchecked(&self) -> &[A] {
        debug_assert!(self.pos.is_contiguous());
        let at = self.pos.at();
        let len = self.len();
        MaybeUninit::slice_assume_init_ref(self.buf.get_unchecked(at..at + len))
    }

    /// Returns a mutable slice containing the contents of the ring buffer, assuming ring buffer is layed out contiguously in memory.
    ///
    /// # Safety
    /// The following invariant must be held:
    /// - `self.pos.is_contiguous()`
    #[inline]
    pub unsafe fn as_slice_unchecked_mut(&mut self) -> &mut [A] {
        debug_assert!(self.pos.is_contiguous());
        let at = self.pos.at();
        let len = self.len();
        MaybeUninit::slice_assume_init_mut(self.buf.get_unchecked_mut(at..at + len))
    }

    /// Returns a slice reference containing the contents of the ring buffer, if the ring buffer is layed out contiguously in memory.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1]);
    /// assert_eq!(buf.as_slice(), Some(&[0, 1][..]));
    /// buf.push(2);
    /// assert_eq!(buf.as_slice(), Some(&[0, 1, 2][..]));
    /// buf.pop_first();
    /// assert_eq!(buf.as_slice(), Some(&[1, 2][..]));
    /// buf.push(3);
    /// assert_eq!(buf.as_slice(), None);
    /// buf.extend([4, 5]);
    /// assert_eq!(buf.as_slice(), Some(&[3, 4, 5][..]));
    /// ```
    #[inline]
    pub fn as_slice(&self) -> Option<&[A]> {
        self.pos.is_contiguous().then(|| {
            // SAFETY: `self.pos.is_contiguous()` returned `true`
            unsafe { self.as_slice_unchecked() }
        })
    }

    /// Returns a slice mutable slice reference containing the contents of the ring buffer, if the ring buffer is layed out contiguously in memory.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1]);
    /// buf.as_slice_mut().unwrap().copy_from_slice(&[2, 3]);
    /// assert_eq!(buf, [2, 3]);
    /// buf.extend([4, 5]);
    /// assert_eq!(buf.as_slice_mut(), None);
    /// buf.extend([6, 7]);
    /// buf.as_slice_mut().unwrap().copy_from_slice(&[8, 9, 10]);
    /// assert_eq!(buf, [8, 9, 10]);
    /// ```
    #[inline]
    pub fn as_slice_mut(&mut self) -> Option<&mut [A]> {
        self.pos.is_contiguous().then(|| {
            // SAFETY: `self.pos.is_contiguous()` returned `true`
            unsafe { self.as_slice_unchecked_mut() }
        })
    }

    /// Returns a slice reference containing the front part of the ring buffer.
    ///
    /// If the ring buffer is not layed out contiguously in memory,
    /// this slice will not contain all the items, and the rest of the items will be in the [back part of the ring buffer](Self::back_slice).
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1, 2]);
    /// assert_eq!(buf.front_slice(), &[0, 1, 2][..]);
    /// assert_eq!(buf.pop_first(), Some(0));
    /// assert_eq!(buf.front_slice(), &[1, 2][..]);
    /// buf.push(3);
    /// assert_eq!(buf.front_slice(), &[1, 2][..]);
    /// buf.pop_push(4);
    /// assert_eq!(buf.front_slice(), &[2][..]);
    /// ```
    pub fn front_slice(&self) -> &[A] {
        let at = self.pos.at();
        let len = self.pos.front_len();
        unsafe { MaybeUninit::slice_assume_init_ref(self.buf.get_unchecked(at..at + len)) }
    }

    /// Returns a mutable slice reference containing the front part of the ring buffer.
    ///
    /// If the ring buffer is not layed out contiguously in memory,
    /// this slice will not contain all the items, and the rest of the items will be in the [back part of the ring buffer](Self::back_slice_mut).
    ///
    /// See also [`split_mut_slices`](Self::split_mut_slices).
    pub fn front_slice_mut(&mut self) -> &mut [A] {
        self.split_mut_slices().0
    }

    /// Returns a slice reference containing the back part of the ring buffer.
    ///
    /// If the ring buffer is layed out contiguously in memory,
    /// this slice will be empty. Otherwise, it will contain all the items that are not in the [front part of the ring buffer](Self::front_slice).
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::from([0, 1, 2]);
    /// assert_eq!(buf.back_slice(), &[][..]);
    /// // at this point, the next item will be written to the back part of the buffer
    /// assert_eq!(buf.pop_push(3), Some(0));
    /// assert_eq!(buf.back_slice(), &[3][..]);
    /// ```
    pub fn back_slice(&self) -> &[A] {
        let len = self.pos.back_len();
        unsafe { MaybeUninit::slice_assume_init_ref(self.buf.get_unchecked(..len)) }
    }

    /// Returns a mutable slice reference containing the back part of the ring buffer.
    ///
    /// If the ring buffer is layed out contiguously in memory,
    /// this slice will be empty. Otherwise, it will contain all the items that are not in the [front part of the ring buffer](Self::front_slice_mut).
    ///
    /// See also [`split_mut_slices`](Self::split_mut_slices).
    pub fn back_slice_mut(&mut self) -> &mut [A] {
        self.split_mut_slices().1
    }

    /// Splits the ring buffer into two mutable slice references, one containing the front part of the ring buffer, and one containing the back part.
    ///
    /// If the ring buffer is layed out contiguously in memory,
    /// the back slice will be empty, and the front slice will contain all the items.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// let (front, back) = buf.split_mut_slices();
    /// assert_eq!(front, &[][..]);
    /// assert_eq!(back, &[][..]);
    /// buf.extend([0, 1, 2]);
    /// let (front, back) = buf.split_mut_slices();
    /// assert_eq!(front, &[0, 1, 2][..]);
    /// assert_eq!(back, &[][..]);
    /// buf.pop_push(3);
    /// let (front, back) = buf.split_mut_slices();
    /// front.copy_from_slice(&[4, 5]);
    /// back.copy_from_slice(&[6]);
    /// assert_eq!(buf, [4, 5, 6]);
    /// buf.pop_push(7);
    /// let (front, back) = buf.split_mut_slices();
    /// front.copy_from_slice(&[8]);
    /// back.copy_from_slice(&[9, 10]);
    /// assert_eq!(buf, [8, 9, 10]);
    /// ```
    pub fn split_mut_slices(&mut self) -> (&mut [A], &mut [A]) {
        let front_len = self.pos.front_len();
        let back_len = self.pos.back_len();
        let (back, front) = unsafe { self.buf.split_at_mut_unchecked(self.pos.at()) };
        let front =
            unsafe { MaybeUninit::slice_assume_init_mut(front.get_unchecked_mut(..front_len)) };
        let back =
            unsafe { MaybeUninit::slice_assume_init_mut(back.get_unchecked_mut(..back_len)) };
        (front, back)
    }

    /// Copies the contents of the ring buffer into an array, if the ring buffer [is full](Self::is_full).
    /// Otherwise, returns `None`.
    ///
    /// # Examples
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// assert_eq!(RingBuffer::<_, 3>::from([0, 1, 2]).to_array(), Some([0, 1, 2]));
    /// assert_eq!(RingBuffer::<_, 3>::from([0, 1]).to_array(), None);
    /// ```
    #[inline]
    pub fn to_array(&self) -> Option<[A; CAP]> {
        if !self.is_full() {
            return None;
        }

        let mut arr = MaybeUninit::uninit_array();
        // SAFETY: `self.len == CAP` and `arr` is of length `CAP`
        unsafe { self.copy_into(&mut arr) };

        let arr = unsafe { MaybeUninit::array_assume_init(arr) };
        Some(arr)
    }

    /// Copies the contents of the ring buffer into a `Vec`, using the given allocator.
    #[inline]
    pub fn to_vec_in<B: Allocator>(&self, alloc: B) -> Vec<A> {
        match Layout::array::<A>(self.len()) {
            Ok(_) => {}
            Err(_) =>
            // SAFETY: we already have an array of at least `self.len` items in `self.buf`,
            // so we know that the layout is valid
            unsafe {
                // this allows the compiler to elide a check and panicking branch in `RawVec::allocate_in`
                unreachable_unchecked()
            },
        };

        let mut slice = Box::new_uninit_slice_in(self.len(), alloc);
        unsafe { self.copy_into(&mut *slice) };
        let slice = unsafe { Box::<[_], _>::assume_init(slice) };
        slice.to_vec()
    }

    /// Copies the contents of the ring buffer into a `Vec`.
    pub fn to_vec(&self) -> Vec<A> {
        self.to_vec_in(Global)
    }
}

/// # Examples
///
/// ```rust
/// # use ring_buffer::RingBuffer;
/// assert!(RingBuffer::<_, 3>::from([0, 1, 2]).iter().eq(&[0, 1, 2]));
/// assert!(RingBuffer::<_, 3>::from([0, 1]).iter().eq(&[0, 1]));
/// ```
///
/// ```compile_fail
/// # use ring_buffer::RingBuffer;
/// RingBuffer::<_, 3>::from([0, 1, 2, 3]);
/// ```
impl<A: Copy, const CAP: usize, const N: usize> From<[A; N]> for RingBuffer<A, CAP>
where
    [(); CAP - N]:,
{
    fn from(arr: [A; N]) -> Self {
        let mut buf = MaybeUninit::uninit_array();
        unsafe {
            ptr::copy_nonoverlapping(arr.as_ptr().cast::<MaybeUninit<A>>(), buf.as_mut_ptr(), N)
        };
        Self {
            buf,
            // SAFETY: `CAP - N` does not underflow, so `N <= CAP`
            pos: unsafe { Pos::new_unchecked(N, 0) },
        }
    }
}

/// Extends the ring buffer with the contents of the given iterator, popping items from the front
/// of the ring buffer if necessary.
///
/// # Panics
/// Panics if `CAP` is 0.
///
/// # Examples
/// ```
/// # use ring_buffer::RingBuffer;
/// let mut buf = RingBuffer::<_, 3>::new();
/// buf.extend([0, 1]);
/// assert_eq!(buf, [0, 1]);
/// buf.extend([2, 3]);
/// assert_eq!(buf, [1, 2, 3]);
/// ```
impl<A: Copy, const CAP: usize> Extend<A> for RingBuffer<A, CAP> {
    #[track_caller]
    fn extend<T: IntoIterator<Item = A>>(&mut self, iter: T) {
        for item in iter {
            self.pop_push(item);
        }
    }
}

/// Creates a new ring buffer from the given iterator.
///
/// Unlike the implementation of `Extend`, if the iterator yields more than `CAP` items, a panic occurs.
///
/// # Examples
/// ```
/// # use ring_buffer::RingBuffer;
/// assert_eq!(RingBuffer::<_, 3>::from_iter([0, 1]), &[0, 1]);
/// assert_eq!([0, 1, 2].into_iter().collect::<RingBuffer::<_, 3>>(), &[0, 1, 2]);
/// ```
///
/// ```should_panic
/// # use ring_buffer::RingBuffer;
/// RingBuffer::<_, 3>::from_iter([0, 1, 2, 3]);
/// ```
impl<A: Copy, const CAP: usize> FromIterator<A> for RingBuffer<A, CAP> {
    #[track_caller]
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
        let mut buf = Self::new();
        for item in iter {
            buf.push(item);
        }
        buf
    }
}

impl<A: Copy + PartialEq, const CAP: usize> PartialEq for RingBuffer<A, CAP> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<A: Copy + Eq, const CAP: usize> Eq for RingBuffer<A, CAP> {}

impl<A: Copy + PartialOrd, const CAP: usize> PartialOrd for RingBuffer<A, CAP> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<A: Copy + Ord, const CAP: usize> Ord for RingBuffer<A, CAP> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<A: Copy + Hash, const CAP: usize> Hash for RingBuffer<A, CAP> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.iter().for_each(|item| item.hash(state))
    }
}

impl<A: Copy + PartialEq, B: AsRef<[A]> + ?Sized, const CAP: usize> PartialEq<B>
    for RingBuffer<A, CAP>
{
    fn eq(&self, other: &B) -> bool {
        self.iter().eq(other.as_ref())
    }
}

impl<A: Copy + Debug, const CAP: usize> Debug for RingBuffer<A, CAP> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'buf, A: Copy, const CAP: usize> IntoIterator for &'buf RingBuffer<A, CAP> {
    type Item = &'buf A;
    type IntoIter = Iter<'buf, A, CAP>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'buf, A: Copy, const CAP: usize> IntoIterator for &'buf mut RingBuffer<A, CAP> {
    type Item = &'buf mut A;
    type IntoIter = IterMut<'buf, A, CAP>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test() {
        let mut arr = RingBuffer::<_, 3>::new();
        assert_eq!(arr.pop_first(), None);
        assert_eq!(arr.iter().next(), None);
        assert_eq!(arr, []);
        assert!(arr.is_empty());
        assert!(!arr.is_full());

        arr.with_vacancy().unwrap().write(0);
        arr.with_vacancy().unwrap().write(1);
        assert_eq!(arr, [0, 1]);
        assert_eq!(arr.len(), 2);
        assert!(!arr.is_empty());
        assert!(!arr.is_full());

        // fill cap
        arr.with_vacancy().unwrap().write(2);

        assert_eq!(arr, [0, 1, 2]);
        assert_eq!(arr.len(), 3);
        assert!(!arr.is_empty());
        assert!(arr.is_full());
        assert!(arr.with_vacancy().is_none());

        assert_eq!(arr.pop_first(), Some(0));
        assert_eq!(arr, [1, 2]);
        assert!(!arr.is_full());
        assert!(arr.iter().eq(&[1, 2]));

        arr.with_vacancy().unwrap().write(3);
        assert!(arr.is_full());
        assert_eq!(arr, [1, 2, 3]);

        assert_eq!(arr.pop_first(), Some(1));
        assert_eq!(arr.pop_first(), Some(2));
        assert_eq!(arr.pop_first(), Some(3));
        assert!(arr.is_empty());
        assert_eq!(arr, []);
    }

    // test to_array
    #[test]
    fn test_to_array() {
        let mut arr = RingBuffer::<_, 3>::new();
        assert_eq!(arr.to_array(), None);

        arr.with_vacancy().unwrap().write(0);
        assert_eq!(arr.to_array(), None);
        assert_eq!(arr.to_vec(), &[0]);

        arr.with_vacancy().unwrap().write(1);
        assert_eq!(arr.to_array(), None);
        assert_eq!(arr.to_vec(), &[0, 1]);

        arr.with_vacancy().unwrap().write(2);
        assert_eq!(arr.to_array(), Some([0, 1, 2]));
        assert_eq!(arr.to_vec(), &[0, 1, 2]);

        assert_eq!(arr.pop_first().unwrap(), 0);
        assert_eq!(arr.pop_first().unwrap(), 1);
        assert_eq!(arr.to_array(), None);
        assert_eq!(arr.to_vec(), &[2]);

        arr.with_vacancy().unwrap().write(3);
        assert_eq!(arr.to_array(), None);
        assert_eq!(arr.to_vec(), &[2, 3]);
        arr.with_vacancy().unwrap().write(4);
        assert_ne!(arr.len(), 0);
        assert_eq!(arr.to_array(), Some([2, 3, 4]));
        assert_eq!(arr.to_vec(), &[2, 3, 4]);
    }

    #[test]
    fn test_zero_cap() {
        let mut arr = RingBuffer::<i32, 0>::from_iter([]);
        assert_eq!(arr.to_array(), Some([]));
        assert_eq!(arr.to_vec(), &[]);
        assert_eq!(arr.pop_first(), None);
        assert!(arr.with_vacancy().is_none());
        assert!(arr.iter().eq(&[]));
        assert!(arr.iter_mut().eq(&[]));
        assert_eq!(arr.get(0), None);
        assert_eq!(arr.get_mut(0), None);
        assert!(arr.is_full());
        assert!(arr.is_empty());
        assert!(!arr.has_remaining());
        assert_eq!(arr.len(), 0);
        assert_eq!(arr.as_slice(), Some(&[][..]));
        assert_eq!(arr.as_slice_mut(), Some(&mut [][..]));
        assert_eq!(arr.front_slice(), &[]);
        assert_eq!(arr.front_slice_mut(), &mut []);
        assert_eq!(arr.back_slice(), &[]);
        assert_eq!(arr.back_slice_mut(), &mut []);
        assert_eq!(arr.split_mut_slices(), (&mut [][..], &mut [][..]));
    }

    #[test]
    #[should_panic = "ring buffer is full"]
    fn test_from_iter_overflow() {
        RingBuffer::<_, 3>::from_iter([0, 1, 2, 3]);
    }

    #[test]
    #[should_panic = "ring buffer is full"]
    fn test_zero_cap_from_iter() {
        RingBuffer::<i32, 0>::from_iter([0, 1, 2]);
    }

    #[test]
    #[should_panic]
    fn test_zero_cap_push() {
        let mut arr = RingBuffer::<i32, 0>::new();
        arr.push(0);
    }

    #[test]
    #[should_panic]
    fn test_zero_cap_extend() {
        let mut arr = RingBuffer::<i32, 0>::new();
        arr.extend([0]);
    }

    #[test]
    #[should_panic]
    fn test_zero_cap_pop_push() {
        let mut arr = RingBuffer::<i32, 0>::new();
        arr.pop_push(0);
    }

    #[test]
    fn test_push() {
        let mut arr = RingBuffer::<_, 3>::new();
        arr.push(0);
        assert_eq!(arr, [0]);
        arr.push(1);
        assert_eq!(arr, [0, 1]);
        arr.push(2);
        assert_eq!(arr, [0, 1, 2]);
    }

    #[test]
    #[should_panic = "ring buffer is full"]
    fn test_push_overflow() {
        let mut arr = RingBuffer::<_, 3>::new();
        arr.push(0);
        arr.push(1);
        arr.push(2);
        arr.push(3);
    }
}
