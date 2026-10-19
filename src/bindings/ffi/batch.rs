//! parallel batch API (extracted from bindings/ffi.rs 2026-08-07; verbatim).

use super::types::*;
use super::util::*;
use super::wkb::*;
use crate::make_valid::MakeValid;
use alloc::vec::Vec;
use geo::Geometry;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Free the returned GeoRepairBatchResult with super::geo_repair_free_batch_result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn geo_repair_make_valid_batch(
    inputs: *const GeoRepairWkbBuffer,
    count: usize,
    parallel: i32,
) -> GeoRepairBatchResult {
    if count == 0 {
        return GeoRepairBatchResult::success(Vec::new());
    }
    if inputs.is_null() {
        return GeoRepairBatchResult::error(
            GeoRepairErrorCode::InvalidInput,
            "inputs must not be null when count > 0",
        );
    }
    match catch_unwind(AssertUnwindSafe(|| {
        let slice = unsafe { std::slice::from_raw_parts(inputs, count) };
        let config = make_config(false, 0, 0, 0);
        let items = if parallel != 0 {
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                let parsed: Vec<Result<Geometry<f64>, GeoRepairErrorCode>> = slice
                    .iter()
                    .map(|buf| geometry_from_wkb(buf.data, buf.len))
                    .collect();
                let repaired: Vec<Result<Vec<u8>, (GeoRepairErrorCode, &'static str)>> = parsed
                    .into_par_iter()
                    .map(|geom| match geom {
                        Ok(g) => {
                            let fixed = g.make_valid_with_config(&config);
                            geometry_to_wkb(&fixed).map_err(|code| (code, "WKB write error"))
                        }
                        Err(code) => Err((code, "WKB parse error")),
                    })
                    .collect();
                repaired
                    .into_iter()
                    .map(|r| match r {
                        Ok(wkb) => GeoRepairResult::success(wkb),
                        Err((code, msg)) => GeoRepairResult::error(code, msg),
                    })
                    .collect()
            }
            #[cfg(not(feature = "parallel"))]
            {
                let _ = &config;
                repair_batch_sequential(slice, &make_config(false, 0, 0, 0))
            }
        } else {
            repair_batch_sequential(slice, &config)
        };
        GeoRepairBatchResult::success(items)
    })) {
        Ok(r) => r,
        Err(_) => GeoRepairBatchResult::error(
            GeoRepairErrorCode::Panic,
            "internal error: batch repair panicked",
        ),
    }
}

