//! cumulative per-stage timing counters and the profile printer
//!
//! Extracted from structure/mod.rs on 2026-08-07 (file-size governance).
//! Content is verbatim - no behavior changes; items are re-exported by
//! structure/mod.rs so `crate::structure::X` paths keep resolving.

use ::core::sync::atomic::{AtomicU64, Ordering};
pub static PROFILE_FP_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_SR_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_HR_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_HN_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_MG_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_FSI_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_CL_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_NEST_NS: AtomicU64 = AtomicU64::new(0);
pub static PROFILE_SUB_NS: AtomicU64 = AtomicU64::new(0);

pub fn reset_profile() {
    PROFILE_FP_NS.store(0, Ordering::Relaxed);
    PROFILE_SR_NS.store(0, Ordering::Relaxed);
    PROFILE_HR_NS.store(0, Ordering::Relaxed);
    PROFILE_HN_NS.store(0, Ordering::Relaxed);
    PROFILE_MG_NS.store(0, Ordering::Relaxed);
    PROFILE_FSI_NS.store(0, Ordering::Relaxed);
    PROFILE_CL_NS.store(0, Ordering::Relaxed);
    PROFILE_NEST_NS.store(0, Ordering::Relaxed);
    PROFILE_SUB_NS.store(0, Ordering::Relaxed);
}

#[cfg(feature = "std")]
pub fn print_profile(n_polys: usize) {
    let fp = PROFILE_FP_NS.load(Ordering::Relaxed);
    let sr = PROFILE_SR_NS.load(Ordering::Relaxed);
    let hr = PROFILE_HR_NS.load(Ordering::Relaxed);
    let hn = PROFILE_HN_NS.load(Ordering::Relaxed);
    let mg = PROFILE_MG_NS.load(Ordering::Relaxed);
    let fsi = PROFILE_FSI_NS.load(Ordering::Relaxed);
    let cl = PROFILE_CL_NS.load(Ordering::Relaxed);
    let nest = PROFILE_NEST_NS.load(Ordering::Relaxed);
    let sub = PROFILE_SUB_NS.load(Ordering::Relaxed);
    let total_ns = fp + sr + hr + hn + mg;
    let total_ms = total_ns as f64 / 1e6;
    let pct = |v: f64| {
        if total_ms > 0.0 {
            v / total_ms * 100.0
        } else {
            0.0
        }
    };
    let ms = |v: u64| v as f64 / 1e6;
    eprintln!("\n=== Structure profile: {n_polys} polys ===");
    eprintln!("  fast_path     {:>9.3}ms  {:>5.1}%", ms(fp), pct(ms(fp)));
    eprintln!("  shell_repair  {:>9.3}ms  {:>5.1}%", ms(sr), pct(ms(sr)));
    eprintln!("    (self_intx) {:>9.3}ms", ms(fsi));
    eprintln!("  hole_repair   {:>9.3}ms  {:>5.1}%", ms(hr), pct(ms(hr)));
    eprintln!(
        "  hole_nest_sub {:>9.3}ms  {:>5.1}%  break:",
        ms(hn),
        pct(ms(hn))
    );
    eprintln!("    classify    {:>9.3}ms  {:>5.1}%", ms(cl), pct(ms(cl)));
    eprintln!(
        "    nesting     {:>9.3}ms  {:>5.1}%",
        ms(nest),
        pct(ms(nest))
    );
    eprintln!("    subtract    {:>9.3}ms  {:>5.1}%", ms(sub), pct(ms(sub)));
    eprintln!("  merge         {:>9.3}ms  {:>5.1}%", ms(mg), pct(ms(mg)));
    eprintln!("  ─────────────────────────────────");
    eprintln!("  total         {:>9.3}ms", ms(total_ns));
}
