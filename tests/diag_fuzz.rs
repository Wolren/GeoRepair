use geo::{Coord, Geometry, Line, LineString, Polygon};
use geo_repair::{MakeValid, MakeValidConfig, PolyMethod};
use geo::validation::Validation;

#[test]
fn diag_structure_pipeline() {
    let ring = vec![
        Coord { x: 33.298685125309, y: 25.64285228568552 },
        Coord { x: 16.056374168398353, y: 41.82073196346561 },
        Coord { x: 5.2001056860635515, y: -1.4935771193319936 },
        Coord { x: 40.0953181621632, y: 49.30127327981244 },
        Coord { x: -30.63143192804603, y: 22.339142189433932 },
        Coord { x: 17.726542485814562, y: -29.738377616718996 },
        Coord { x: 33.298685125309, y: 25.64285228568552 },  // closure
    ];
    let poly = Polygon::new(LineString::new(ring.clone()), Vec::new());
    
    // Step 1: edges
    let edges: Vec<Line<f64>> = ring.windows(2).map(|w| Line::new(w[0], w[1])).collect();
    println!("=== Edges ({}) ===", edges.len());
    for (i, e) in edges.iter().enumerate() {
        println!("  Edge {}: ({:.10}, {:.10}) -> ({:.10}, {:.10})", i, e.start.x, e.start.y, e.end.x, e.end.y);
    }
    
    // Check all non-adjacent edge pairs for intersection
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for e in &edges {
        min_x = min_x.min(e.start.x).min(e.end.x);
        max_x = max_x.max(e.start.x).max(e.end.x);
        min_y = min_y.min(e.start.y).min(e.end.y);
        max_y = max_y.max(e.start.y).max(e.end.y);
    }
    let coord_scale = (max_x - min_x).abs().max((max_y - min_y).abs()).max(1.0);
    let eps = 1e-12 * coord_scale;
    
    println!("\n=== Edge intersections (eps={:e}) ===", eps);
    let n = edges.len();
    for i in 0..n {
        for j in (i+2)..n {
            if i + 1 == j && edges[i].end == edges[j].start { continue; }
            if i == 0 && j == n - 1 && edges[i].start == edges[j].end { continue; }
            
            let denom = (edges[i].end.x - edges[i].start.x) * (edges[j].end.y - edges[j].start.y)
                - (edges[i].end.y - edges[i].start.y) * (edges[j].end.x - edges[j].start.x);
            if denom.abs() < eps { continue; }
            
            let ti = ((edges[j].start.x - edges[i].start.x) * (edges[j].end.y - edges[j].start.y)
                - (edges[j].start.y - edges[i].start.y) * (edges[j].end.x - edges[j].start.x)) / denom;
            let tj = ((edges[j].start.x - edges[i].start.x) * (edges[i].end.y - edges[i].start.y)
                - (edges[j].start.y - edges[i].start.y) * (edges[i].end.x - edges[i].start.x)) / denom;
            
            if ti >= -eps && ti <= 1.0 + eps && tj >= -eps && tj <= 1.0 + eps {
                let pi = Coord { x: edges[i].start.x + ti * (edges[i].end.x - edges[i].start.x), y: edges[i].start.y + ti * (edges[i].end.y - edges[i].start.y) };
                let pj = Coord { x: edges[j].start.x + tj * (edges[j].end.x - edges[j].start.x), y: edges[j].start.y + tj * (edges[j].end.y - edges[j].start.y) };
                
                let on_i = (ti > eps && ti < 1.0 - eps);
                let on_j = (tj > eps && tj < 1.0 - eps);
                println!("  E{} x E{}: ti={:.10} tj={:.10} pi=({:.10},{:.10}) pj=({:.10},{:.10}) on_i={} on_j={}", 
                    i, j, ti, tj, pi.x, pi.y, pj.x, pj.y, on_i, on_j);
            }
        }
    }
    
    // Now check the result structure produces
    println!();
    let config = MakeValidConfig { poly_method: PolyMethod::Structure, ..Default::default() };
    let result = poly.make_valid_with_config(&config);
    println!("Structure output valid: {:?}", result.check_validation());
    match &result {
        Geometry::Polygon(p) => {
            println!("  Polygon exterior ({} verts):", p.exterior().0.len());
            for (i, c) in p.exterior().0.iter().enumerate() {
                println!("    {}: ({:.10}, {:.10})", i, c.x, c.y);
            }
        }
        _ => println!("  {:?}", result),
    }
}
