#[cfg(all(test, feature = "arrange"))]
mod gate_completeness {
    //! The fast-path gate is a complete certifier: every polygon the gate accepts
    //! (FixOutcome::Fast) must survive the exit validator after OGC re-winding, because
    //! the Fast path now skips that validator (2026-08-07).

    use crate::MakeValidConfig;
    use crate::make_valid::{enforce_ogc_winding, is_valid_with_geo};
    use crate::validation::GeoValidation;
    use geo::{Coord, LineString, Polygon};

    fn ring(coords: &[(f64, f64)]) -> LineString<f64> {
        let mut v: Vec<Coord<f64>> = coords.iter().map(|&(x, y)| Coord { x, y }).collect();
        v.push(v[0]);
        LineString::new(v)
    }

    fn circle(n: usize, r: f64, ccw: bool) -> Polygon<f64> {
        let mut v = Vec::with_capacity(n + 1);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let (x, y) = (r * a.cos(), r * a.sin());
            v.push(if ccw {
                Coord { x, y }
            } else {
                Coord { x: -x, y }
            });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn star(n: usize) -> Polygon<f64> {
        let mut v = Vec::with_capacity(n + 1);
        for i in 0..n {
            let a = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
            let r = if i % 3 == 0 { 100.0 } else { 50.0 };
            v.push(Coord {
                x: r * a.cos(),
                y: r * a.sin(),
            });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn spaghetti(n: usize, seed: u64) -> Polygon<f64> {
        let mut s = seed;
        let mut next = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as u32
        };
        let span = (n as f64).sqrt().ceil() as i64;
        let (mut x, mut y) = (0i64, 0i64);
        let mut v = Vec::with_capacity(n + 1);
        v.push(Coord {
            x: x as f64,
            y: y as f64,
        });
        for _ in 1..n {
            match next() % 4 {
                0 => x += 1,
                1 => x -= 1,
                2 => y += 1,
                _ => y -= 1,
            }
            x = x.rem_euclid(span);
            y = y.rem_euclid(span);
            v.push(Coord {
                x: x as f64,
                y: y as f64,
            });
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn t_junction_ring() -> Polygon<f64> {
        Polygon::new(
            ring(&[(60.0, 90.0), (160.0, 190.0), (260.0, 90.0), (110.0, 140.0)]),
            Vec::new(),
        )
    }

    fn eps_collinear_sliver() -> Polygon<f64> {
        // Two same-ring edges within 5e-14 of collinear overlap (the 32*eps*scale
        // collinear gate class; geo_bridge catches it).
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
        Polygon::new(
            ring(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)]),
            vec![ring(&[(0.0, 2.0), (2.0, 2.0), (2.0, 4.0), (0.0, 4.0)])],
        )
    }

    fn hole_edge_overlapping_shell() -> Polygon<f64> {
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
                v.push(Coord {
                    x: ax + (bx - ax) * t,
                    y: ay + (by - ay) * t,
                });
            }
        }
        v.push(v[0]);
        Polygon::new(LineString::new(v), Vec::new())
    }

    fn mixed_magnitude() -> Polygon<f64> {
        Polygon::new(
            ring(&[
                (-5.0e6, -5.0e6),
                (-5.0e6, 5.0e6),
                (5.0e6, 5.0e6),
                (5.0e6, -5.0e6),
            ]),
            vec![ring(&[
                (-1.0e-9, -1.0e-9),
                (-1.0e-9, 1.0e-9),
                (1.0e-9, 1.0e-9),
                (1.0e-9, -1.0e-9),
            ])],
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
        // Fast path must route it to arrange (fuzz invariant_mixed_fp_in_same_ring
        // perm=1, keep_collapsed=true).
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord {
                    x: f64::MAX,
                    y: f64::MIN,
                },
                Coord {
                    x: f64::MIN_POSITIVE,
                    y: -f64::MIN_POSITIVE,
                },
                Coord { x: 0.0, y: 0.0 },
                Coord {
                    x: f64::MAX,
                    y: f64::MIN,
                },
            ]),
            Vec::new(),
        ));
        // than f64::epsilon (DegenerateExterior) even though the ring
        // (fuzz_inprocess_loop, 2026-08-07).
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord {
                    x: 0.0,
                    y: 5.411379623889514e-305,
                },
                Coord {
                    x: 2.0,
                    y: 4.779275733397475e-58,
                },
                Coord {
                    x: 5.0,
                    y: 5.117669874566563e-307,
                },
                Coord { x: 0.0, y: 0.0 },
                Coord {
                    x: 0.0,
                    y: 5.411379623889514e-305,
                },
            ]),
            Vec::new(),
        ));
        // Near-parallel mixed-magnitude pair (the adaptive error bound class): a long
        // edge + a short edge separated by ~1e-8
        cases.push(Polygon::new(
            LineString::new(vec![
                Coord { x: 5e-9, y: 9e-9 },
                Coord {
                    x: -736.3,
                    y: 678.1,
                },
                Coord {
                    x: 16172637.6,
                    y: 6.773626999899899e-264,
                },
                Coord { x: 9e-9, y: -9e-9 },
                Coord {
                    x: 219.2,
                    y: 1.1749363827356277e-161,
                },
                Coord {
                    x: 5e-9,
                    y: 8.869032762944698e-9,
                },
                Coord { x: 5e-9, y: 9e-9 },
            ]),
            Vec::new(),
        ));

        for (i, poly) in cases.iter().enumerate() {
            let out = crate::structure::fix_polygon_owned(
                poly.clone(),
                &MakeValidConfig::default(),
                None,
            );
            match out {
                crate::structure::FixOutcome::Fast(g, _, _, _) => {
                    let (g, ok) = enforce_ogc_winding(g);
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
        // A (-0.0, y) / (+0.0, y) non-adjacent vertex pair is a pinch
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
            v.errors
                .iter()
                .any(|e| matches!(e, crate::validation::GeometryValidationError::PinchPoint)),
            "validator did not flag the -0.0 pinch: {:?}",
            v.errors
        );
    }

    /// A regular n-gon, ccw, no holes.
    fn ngon(n: usize, r: f64, cx: f64, cy: f64) -> Vec<Coord<f64>> {
        let mut v: Vec<Coord<f64>> = (0..n - 1)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / (n - 1) as f64;
                Coord {
                    x: cx + r * a.cos(),
                    y: cy + r * a.sin(),
                }
            })
            .collect();
        v.push(v[0]);
        v
    }

    /// Non-adjacent repeated vertex on a ring larger than `SMALL_RING_LINES`
    /// (2026-10-17).
    #[test]
    fn gate_rejects_repeated_vertex_on_chain_path() {
        let clean = Polygon::new(LineString::new(ngon(40, 100.0, 500.0, 500.0)), Vec::new());

        let mut coords = ngon(40, 100.0, 500.0, 500.0);
        coords[21] = coords[1];
        let pinched = Polygon::new(LineString::new(coords), Vec::new());

        match crate::structure::fix_polygon_owned(clean.clone(), &MakeValidConfig::default(), None)
        {
            crate::structure::FixOutcome::Fast(g, _, _, _) => {
                let (g, ok) = enforce_ogc_winding(g);
                assert!(ok, "clean 40-gon failed orientation certification");
                assert!(is_valid_with_geo(&g), "clean 40-gon shipped invalid");
            }
            other => panic!(
                "clean 40-gon did not take the fast path: {}",
                match other {
                    crate::structure::FixOutcome::Repaired(_) => "Repaired",
                    crate::structure::FixOutcome::Unconsumed(_) => "Unconsumed",
                    crate::structure::FixOutcome::Fast(..) => "Fast",
                }
            ),
        }

        assert!(
            !pinched.validate().valid,
            "validator accepted a non-adjacent repeated vertex"
        );

        match crate::structure::fix_polygon_owned(
            pinched.clone(),
            &MakeValidConfig::default(),
            None,
        ) {
            crate::structure::FixOutcome::Fast(g, _, _, _) => {
                let (g, ok) = enforce_ogc_winding(g);
                assert!(
                    !ok || is_valid_with_geo(&g),
                    "Fast shipped a repeated-vertex ring the validator rejects"
                );
            }
            crate::structure::FixOutcome::Repaired(g) => {
                assert!(g.is_valid(), "Repaired shipped an invalid polygon");
            }
            crate::structure::FixOutcome::Unconsumed(_) => {}
        }
    }
}

#[cfg(all(test, feature = "arrange"))]
mod gate_retrace_ring {
    //! fuzz-nightly class C (run 35578735657, 2026-09-21): the fast-path gate certified
    //! a ring whose closing edge doubles back over part of the first edge; the OGC
    //! re-winding after the gate flipped the segments and the exit validator then
    //! rejected the shipped output with SelfIntersection on geometry the gate had
    //! called clean.

    use crate::MakeValidConfig;
    use geo::{Coord, LineString, Polygon};

    const RETRACE_RING: [[f64; 2]; 5] = [
        [0.0, -5.500000000232603],
        [0.0, 589338232487936.0],
        [999901360038144.0, 1.5],
        [0.0, 5.0],
        [0.0, -5.500000000232603],
    ];

    #[test]
    fn gate_rejects_retrace_ring() {
        let mut v: Vec<Coord<f64>> = RETRACE_RING.iter().map(|&[x, y]| Coord { x, y }).collect();
        if v.first() != v.last() {
            v.push(v[0]);
        }
        let poly = Polygon::new(LineString::new(v), Vec::new());
        let outcome = crate::structure::fix_polygon_owned(poly, &MakeValidConfig::default(), None);
        assert!(
            !matches!(outcome, crate::structure::FixOutcome::Fast(..)),
            "the gate must not certify a retrace ring"
        );
    }
}

/// `ring_guards` is the gate's whole-ring guard reduction (sub-ULP edge flag plus
/// smallest non-zero |ordinate|), and `has_sub_ulp_edge` is the pre-collapse pass's
mod ring_guards_agreement {
    use crate::arrange::{has_sub_ulp_edge, ring_guards};
    use geo::{Coord, LineString, Polygon};

    fn closed(mut v: Vec<Coord<f64>>) -> Vec<Coord<f64>> {
        let f = v[0];
        v.push(f);
        v
    }

    fn as_poly(ring: &[Coord<f64>]) -> Polygon<f64> {
        Polygon::new(LineString::new(ring.to_vec()), Vec::new())
    }

    fn naive_sub_ulp(ring: &[Coord<f64>]) -> bool {
        for w in ring.windows(2) {
            let eps = f64::EPSILON
                * w[0]
                    .x
                    .abs()
                    .max(w[0].y.abs())
                    .max(w[1].x.abs())
                    .max(w[1].y.abs());
            if eps > 0.0 && (w[1].x - w[0].x).abs().max((w[1].y - w[0].y).abs()) < eps {
                return true;
            }
        }
        false
    }

    /// Mirrors the in-walk `min_abs` this reduction replaced: the minimum over non-zero
    /// |x| and |y| of `windows(2)`'s leading coordinate.
    fn naive_min_abs(ring: &[Coord<f64>]) -> f64 {
        let mut m = f64::INFINITY;
        for w in ring.windows(2) {
            for v in [w[0].x.abs(), w[0].y.abs()] {
                if v != 0.0 {
                    m = m.min(v);
                }
            }
        }
        m
    }

    #[test]
    fn matches_the_naive_rule_and_the_precollapse_pass() {
        let cases: Vec<Vec<Coord<f64>>> = vec![
            closed(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
            ]),
            // Same square at 1e12 magnitude: still a valid 4-unit ring.
            closed(vec![
                Coord { x: 1e12, y: 1e12 },
                Coord {
                    x: 1e12 + 4.0,
                    y: 1e12,
                },
                Coord {
                    x: 1e12 + 4.0,
                    y: 1e12 + 4.0,
                },
                Coord {
                    x: 1e12,
                    y: 1e12 + 4.0,
                },
            ]),
            // 1e-8 spike at 1e8 magnitude: the noise class the guard exists for (1e-8 <
            // epsilon * 1e8).
            closed(vec![
                Coord { x: 1e8, y: 1e8 },
                Coord {
                    x: 1e8 + 1.0,
                    y: 1e8,
                },
                Coord {
                    x: 1e8 + 1.0,
                    y: 1e8 + 1e-8,
                },
                Coord {
                    x: 1e8,
                    y: 1e8 + 1.0,
                },
            ]),
            closed(vec![
                Coord { x: 0.0, y: 0.0 },
                Coord { x: 1.0, y: 0.0 },
                Coord { x: 1.0, y: 1.0 },
                Coord { x: 0.0, y: 1.0 },
                Coord {
                    x: 4.9e208,
                    y: 4.9e208,
                },
            ]),
            // Signed zeros must read as zero (skipped), not as a 0.0 minimum.
            closed(vec![
                Coord { x: -0.0, y: -0.0 },
                Coord { x: 2.0, y: 0.0 },
                Coord { x: 2.0, y: 3.0 },
                Coord { x: 0.0, y: 3.0 },
            ]),
        ];

        for (i, ring) in cases.iter().enumerate() {
            let (sub, min_abs) = ring_guards(ring);
            assert_eq!(
                sub,
                naive_sub_ulp(ring),
                "case {i}: ring_guards sub-ULP verdict disagrees with the naive rule"
            );
            assert_eq!(
                sub,
                has_sub_ulp_edge(&as_poly(ring), None),
                "case {i}: the pre-collapse pass disagrees with ring_guards"
            );
            let want = naive_min_abs(ring);
            assert!(
                min_abs == want || (min_abs.is_infinite() && want.is_infinite()),
                "case {i}: min_abs {min_abs} != {want}"
            );
        }
    }
}
