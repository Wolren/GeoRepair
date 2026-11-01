//! Batch repair entries that build with or without the `parallel` feature.
//!
//! Each function keeps input order and returns one geometry per input polygon. With
//! `parallel` enabled the work goes through `crate::parallel`; otherwise a serial
//! loop runs the same call per polygon, so a host that forbids threads compiles the
//! same call site.
//!
//! ```rust
//! # use geo::{LineString, Polygon};
//! # use geo_repair::{MakeValidConfig, batch::fix_polygon_batch};
//! let bowtie = Polygon::new(
//!     LineString::from(vec![(0.0, 0.0), (10.0, 10.0), (0.0, 10.0), (10.0, 0.0), (0.0, 0.0)]),
//!     vec![],
//! );
//! let out = fix_polygon_batch(&[&bowtie], &MakeValidConfig::default());
//! assert_eq!(out.len(), 1);
//! ```

use alloc::vec::Vec;
use geo::{Geometry, Polygon};

use crate::core::MakeValidConfig;
#[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
use crate::make_valid::MakeValid;

/// Repair a slice of borrowed polygons, one result per input, in input order.
#[cfg(any(feature = "arrange", feature = "structure"))]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn fix_polygon_batch(polys: &[&Polygon<f64>], config: &MakeValidConfig) -> Vec<Geometry<f64>> {
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        crate::parallel::par_fix_polygon_batch(polys, config)
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let mut out: Vec<Geometry<f64>> = Vec::with_capacity(polys.len());
        for p in polys {
            out.push((*p).make_valid_with_config(config));
        }
        out
    }
}

/// Repair owned polygons without cloning rings that are already valid.
#[cfg(any(feature = "arrange", feature = "structure"))]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn fix_polygon_batch_owned(
    polys: Vec<Polygon<f64>>,
    config: &MakeValidConfig,
) -> Vec<Geometry<f64>> {
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        crate::parallel::par_fix_polygon_batch_owned(polys, config)
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let mut out: Vec<Geometry<f64>> = Vec::with_capacity(polys.len());
        for p in polys {
            out.push(crate::make_valid::make_valid_owned(p, config));
        }
        out
    }
}

/// Repair polygons from an iterator in chunks of `chunk_size`, keeping input order.
///
/// `chunk_size` below 1 is clamped to 1. The serial path ignores the chunk size.
#[cfg(any(feature = "arrange", feature = "structure"))]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn fix_polygon_batch_chunked<I>(
    iter: I,
    chunk_size: usize,
    config: &MakeValidConfig,
) -> Vec<Geometry<f64>>
where
    I: Iterator<Item = Polygon<f64>>,
{
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        crate::parallel::par_fix_polygon_batch_chunked(iter, chunk_size, config)
    }
    #[cfg(not(all(feature = "parallel", not(target_arch = "wasm32"))))]
    {
        let _ = chunk_size;
        iter.map(|p| crate::make_valid::make_valid_owned(p, config))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_valid::MakeValid;
    use geo::LineString;

    fn bowtie() -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (10.0, 0.0),
                (0.0, 0.0),
            ]),
            vec![],
        )
    }

    fn square() -> Polygon<f64> {
        Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ]),
            vec![],
        )
    }

    #[test]
    fn borrowed_batch_matches_single_calls() {
        let a = square();
        let b = bowtie();
        let refs = [&a, &b];
        let batch = fix_polygon_batch(&refs, &MakeValidConfig::default());
        assert_eq!(batch.len(), 2);
        assert_eq!(
            batch[0],
            a.make_valid_with_config(&MakeValidConfig::default())
        );
        assert_eq!(
            batch[1],
            b.make_valid_with_config(&MakeValidConfig::default())
        );
    }

    #[test]
    fn owned_batch_matches_single_calls() {
        let cfg = MakeValidConfig::default();
        let expected: Vec<Geometry<f64>> = [square(), bowtie()]
            .iter()
            .map(|p| p.make_valid_with_config(&cfg))
            .collect();
        let out = fix_polygon_batch_owned(vec![square(), bowtie()], &cfg);
        assert_eq!(out, expected);
    }

    #[test]
    fn chunked_batch_keeps_order_and_handles_remainder() {
        let cfg = MakeValidConfig::default();
        let input = [square(), bowtie(), square()];
        let expected: Vec<Geometry<f64>> = input
            .iter()
            .map(|p| p.make_valid_with_config(&cfg))
            .collect();
        let out = fix_polygon_batch_chunked(input.into_iter(), 2, &cfg);
        assert_eq!(out, expected);
    }

    #[test]
    fn empty_batch_is_empty() {
        let out = fix_polygon_batch(&[], &MakeValidConfig::default());
        assert!(out.is_empty());
        let out = fix_polygon_batch_owned(Vec::new(), &MakeValidConfig::default());
        assert!(out.is_empty());
    }
}
