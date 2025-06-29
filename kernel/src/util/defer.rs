use core::sync::atomic::{AtomicBool, Ordering};

pub struct DeferHandle(AtomicBool);

macro_rules! defer_handle {
    ($b:block) => {{
        scopeguard::guard($crate::util::defer::DeferHandle::new(), |h| {
            if !h.is_canceled() {
                $b
            }
        })
    }};
}
pub(crate) use defer_handle;

impl DeferHandle {
    pub fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
