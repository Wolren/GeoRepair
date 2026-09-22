//! Shared support for the fuzz targets.
//!
//! libfuzzer-sys's `initialize` (called from the `fuzz_target!` macro's
//! `LLVMFuzzerInitialize`) installs a panic hook that runs the default hook
//! and then `std::process::abort()` BEFORE any unwinding happens
//! (libfuzzer-sys 0.4.x `src/lib.rs`). That defeats every `catch_unwind`
//! in the targets and in the library's containment guards: a repair panic
//! the library catches would still kill the process as a deadly signal,
//! which is not the contract documented in `fuzz/Cargo.toml` (the profile
//! is `panic = "unwind"` precisely so containment can run). Measured:
//! fuzz-nightly run 35499180884 (2026-09-20) crashed on an i_overlay
//! `is_fill_top` assertion that `structure::merge::merge_shells` caught
//! correctly in a normal `cargo test` replay; under the aborting hook the
//! process died at the panic itself.
//!
//! Each target calls [`install_unwinding_panic_hook`] as its first action,
//! re-installing a hook that prints the panic (with a backtrace, since the
//! stack is unwound before libFuzzer's own abort at the FFI boundary) and
//! then lets unwinding proceed:
//!
//!   * panics the target itself raises (contract assert failures) unwind
//!     out of the target body, are caught by libfuzzer-sys's
//!     `test_input_wrap`, and abort there: still a saved crash artifact;
//!   * panics the library contains (its own `catch_unwind` guards) unwind
//!     into the guard and become fallback results: no crash, which is the
//!     containment contract;
//!   * panics that escape the library unwind into the target's
//!     `catch_unwind`, the target re-panics with its message, and
//!     `test_input_wrap` aborts: still a crash.
use std::sync::Once;

static INSTALL: Once = Once::new();

/// Install the unwinding panic hook exactly once per process. Cheap after
/// the first call (`Once` fast path), safe to call at the top of every
/// target body.
pub(crate) fn install_unwinding_panic_hook() {
    INSTALL.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "Box<dyn Any>".to_string());
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("<unnamed>");
            let loc = info
                .location()
                .map(|l| format!("{}:{}:", l.file(), l.line()))
                .unwrap_or_default();
            eprintln!("thread '{name}' panicked at {loc}: {msg}");
            eprintln!(
                "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace"
            );
            // Captured before unwinding: libFuzzer aborts at its FFI
            // boundary after the stack is gone, so the useful frames must
            // be printed here.
            let bt = std::backtrace::Backtrace::force_capture();
            eprintln!("{bt}");
        }));
    });
}
