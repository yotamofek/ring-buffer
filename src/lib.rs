#![allow(incomplete_features)]
#![feature(
    allocator_api,
    const_maybe_uninit_uninit_array,
    generic_const_exprs,
    iter_advance_by,
    maybe_uninit_array_assume_init,
    maybe_uninit_uninit_array,
    min_specialization,
    new_uninit,
    slice_ptr_get,
    trusted_len,
    trusted_random_access
)]

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

use self::{
    iter::{Iter, IterMut},
    pos::Pos,
};

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
            pos: Pos::new(0, 0),
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
    pub const fn is_full(&self) -> bool {
        self.pos.is_full()
    }

    /// Returns the number of items that can be added to the ring buffer before it is full.
    ///
    /// Same as `CAP - self.len()`.
    ///
    /// # Example
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
    /// Same as `self.remaining() > 0`.
    pub const fn has_remaining(&self) -> bool {
        self.remaining() > 0
    }

    /// Returns a reference to the first item in the ring buffer without doing bounds checks.
    ///
    /// # Safety
    /// `CAP` must be greater than 0.
    /// It *is* safe to call this method on an empty ring buffer.
    #[inline(always)]
    unsafe fn first_element(&mut self) -> &mut MaybeUninit<A> {
        debug_assert!(CAP > 0);
        self.buf.get_unchecked_mut(self.pos.at())
    }

    /// Returns a reference to the item at the given index without doing bounds checks. Also assumes that the item is initialized.
    ///
    /// # Safety
    /// The given index must be less than `self.len()`.
    pub unsafe fn get_unchecked(&self, index: usize) -> &A {
        debug_assert!(index < self.len());
        let index = self.pos.logical_index(index);
        self.buf.get_unchecked(index).assume_init_ref()
    }

    /// Returns a mutable reference to the item at the given index without doing bounds checks. Also assumes that the item is initialized.
    ///
    /// # Safety
    /// The given index must be less than `self.len()`.
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut A {
        debug_assert!(index < self.len());
        let index = self.pos.logical_index(index);
        self.buf.get_unchecked_mut(index).assume_init_mut()
    }

    /// Returns a reference to the item at the given index, or `None` if the index is out of bounds.
    ///
    /// # Example:
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
    /// # Example:
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

    /// Removes the first item from the ring buffer and returns it, or `None` if the buffer is empty.
    ///
    /// # Example:
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// let mut buf = RingBuffer::<_, 3>::new();
    /// assert_eq!(buf.pop_first(), None);
    /// buf.extend([0, 1]);
    /// assert_eq!(buf.pop_first(), Some(0));
    /// assert_eq!(buf.pop_first(), Some(1));
    /// assert_eq!(buf.pop_first(), None);
    /// ```
    pub fn pop_first(&mut self) -> Option<A> {
        if self.is_empty() {
            None
        } else {
            let item = mem::replace(
                // SAFETY: `CAP` > 0, otherwise `self.is_empty()` would have returned `true`
                unsafe { self.first_element() },
                MaybeUninit::uninit(),
            );
            // SAFETY: buffer is non-empty, so has at least one element
            let item = unsafe { item.assume_init() };
            self.pos.advance(1);
            self.pos.set_len(self.len() - 1);
            Some(item)
        }
    }

    pub fn with_vacancy(&mut self) -> Option<VacantEntry<A, CAP>> {
        (!self.is_full()).then_some(VacantEntry(self))
    }

    /// Adds an item to the end of the ring buffer, removing the first item if the buffer is full.
    /// Returns the removed item if the buffer was full, otherwise `None`.
    ///
    /// # Panics
    /// Panics if `CAP` is 0.
    ///
    /// # Example
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
    pub fn pop_push(&mut self, item: A) -> Option<A> {
        assert!(CAP > 0);

        if self.is_full() {
            let item = mem::replace(
                // SAFETY: `CAP` > 0, otherwise `self.is_full()` would have returned `false`
                unsafe { self.first_element() },
                MaybeUninit::new(item),
            );
            // SAFETY: buffer is full, so has at least one element
            let item = unsafe { item.assume_init() };
            self.pos.advance(1);
            Some(item)
        } else {
            let index = self.pos.logical_index(self.len());
            // SAFETY: `index` is in bounds
            unsafe {
                self.buf.get_unchecked_mut(index).write(item);
            }
            self.pos.set_len(self.len() + 1);
            None
        }
    }

    /// Adds an item to the end of the ring buffer.
    ///
    /// # Panics
    /// Panics if the buffer is full.
    ///
    /// # Example
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
    pub fn push(&mut self, item: A) {
        self.with_vacancy()
            .expect("ring buffer is full")
            .write(item);
    }

    /// Returns an iterator over the items in the ring buffer.
    pub fn iter(&self) -> Iter<'_, A, CAP> {
        Iter::new(&self.buf, self.len(), self.pos.at())
    }

    /// Returns an iterator over the mutable references to the items in the ring buffer.
    pub fn iter_mut(&mut self) -> IterMut<'_, A, CAP> {
        let len = self.len();
        IterMut::new(&mut self.buf, len, self.pos.at())
    }

    /// Removes all items from the ring buffer.
    pub fn clear(&mut self) {
        self.pos = Pos::default();
    }

    /// Copies the contents of the ring buffer into `dst` in a contiguous manner.
    ///
    /// # Safety
    /// It must be valid to copy `self.len` items of type `A` into `dst`.
    /// `dst` must not overlap with `self.buf`.
    #[inline(always)]
    unsafe fn copy_into(&self, dst: *mut [MaybeUninit<A>]) {
        let front_len = CAP.min(self.pos.at() + self.len()) - self.pos.at();
        let back_len = self.len() - front_len;

        ptr::copy_nonoverlapping::<MaybeUninit<A>>(
            self.buf.as_ptr().add(self.pos.at()),
            dst.as_mut_ptr(),
            front_len,
        );
        ptr::copy_nonoverlapping::<MaybeUninit<A>>(
            self.buf.as_ptr(),
            dst.as_mut_ptr().add(front_len),
            back_len,
        );
    }

    /// Copies the contents of the ring buffer into an array, if the ring buffer is full.
    /// Otherwise, returns `None`.
    ///
    /// # Example:
    /// ```
    /// # use ring_buffer::RingBuffer;
    /// assert_eq!(RingBuffer::<_, 3>::from([0, 1, 2]).to_array(), Some([0, 1, 2]));
    /// assert_eq!(RingBuffer::<_, 3>::from([0, 1]).to_array(), None);
    /// ```
    pub fn to_array(&self) -> Option<[A; CAP]> {
        if self.len() != CAP {
            return None;
        }

        let mut arr = MaybeUninit::uninit_array();
        // SAFETY: `self.len == CAP` and `arr` is of length `CAP`
        unsafe { self.copy_into(&mut arr) };

        let arr = unsafe { MaybeUninit::array_assume_init(arr) };
        Some(arr)
    }

    /// Copies the contents of the ring buffer into a `Vec`, using the given allocator.
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

pub struct VacantEntry<'buf, A: Copy, const CAP: usize>(&'buf mut RingBuffer<A, CAP>);

impl<A: Copy, const CAP: usize> VacantEntry<'_, A, CAP> {
    pub fn write(&mut self, item: A) {
        let Self(buf) = self;

        let index = buf.pos.logical_index(buf.len());
        unsafe { buf.buf.get_unchecked_mut(index).write(item) };
        buf.pos.set_len(buf.len() + 1);
    }
}

/// # Example:
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
            pos: Pos::new(N, 0),
        }
    }
}

/// Extends the ring buffer with the contents of the given iterator, popping items from the front
/// of the ring buffer if necessary.
///
/// # Example:
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
/// # Example:
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
        assert_eq!(arr.len(), 0);
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
