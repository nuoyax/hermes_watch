//! Real-texture 3D Earth rendered as a triangulated GPU mesh (Google-Earth
//! style): textured sphere with per-vertex UV mapping, correct day/night
//! lighting via a dark overlay pass, continuous auto-rotation, and the
//! focused satellite's orbit ring.

use crate::data::model::Sat;
use crate::orbit::GeoPoint;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Per-pane camera state.
#[derive(Debug, Clone, Copy)]
pub struct GlobeState {
    /// User yaw offset (radians).
    pub yaw: f64,
    /// Tilt (radians).
    pub pitch: f64,
    /// Globe radius in pixels.
    pub zoom: f32,
    /// Last interaction (pause auto-spin while the user drags).
    pub last_interaction: Option<std::time::Instant>,
    /// When set, the camera tracks this Earth-fixed longitude: the location
    /// stays facing the viewer as the Earth turns underneath (real rotation).
    pub lock_lon: Option<f64>,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.35,
            zoom: 150.0,
            last_interaction: None,
            lock_lon: None,
        }
    }
}

impl GlobeState {
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
        self.last_interaction = Some(std::time::Instant::now());
        self.lock_lon = None; // manual drag releases the follow-lock
    }
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(40.0, 500.0);
        self.last_interaction = Some(std::time::Instant::now());
    }
    /// Camera yaw: either locked onto `lock_lon` (region faces the viewer,
    /// drifting with the real Earth rotation) or slow free auto-spin,
    /// paused 3 s after interaction.
    fn effective_yaw(&self, now: std::time::Instant, gmst: f64) -> f64 {
        if let Some(lon) = self.lock_lon {
            return -(lon.to_radians() + gmst);
        }
        let idle = self
            .last_interaction
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(f64::INFINITY);
        if idle < 3.0 {
            // Hold still right after a drag — no drift.
            return self.yaw;
        }
        self.yaw
    }
}

/// GPU-textured Earth.
pub struct Earth {
    img: image::RgbImage,
    tex: Option<egui::TextureHandle>,
}

impl Earth {
    pub fn load() -> Self {
        let img = image::load_from_memory(include_bytes!("../../../earth_day.jpg"))
            .expect("embedded earth texture")
            .to_rgb8();
        let img = image::imageops::resize(&img, 2048, 1024, image::imageops::FilterType::Triangle);
        Self { img, tex: None }
    }

    fn texture(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        if let Some(t) = &self.tex {
            return t.clone();
        }
        let size = [self.img.width() as usize, self.img.height() as usize];
        let pixels: Vec<egui::Color32> = self
            .img
            .pixels()
            .map(|p| egui::Color32::from_rgb(p[0], p[1], p[2]))
            .collect();
        let handle = ctx.load_texture(
            "earth-day",
            egui::ColorImage { size, pixels },
            egui::TextureOptions::LINEAR,
        );
        self.tex = Some(handle.clone());
        handle
    }
}

#[derive(Debug, Clone, Copy)]
pub struct V3(pub f64, pub f64, pub f64);

impl V3 {
    fn dot(self, o: V3) -> f64 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
}

/// Camera transform: world unit vector → camera space (scaled by r).
fn rotate_to_cam(v: V3, r: f64, yaw: f64, pitch: f64) -> V3 {
    let (cp, sp) = (pitch.cos(), pitch.sin());
    let y2 = v.1 * cp - v.2 * sp;
    let z2 = v.1 * sp + v.2 * cp;
    let (cy, sy) = (yaw.cos(), yaw.sin());
    V3((v.0 * cy + z2 * sy) * r, y2 * r, (-v.0 * sy + z2 * cy) * r)
}

fn project(v: V3, center: Pos2) -> (Pos2, f64) {
    (Pos2::new(center.x + v.0 as f32, center.y - v.1 as f32), v.2)
}

/// Render the textured rotating Earth + focused satellite.
#[allow(clippy::too_many_arguments)]
pub fn show_globe(
    painter: &Painter,
    rect: Rect,
    cam: &mut GlobeState,
    earth: &mut Earth,
    sun_dir_ef: V3,
    earth_rot: f64,
    sat: Option<&Sat>,
    orbit: &[GeoPoint],
    sat_pos: Option<GeoPoint>,
) {
    let center = rect.center();
    let r = cam.zoom;
    let now = std::time::Instant::now();
    let yaw = cam.effective_yaw(now, earth_rot);
    let pitch = cam.pitch;

    // Deep space + atmosphere limb.
    painter.rect_filled(rect, 0.0, Color32::from_rgb(6, 8, 14));
    painter.circle_filled(center, r + 6.0, Color32::from_rgba_unmultiplied(90, 140, 220, 45));
    painter.circle_filled(center, r + 2.0, Color32::from_rgb(80, 120, 190));

    // === Textured sphere as a triangle mesh with per-vertex UV + shading ===
    let tex = earth.texture(painter.ctx());
    let (lat_bands, lon_bands) = (48usize, 96usize);
    let mut vertices: Vec<egui::epaint::Vertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for la in 0..=lat_bands {
        let lat = -90.0 + 180.0 * la as f64 / lat_bands as f64;
        for lo in 0..=lon_bands {
            let lon = -180.0 + 360.0 * lo as f64 / lon_bands as f64;
            // Surface normal in Earth-fixed frame (with GMST rotation applied,
            // since the texture is Earth-fixed too — camera spin comes from yaw).
            let (la_r, lo_r) = (lat.to_radians(), (lon + earth_rot.to_degrees()).to_radians());
            let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
            let cam_v = rotate_to_cam(n, r as f64, yaw, pitch);
            let (pos, _z) = project(cam_v, center);

            // Lighting: sun in camera frame vs surface normal in camera frame.
            let sun_cam = rotate_to_cam(sun_dir_ef, 1.0, yaw, pitch);
            let n_cam = rotate_to_cam(n, 1.0, yaw, pitch);
            let d = n_cam.dot(sun_cam).clamp(0.0, 1.0);
            let shade = 0.10 + 0.92 * d;
            let c = Color32::from_rgba_unmultiplied(
                (255.0 * shade) as u8,
                (255.0 * shade) as u8,
                (255.0 * shade) as u8,
                255,
            );

            // UV: equirectangular.
            let uv = egui::Pos2::new(
                ((lon + 180.0) / 360.0) as f32,
                ((90.0 - lat) / 180.0) as f32,
            );
            vertices.push(egui::epaint::Vertex {
                pos,
                uv,
                color: c,
            });
        }
    }
    let stride = lon_bands + 1;
    for la in 0..lat_bands {
        for lo in 0..lon_bands {
            let a = (la * stride + lo) as u32;
            let b = a + 1;
            let c_ = a + stride as u32;
            let d = c_ + 1;
            indices.extend_from_slice(&[a, c_, b, b, c_, d]);
        }
    }
    painter.add(egui::Mesh {
        vertices,
        indices,
        texture_id: tex.id(),
    });

    // Subtle atmosphere terminator glow on the night side edge.
    // (skip — the vertex shading handles it)

    let Some(sat) = sat else { return };
    let color = sat.group.color();

    // Orbit ring — glow + line, fading where behind the globe.
    let mut prev: Option<(Pos2, f64)> = None;
    for p in orbit {
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.35);
        let (la_r, lo_r) = (
            p.lat_deg.to_radians(),
            (p.lon_deg + earth_rot.to_degrees()).to_radians(),
        );
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        let cam_v = rotate_to_cam(n, alt_r, yaw, pitch);
        let cur = project(cam_v, center);
        if let Some((a, az)) = prev {
            let fade = if az > 0.0 && cur.1 > 0.0 { 0.95 } else { 0.28 };
            painter.line_segment([a, cur.0], Stroke::new(4.0, blend(color, 0.20 * fade)));
            painter.line_segment([a, cur.0], Stroke::new(1.6, blend(color, fade)));
        }
        prev = Some(cur);
    }

    // The satellite: glowing halo + white-outlined dot + label.
    if let Some(p) = sat_pos {
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.35);
        let (la_r, lo_r) = (
            p.lat_deg.to_radians(),
            (p.lat_deg * 0.0 + p.lon_deg + earth_rot.to_degrees()).to_radians(),
        );
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        let cam_v = rotate_to_cam(n, alt_r, yaw, pitch);
        let (sp, z) = project(cam_v, center);
        let behind = z < 0.0 && sp.distance(center) < r;
        if !behind {
            painter.circle_filled(sp, 9.0, blend(color, 0.30));
            painter.circle_filled(sp, 4.5, color);
            painter.circle_stroke(sp, 6.0, Stroke::new(1.5, Color32::WHITE));
            painter.text(
                sp + Vec2::new(12.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{} · {} km", sat.name, p.alt_km as i32),
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }
    }
}

fn blend(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)
}

/// Sun direction in EARTH-FIXED frame (unit vector) at `time`: points from
/// Earth's center toward the subsolar point, expressed in the same frame the
/// texture/longitudes use. Because the mesh rotates longitudes by GMST, the
/// sun must be counter-rotated by the same amount to stay physically correct.
pub fn sun_direction(time: DateTime<Utc>) -> V3 {
    // Subsolar point: latitude = solar declination, longitude = where local
    // solar noon is right now (UTC hour angle).
    let day = time.ordinal() as f64;
    let decl_deg = -23.44 * ((2.0 * std::f64::consts::PI * (day - 81.0) / 365.25).sin());
    let utc_hours =
        time.hour() as f64 + time.minute() as f64 / 60.0 + time.second() as f64 / 3600.0;
    let subsolar_lon = (180.0 - utc_hours * 15.0).rem_euclid(360.0) - 180.0;

    // The mesh places Earth-fixed lon L at world angle (L + GMST). To express
    // the sun in the mesh's Earth-fixed frame, subtract GMST.
    let gmst_deg_now = gmst_deg(time);
    let lon_ef = subsolar_lon - gmst_deg_now;

    let (la, lo) = (decl_deg.to_radians(), lon_ef.to_radians());
    V3(la.cos() * lo.cos(), la.sin(), la.cos() * lo.sin())
}

/// Earth rotation (GMST, radians).
pub fn earth_rotation(time: DateTime<Utc>) -> f64 {
    gmst_deg(time).to_radians()
}

/// Greenwich Mean Sidereal Time in degrees.
pub fn gmst_deg(time: DateTime<Utc>) -> f64 {
    let day = NaiveDate::from_ymd_opt(time.year(), time.month(), time.day()).unwrap_or_default();
    let jd0 = 1_721_425.5 + day.num_days_from_ce() as f64 - 0.5;
    let jd = jd0
        + time.hour() as f64 / 24.0
        + time.minute() as f64 / 1440.0
        + time.second() as f64 / 86_400.0;
    let t = (jd - 2_451_545.0) / 36_525.0;
    let gmst = 280.46061837 + 36_079.8750114 * t + 0.000_387_933 * t * t;
    gmst.rem_euclid(360.0)
}
