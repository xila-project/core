use super::write_output;
use core::sync::atomic::{AtomicU8, Ordering};

pub(super) static INITIALIZED: AtomicU8 = AtomicU8::new(0);

#[used]
#[unsafe(link_section = ".preinit_array")]
static PREINITIALIZER: extern "C" fn() = preinitialize;
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZER: extern "C" fn() = initialize;
#[used]
#[unsafe(link_section = ".fini_array")]
static FINALIZER: extern "C" fn() = finalize;

extern "C" fn preinitialize() {
    if write_output(b"Rust preinit\n") {
        INITIALIZED.store(1, Ordering::Relaxed);
    }
}

extern "C" fn initialize() {
    if INITIALIZED.load(Ordering::Relaxed) == 1 && write_output(b"Rust init\n") {
        INITIALIZED.store(2, Ordering::Relaxed);
    }
}

extern "C" fn finalize() {
    write_output(b"Rust fini\n");
}
