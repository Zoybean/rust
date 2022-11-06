use core::clone::Clone;

use crate::iter::{FusedIterator, TrustedLen};

/// An iterator adaptor which conservatively clones an element to pair with
/// elements from the underlying iterator
///
/// This `enum` is created by [`Iterator::zip_clones`]. See its documentation
/// for more information.
#[unstable(feature = "iter_zip_clones", reason = "recently added", issue = "none")]
#[derive(Debug, Clone)]
pub enum ZipClone<I, C>
where
    I: Iterator,
    C: Clone,
{
    /// Nothing left from the underlying iterator.
    Empty,
    /// At least one item left from the underlying iterator, plus the cloneable
    /// element to pair with it
    More(I::Item, C, I),
}

#[unstable(feature = "iter_zip_clones", reason = "recently added", issue = "none")]
impl<I, C> ZipClone<I, C>
where
    I: Iterator,
    C: Clone,
{
    /// Create a new `ZipClone` iterator
    pub fn new(mut it: I, to_clone: C) -> Self {
        match it.next() {
            None => Self::Empty,
            Some(next) => Self::More(next, to_clone, it),
        }
    }
}

#[unstable(feature = "iter_zip_clones", reason = "recently added", issue = "none")]
impl<I, C> Iterator for ZipClone<I, C>
where
    C: Clone,
    I: Iterator,
{
    type Item = (I::Item, C);

    fn next(&mut self) -> Option<Self::Item> {
        match core::mem::replace(self, Self::Empty) {
            Self::Empty => None,
            Self::More(last, to_clone, mut it) => {
                if let Some(next) = it.next() {
                    let cloned = to_clone.clone();
                    *self = Self::More(next, cloned, it);
                }
                Some(last, to_clone)
            }
        }
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<I: Iterator, C: Clone> ExactSizeIterator for ZipClone<I, C> where I: ExactSizeIterator {}

#[stable(feature = "fused", since = "1.26.0")]
impl<I: Iterator, C: Clone> FusedIterator for ZipClone<I, C> {}

#[unstable(feature = "trusted_len", issue = "37572")]
unsafe impl<I: Iterator, C: Clone> TrustedLen for ZipClone<I, C> where I: TrustedLen {}
