use std::ops::Index;
use std::ops::Deref;
use std::mem;

/// A collection with push- and pop-front methods
/// that reuses existing storage.
///
/// So it's useful only if you'll have less pushes
/// than pops.
pub struct Stream<T> {
    pos: usize,
    storage: Box<[T]>,
}

impl<T: Dummy> Stream<T> {
    pub fn new(data: Box<[T]>) -> Self {
        Stream { pos: 0, storage: data }
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let slot = match self.storage.get_mut(self.pos) {
            Some(slot) => slot,
            None => return None,
        };

        let val = mem::replace(slot, T::dummy());
        self.pos += 1;
        Some(val)
    }

    /// Panics on capacity overflow
    pub fn push_front(&mut self, val: T) {
        if self.pos == 0 {
            panic!("Stream: can't push front for there's no space");
        }

        self.pos -= 1;
        self.storage[self.pos] = val;
    }
}

impl<T> Deref for Stream<T> {
    type Target = [T];
    fn deref(&self) -> &[T] { &self.storage[self.pos..] }
}

impl<Idx, T> Index<Idx> for Stream<T> where [T]: Index<Idx> {
    type Output = <[T] as Index<Idx>>::Output;
    fn index(&self, index: Idx) -> &Self::Output {
        &self.deref()[index]
    }
}

/// Like Default, but for also for things that
/// don't have a reasonable default.
pub trait Dummy {
    fn dummy() -> Self;
}

impl <T: Default> Dummy for T {
    fn dummy() -> Self { Self::default() }
}
