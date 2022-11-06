use core::cell::RefCell;

#[test]
fn test() {
    test_counter::<0>();
    test_counter::<1>();
    test_counter::<2>();
    test_counter::<3>();
    test_counter::<4>();
    test_counter::<5>();
    test_counter::<1000>();
}

/// Tests that the number of times clone is called is exactly 1 less than
/// the number of items in the original iterator, or 0 if the iterator is empty to start.
fn test_counter<const N: usize>() {
    let count = RefCell::new(0);
    let counter = Counter(&count);
    let _ = [(); N].into_iter().zip_clones(counter).for_each(|_| {});
    // 1 fewer clone than the number of items in the collection
    assert_eq!(N.saturating_sub(1), *count.borrow());
}

struct Counter<'a>(&'a RefCell<usize>);

impl<'a> Clone for Counter<'a> {
    fn clone(&self) -> Self {
        *self.0.borrow_mut() += 1;
        Self(self.0)
    }
}
