//! Baked-in Real Earth geometry: parsed Natural Earth 110m coastline/land,
//! embedded via include_str! at build time.
//!
//! Provides `COASTLINES: &[&[(f64, f64)]]` — polylines of (lat, lon) — usable
//! by both the 2D world map and the 3D globe.

use serde::Deserialize;

#[derive(Deserialize)]
struct GeoJson {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Feature {
    geometry: Geometry,
}

#[derive(Deserialize)]
struct Geometry {
    #[serde(rename = "type")]
    _kind: String,
    coordinates: serde_json::Value,
}

const COASTLINE_JSON: &str = include_str!("../../coastline110.json");

/// Parse once lazily into flat polylines.
pub fn coastlines() -> &'static Vec<Vec<(f64, f64)>> {
    use std::sync::OnceLock;
    static COAST: OnceLock<Vec<Vec<(f64, f64)>>> = OnceLock::new();
    COAST.get_or_init(|| {
        let gj: GeoJson = serde_json::from_str(COASTLINE_JSON).expect("valid embedded geojson");
        let mut out = Vec::new();
        for f in &gj.features {
            if let serde_json::Value::Array(lines) = &f.geometry.coordinates {
                // MultiLineString: [[[lon,lat],...],...]; LineString: [[lon,lat],...]
                let is_multi = lines.first().is_some_and(|l| {
                    l.as_array().is_some_and(|p| p.first().is_some_and(|q| q.is_array()))
                });
                if is_multi {
                    for line in lines {
                        out.push(parse_line(line));
                    }
                } else {
                    out.push(parse_line(&serde_json::Value::Array(lines.clone())));
                }
            }
        }
        out
    })
}

fn parse_line(v: &serde_json::Value) -> Vec<(f64, f64)> {
    v.as_array()
        .map(|pts| {
            pts.iter()
                .filter_map(|p| {
                    let a = p.as_array()?;
                    let lon = a.first()?.as_f64()?;
                    let lat = a.get(1)?.as_f64()?;
                    Some((lat, lon))
                })
                .collect()
        })
        .unwrap_or_default()
}
