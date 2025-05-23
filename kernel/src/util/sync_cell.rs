use spin::RwLock;

/// Like Cell<T> but can be shared between threads more easily
#[derive(Debug)]
pub struct SynCell<T> {
    inner: RwLock<T>,
}

impl<T> SynCell<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: RwLock::new(value),
        }
    }

    pub fn set(&self, value: T) {
        *self.inner.write() = value;
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut inner = self.inner.write();
        f(&mut inner)
    }
}

impl<T: Clone> SynCell<T> {
    pub fn cloned(&self) -> T {
        self.inner.read().clone()
    }
}

impl<T: Copy> SynCell<T> {
    pub fn get(&self) -> T {
        *self.inner.read()
    }

    /// Replace the inner value using the provided function. Returns the
    /// previous value.
    pub fn replace(&self, f: impl FnOnce(&T) -> T) -> T {
        let mut inner = self.inner.write();

        let prev = *inner;
        *inner = f(&inner);
        prev
    }
}
