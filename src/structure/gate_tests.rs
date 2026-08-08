#[cfg(all(test, feature = "arrange"))]
mod gate_completeness {
    //! The fast-path gate is a COMPLETE certifier: every polygon the gate
    //! accepts (FixOutcome::Fast) must survive the exit validator after
    //! OGC re-winding, because the Fast path now SKIPS that validator
    //! (2026-08-07). A gate/validator divergence here ships invalid output.

    use super::*;
    use crate::make_valid::{enforce_ogc_winding, is_valid_with_geo};
    use crate::validation::GeoValidation;
    use crate::{MakeValidConfig, PolyMethod};
    use geo::{Coord, LineString, Polygon};

    fn ring(coords: &[(f64, f64)]) -> LineString<f64> {
        let mut v: Vec<Coord<f64>> = coords
            .iter()
            .map(|&(x, y)| Coord { x, y })
            .collect();
        v.push(v[0]);
        LineString::new(v)
    }

    fn circle(n: usize, r: f64, ccw: bool) -> Polygon<f64> {
        let mut v = Vec::with_capacity(n + 1);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let (x, y) = (r * a.cos(), r * a.sin());
            v.push(if ccw { Coord { x, y } } else { Coord { x: -x, y } });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn star(n: usize) -> Polygon<f64> {
        let mut v = Vec::with_capacity(n + 1);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let r = if i % 3 == 0 { 100.0 } else { 50.0 };
            v.push(Coord { x: r * a.cos(), y: r * a.sin() });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn spaghetti(n: usize, seed: u64) -> Polygon<f64> {
        let mut s = seed;
        let mut next = move || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u32
        };
        let span = (n as f64).sqrt().ceil() as i64;
        let (mut x, mut y) = (0i64, 0i64);
        let mut v = Vec::with_capacity(n + 1);
        v.push(Coord { x: x as f64, y: y as f64 });
        for _ in 1..n {
            match next() % 4 {
                0 => x += 1,
                1 => x -= 1,
                2 => y += 1,
                _ => y -= 1,
            }
            x = x.rem_euclid(span);
            y = y.rem_euclid(span);
            v.push(Coord { x: x as f64, y: y as f64 });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn t_junction_ring() -> Polygon<f64> {
        // Closing vertex (110, 140) lies on edge (60, 90)-(160, 190):
        // the GEOS XML Test 22 class - vertex-on-edge self-touch.
        Polygon::new(
            ring(&[(60.0, 90.0), (160.0, 190.0), (260.0, 90.0), (110.0, 140.0)]),
            Vec::new(),
        )
    }

    fn eps_collinear_sliver() -> Polygon<f64> {
        // Two same-ring edges within 5e-14 of collinear overlap (the
        // 32*EPS*scale collinear gate class; geo_bridge catches it).
        Polygon::new(
            ring(&[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (5.0, 5e-14),
                (0.0, 10.0),
            ]),
            Vec::new(),
        )
    }

    fn duplicated_holes() -> Polygon<f64> {
        let h = ring(&[(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0)]);
        Polygon::new(
            ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
            vec![h.clone(), h],
        )
    }

    fn hole_equals_shell() -> Polygon<f64> {
        let h = ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]);
        Polygon::new(h.clone(), vec![h])
    }

    fn nested_holes() -> Polygon<f64> {
        Polygon::new(
            ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
            vec![
                ring(&[(2.0, 2.0), (2.0, 8.0), (8.0, 8.0), (8.0, 2.0)]),
                ring(&[(3.0, 3.0), (3.0, 5.0), (5.0, 5.0), (5.0, 3.0)]),
            ],
        )
    }

    fn hole_touching_shell() -> Polygon<f64> {
        // OGC-valid: a hole touching the shell at a single point.
        Polygon::new(
            ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
            vec![ring(&[(0.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)])],
        )
    }

    fn hole_edge_overlapping_shell() -> Polygon<f64> {
        // Hole edge collinear-overlapping the shell edge - invalid.
        Polygon::new(
            ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
            vec![ring(&[(0.0, 2.0), (5.0, 2.0), (5.0, 4.0), (0.0, 4.0)])],
        )
    }

    fn bowtie_subdivided(k: usize) -> Polygon<f64> {
        let edges = [
            ((0.0, 0.0), (10.0, 10.0)),
            ((10.0, 10.0), (10.0, 0.0)),
            ((10.0, 0.0), (0.0, 10.0)),
            ((0.0, 10.0), (0.0, 0.0)),
        ];
        let mut v = Vec::new();
        for ((ax, ay), (bx, by)) in edges {
            for i in 0..k {
                let t = i as f64 / k as f64;
                v.push(Coord { x: ax + (bx - ax) * t, y: ay + (by - ay) * t });
            }
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn mixed_magnitude() -> Polygon<f64> {
        // Big shell, micro ring inside: the poly-global eps would swamp
        // the micro ring's tolerance - the gate must use per-ring eps.
        Polygon::new(
            ring(&[(-5.0e6, -5.0e6), (-5.0e6, 5.0e6), (5.0e6, 5.0e6), (5.0e6, -5.0e6)]),
            vec![
                ring(&[(-1.0e-9, -1.0e-9), (-1.0e-9, 1.0e-9), (1.0e-9, 1.0e-9), (1.0e-9, -1.0e-9)]),
            ],
        )
    }

    #[test]
    fn fast_outcome_is_valid_after_rewind() {
        let mut cases: Vec<Polygon<f64>> = Vec::new();
        for n in [4usize, 32, 100, 1000, 5000] {
            cases.push(circle(n, 100.0, true));
            cases.push(circle(n, 100.0, false)); // CW - winding-only
            cases.push(star(n));
        }
        for n in [100usize, 500, 2000] {
            cases.push(spaghetti(n, 7));
            cases.push(spaghetti(n, 0x9E3779B97F4A7C15));
        }
        cases.push(t_junction_ring());
        cases.push(eps_collinear_sliver());
        cases.push(duplicated_holes());
        cases.push(hole_equals_shell());
        cases.push(nested_holes());
        cases.push(hole_touching_shell());
        cases.push(hole_edge_overlapping_shell());
        cases.push(bowtie_subdivided(4));
        cases.push(bowtie_subdivided(125));
        cases.push(mixed_magnitude());
        // Extreme-magnitude closed triangle: exact orient ~1e-631, the
        // robust predicate's collinear fallback sign is garbage - the
        // Fast path must route it to arrange (fuzz
        // invariant_mixed_fp_in_same_ring perm=1, keep_collapsed=true).
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord { x: f64::MAX, y: f64::MIN },
                Coord { x: f64::MIN_POSITIVE, y: -f64::MIN_POSITIVE },
                Coord { x: 0.0, y: 0.0 },
                Coord { x: f64::MAX, y: f64::MIN },
            ]),
            Vec::new(),
        ));
        // Micro-magnitude sliver: the validator rejects envelopes thinner
        // than f64::EPSILON (DegenerateExterior) even though the ring is
        // valid at its own scale - the gate must route it to repair
        // (fuzz_inprocess_loop, 2026-08-07).
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord { x: 0.0, y: 5.411379623889514e-305 },
                Coord { x: 2.0, y: 4.779275733397475e-58 },
                Coord { x: 5.0, y: 5.117669874566563e-307 },
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 0.0, y: 5.411379623889514e-305 },
            ]),
            Vec::new(),
        ));
        // Near-parallel mixed-magnitude pair (the adaptive error bound
        // class): a long edge + a short edge separated by ~1e-8 - the
        // L2-based collinear margin flagged it, the product-sum bound
        // does not (fuzz_inprocess_loop, 2026-08-07).
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord { x: 5e-9, y: 9e-9 },
                Coord { x: -736.3, y: 678.1 },
                Coord { x: 16172637.6, y: 6.773626999899899e-264 },
                Coord { x: 9e-9, y: -9e-9 },
                Coord { x: 219.2, y: 1.1749363827356277e-161 },
                Coord { x: 5e-9, y: 8.869032762944698e-9 },
                Coord { x: 5e-9, y: 9e-9 },
            ]),
            Vec::new(),
        ));

        for (i, poly) in cases.iter().enumerate() {
            let out = crate::structure::fix_polygon_owned(poly.clone(), &MakeValidConfig::default(), None);
            match out {
                crate::structure::FixOutcome::Fast(g) => {
                    let (g, ok) = enforce_ogc_winding(g);
                    // The production contract: Fast + orientation-ok must be
                    // validator-clean (the arms check orientation AFTER
                    // re-winding; rings in the exact-orient ~0 zone route to
                    // arrange there - e.g. case 31, the extreme triangle).
                    if ok {
                        assert!(
                            is_valid_with_geo(&g),
                            "case {i}: Fast shipped a polygon the validator rejects"
                        );
                    }
                }
                crate::structure::FixOutcome::Repaired(g) => {
                    assert!(
                        g.is_valid(),
                        "case {i}: Repaired shipped an invalid polygon"
                    );
                }
                crate::structure::FixOutcome::Unconsumed(_) => {}
            }
        }
    }

    #[test]
    fn gate_rejects_negative_zero_pinch() {
        // A (-0.0, y) / (+0.0, y) non-adjacent vertex pair is a pinch: the
        // validator's key normalization (c.x + 0.0).to_bits() treats the
        // two zeros as the same vertex. The gate's duplicate scan must
        // agree or the Fast path ships a polygon the exit validator
        // rejects (found by audit 2026-08-08; the fuzz never generated
        // the sign-flipped pair).
        let mut coords = vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 10.0, y: 0.0 },
            Coord { x: -0.0, y: 0.0 },
            Coord { x: 10.0, y: 10.0 },
            Coord { x: 0.0, y: 10.0 },
        ];
        coords.push(coords[0]);
        let poly = Polygon::new(LineString::new(coords), Vec::new());
        assert!(
            !crate::arrange::poly_has_basic_form(&poly),
            "gate accepted a -0.0/+0.0 pinch the validator rejects"
        );
        let v = poly.validate();
        assert!(
            v.errors.iter().any(|e| matches!(
                e,
                crate::validation::GeometryValidationError::PinchPoint
            )),
            "validator did not flag the -0.0 pinch: {:?}",
            v.errors
        );
    }
}
