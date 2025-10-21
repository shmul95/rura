use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_IO: AtomicBool = AtomicBool::new(false);

pub fn set_debug_io(enabled: bool) {
    DEBUG_IO.store(enabled, Ordering::Relaxed);
}

pub fn debug_io_enabled() -> bool {
    DEBUG_IO.load(Ordering::Relaxed)
}
