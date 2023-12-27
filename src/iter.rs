use std::{
    iter::{FusedIterator, TrustedLen, TrustedRandomAccess, TrustedRandomAccessNoCoerce},
    marker::PhantomData,
    mem::MaybeUninit,
    num::NonZeroUsize,
};

use crate::{Pos, RingBuffer};

macro_rules! iter {
    ($name:ident(*$raw_mut:tt T, {$( $mut_:tt )?}, $assume_init_ref:ident)) => {
        pub struct $name<'buf, A: Copy, const CAP: usize> {
            buf: *$raw_mut [MaybeUninit<A>; CAP],
            pos: Pos<CAP>,
            _marker: PhantomData<&'buf $($mut_)? RingBuffer<A, CAP>>,
        }

        impl<'buf, A: Copy, const CAP: usize> $name<'buf, A, CAP> {
            pub(crate) fn new(buf: &'buf $($mut_)? [MaybeUninit<A>; CAP], len: usize, at: usize) -> Self {
                Self {
                    buf,
                    pos: Pos::new(len, at),
                    _marker: PhantomData,
                }
            }

            /// Returns a pointer to the item at the given index without doing bounds checks. Also assumes that the item is initialized.
            #[inline(always)]
            fn get_unchecked(& $($mut_)? self, index: usize) -> *$raw_mut A {
                let index = self.pos.logical_index(index);
                let ptr = self.buf.cast::<A>();
                ptr.wrapping_add(index)
            }
        }

        impl<A: Copy, const CAP: usize> Clone for $name<'_, A, CAP> {
            fn clone(&self) -> Self {
                Self {
                    buf: self.buf,
                    pos: self.pos,
                    _marker: PhantomData,
                }
            }
        }

        impl<'buf, A: Copy, const CAP: usize> Iterator for $name<'buf, A, CAP> {
            type Item = &'buf $($mut_)? A;

            fn next(&mut self) -> Option<Self::Item> {
                self.pos.set_len(self.pos.len().checked_sub(1)?);
                let item = self.get_unchecked(0);
                self.pos.advance(1);
                Some(unsafe { & $($mut_)? *item })
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                (self.pos.len(), Some(self.pos.len()))
            }

            fn count(self) -> usize {
                self.pos.len()
            }

            fn advance_by(&mut self, n: usize) -> Result<(), NonZeroUsize> {
                match self.pos.len().checked_sub(n) {
                    Some(left) => {
                        self.pos.set_len(left);
                        self.pos.advance(n);
                        Ok(())
                    }
                    None => {
                        // `n > self.len`, because otherwise checked_sub would have returned Some
                        let left = unsafe { NonZeroUsize::new_unchecked(n - self.pos.len()) };
                        self.pos.set_len(0);
                        Err(left)
                    }
                }
            }

            unsafe fn __iterator_get_unchecked(&mut self, idx: usize) -> Self::Item
            where
                Self: TrustedRandomAccessNoCoerce,
            {
                & $($mut_)? *self.get_unchecked(idx)
            }
        }

        impl<A: Copy, const CAP: usize> DoubleEndedIterator for $name<'_, A, CAP> {
            fn next_back(&mut self) -> Option<Self::Item> {
                self.pos.set_len(self.pos.len().checked_sub(1)?);
                let item = self.get_unchecked(self.pos.len());
                Some(unsafe { & $($mut_)? *item })
            }

            fn advance_back_by(&mut self, n: usize) -> Result<(), NonZeroUsize> {
                match self.pos.len().checked_sub(n) {
                    Some(left) => {
                        self.pos.set_len(left);
                        Ok(())
                    }
                    None => {
                        // `n > self.len`, because otherwise checked_sub
                        // would have returned Some
                        let left = unsafe { NonZeroUsize::new_unchecked(n - self.pos.len()) };
                        self.pos.set_len(0);
                        Err(left)
                    }
                }
            }
        }

        impl<A: Copy, const CAP: usize> FusedIterator for $name<'_, A, CAP> {}

        impl<A: Copy, const CAP: usize> ExactSizeIterator for $name<'_, A, CAP> {
            fn len(&self) -> usize {
                self.pos.len()
            }
        }

        unsafe impl<A: Copy, const CAP: usize> TrustedLen for $name<'_, A, CAP> {}

        unsafe impl<A: Copy, const CAP: usize> TrustedRandomAccessNoCoerce
            for $name<'_, A, CAP>
        {
            const MAY_HAVE_SIDE_EFFECT: bool = false;
        }

        unsafe impl<A: Copy, const CAP: usize> TrustedRandomAccess for $name<'_, A, CAP> {}
    };
}

iter!(Iter(*const T, {/* no mut */}, assume_init_ref));
iter!(IterMut(*mut T, {mut}, assume_init_mut));

unsafe impl<'buf, A: Copy + Sync, const CAP: usize> Sync for Iter<'buf, A, CAP> {}
unsafe impl<'buf, A: Copy + Sync, const CAP: usize> Send for Iter<'buf, A, CAP> {}

unsafe impl<'buf, A: Copy + Sync, const CAP: usize> Sync for IterMut<'buf, A, CAP> {}
unsafe impl<'buf, A: Copy + Send, const CAP: usize> Send for IterMut<'buf, A, CAP> {}

#[cfg(test)]
mod tests {
    use crate::RingBuffer;

    #[test]
    fn test_iter_advance_by() {
        // test non-overflowing buffer

        let arr = RingBuffer::<_, 5>::from([0, 1, 2]);
        let mut iter = arr.iter();

        assert_eq!(iter.len(), 3);
        assert!(iter.clone().eq(&[0, 1, 2]));

        assert_eq!(iter.advance_by(1), Ok(()));
        assert_eq!(iter.len(), 2);
        assert!(iter.clone().eq(&[1, 2]));

        assert_eq!(iter.advance_by(2), Ok(()));
        assert_eq!(iter.len(), 0);
        assert!(iter.next().is_none());

        assert_eq!(iter.advance_by(1).unwrap_err().get(), 1);

        assert_eq!(arr.iter().advance_by(3), Ok(()));
        assert_eq!(arr.iter().advance_by(4).unwrap_err().get(), 1);

        // test overflowing buffer

        let mut arr = RingBuffer::<_, 5>::from([0, 1, 2, 3, 4]);
        assert_eq!(arr.pop_first(), Some(0));
        assert_eq!(arr.pop_first(), Some(1));
        arr.with_vacancy().unwrap().write(5);
        arr.with_vacancy().unwrap().write(6);
        assert_ne!(arr.pos.at(), 0);

        let mut iter = arr.iter();

        assert_eq!(iter.len(), 5);
        assert!(iter.clone().eq(&[2, 3, 4, 5, 6]));

        assert_eq!(iter.advance_by(1), Ok(()));
        assert_eq!(iter.len(), 4);
        assert!(iter.clone().eq(&[3, 4, 5, 6]));

        assert_eq!(iter.advance_by(3), Ok(()));
        assert_eq!(iter.len(), 1);
        assert!(iter.clone().eq(&[6]));

        assert_eq!(iter.advance_by(1), Ok(()));
        assert_eq!(iter.len(), 0);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_iter_double_ended() {
        let mut arr = RingBuffer::<_, 5>::from([0, 1, 2, 3, 4]);
        assert_eq!(arr.pop_first(), Some(0));
        assert_eq!(arr.pop_first(), Some(1));
        arr.with_vacancy().unwrap().write(5);
        arr.with_vacancy().unwrap().write(6);
        assert_ne!(arr.pos.at(), 0);

        assert!(arr.iter().rev().eq(&[6, 5, 4, 3, 2]));

        let mut iter = arr.iter();

        assert_eq!(iter.next_back(), Some(&6));
        assert_eq!(iter.len(), 4);
        assert!(iter.clone().eq(&[2, 3, 4, 5]));

        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next_back(), Some(&5));
        assert_eq!(iter.len(), 2);
        assert!(iter.clone().eq(&[3, 4]));

        assert_eq!(iter.next(), Some(&3));
        assert_eq!(iter.next_back(), Some(&4));
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());

        // test advance_back_by

        let mut iter = arr.iter();
        assert_eq!(iter.advance_back_by(2), Ok(()));
        assert_eq!(iter.len(), 3);
        assert!(iter.clone().eq(&[2, 3, 4]));

        assert_eq!(iter.advance_back_by(1), Ok(()));
        assert_eq!(iter.len(), 2);
        assert!(iter.clone().eq(&[2, 3]));

        assert_eq!(iter.advance_back_by(2), Ok(()));
        assert_eq!(iter.len(), 0);
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());

        assert_eq!(arr.iter().advance_back_by(5), Ok(()));
        assert_eq!(arr.iter().advance_back_by(6).unwrap_err().get(), 1);
    }

    #[test]
    fn test_iter_trusted_random_access() {
        let mut arr = RingBuffer::<_, 5>::from([0, 1, 2, 3, 4]);
        assert_eq!(arr.pop_first(), Some(0));
        assert_eq!(arr.pop_first(), Some(1));
        arr.with_vacancy().unwrap().write(5);
        arr.with_vacancy().unwrap().write(6);
        assert_ne!(arr.pos.at(), 0);

        arr.iter_mut()
            // this invokes __iterator_get_unchecked
            .zip(&[1, 2, 3, 4, 5])
            .for_each(|(a, b)| *a *= b);

        assert!(arr.iter().eq(&[2, 6, 12, 20, 30]));
    }
}
