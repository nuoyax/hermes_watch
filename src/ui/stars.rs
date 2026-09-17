//! Real star field: 2,866 naked-eye stars from the HYG database (Hipparcos),
//! embedded at build time. Positions are equatorial (ra/dec radians),
//! rendered in the fixed celestial frame (stars don't rotate with the Earth).

use serde::Deserialize;

#[derive(Deserialize)]
struct Star {
    ra: f64,   // right ascension, radians
    dec: f64,  // declination, radians
    mag: f64,  // apparent magnitude (lower = brighter)
    ci: f64,   // color index (B-V): <0 blue, ~0.5 white, >1.5 red
}

const STARS_JSON: &str = include_str!("../../stars_bright.json");

pub struct StarField {
    /// (unit vector x,y,z in equatorial frame, size px, color)
    stars: Vec<([f64; 3], f32, egui::Color32)>,
}

impl StarField {
    pub fn load() -> Self {
        let raw: Vec<Star> = serde_json::from_str(STARS_JSON).expect("valid embedded stars");
        let stars = raw
            .iter()
            .map(|s| {
                let (ra, dec) = (s.ra, s.dec);
                // Equatorial unit vector; z = dec axis.
                let v = [
                    dec.cos() * ra.cos(),
                    dec.cos() * ra.sin(),
                    dec.sin(),
                ];
                let size = mag_to_size(s.mag);
                let color = color_index_to_color(s.ci);
                (v, size, color)
            })
            .collect();
        Self { stars }
    }

    /// Paint all stars into `painter` given a camera rotation (same yaw/pitch
    /// convention as the 3D globe, so the sky rotates when you drag).
    pub fn paint(&self, painter: &egui::Painter, center: egui::Pos2, scale: f32, yaw: f64, pitch: f64) {
        let (cy, sy) = (yaw.cos(), yaw.sin());
        let (cp, sp) = (pitch.cos(), pitch.sin());
        for (v, size, color) in &self.stars {
            // Equatorial: x,y in equatorial plane; map to screen like the globe
            // (celestial sphere conceptually "inside out": we look at it from inside).
            // ra → yaw spin, dec → latitude.
            let x = v[0];
            let y = v[2]; // dec up
            let z = v[1];
            // Pitch then yaw (matching globe camera).
            let y2 = y * cp - z * sp;
            let z2 = y * sp + z * cp;
            let x3 = x * cy + z2 * sy;
            let z3 = -x * sy + z2 * cy;
            // Only draw the front hemisphere (z3 > 0 faces the viewer).
            if z3 <= 0.02 {
                continue;
            }
            // Depth fade near limb.
            let fade = (z3 * 3.0).min(1.0) as f32;
            let p = egui::Pos2::new(
                center.x + (x3 * scale as f64) as f32,
                center.y - (y2 * scale as f64) as f32,
            );
            let c = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (255.0 * fade) as u8);
            painter.circle_filled(p, *size, c);
        }
    }
}

fn mag_to_size(mag: f64) -> f32 {
    // mag -1 (Sirius) → 2.6px, mag 5.5 → 0.8px
    ((5.8 - mag) * 0.45).clamp(0.6, 3.0) as f32
}

fn color_index_to_color(ci: f64) -> egui::Color32 {
    // B-V → rough stellar temperature color.
    if ci < 0.0 {
        egui::Color32::from_rgb(170, 195, 255) // blue
    } else if ci < 0.4 {
        egui::Color32::from_rgb(220, 230, 255) // blue-white
    } else if ci < 0.8 {
        egui::Color32::from_rgb(255, 250, 240) // white
    } else if ci < 1.4 {
        egui::Color32::from_rgb(255, 235, 200) // yellow-orange
    } else {
        egui::Color32::from_rgb(255, 205, 170) // red-orange
    }
}
