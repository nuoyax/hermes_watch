//! Real-texture 3D Earth: NASA Blue Marble texture on a sphere with correct
//! per-pixel lighting (sun from actual UTC time), camera auto-rotation (the
//! globe appears to spin), and the focused satellite's orbit ring.

use crate::data::model::Sat;
use crate::orbit::GeoPoint;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Per-pane camera state for the globe.
#[derive(Debug, Clone, Copy)]
pub struct GlobeState {
    /// User yaw offset (radians) on top of the auto-spin.
    pub yaw: f64,
    /// Tilt toward/away from the viewer (radians).
    pub pitch: f64,
    /// Globe radius in pixels.
    pub zoom: f32,
    /// Last interaction time (for auto-spin resume).
    pub last_interaction: Option<std::time::Instant>,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.35,
            zoom: 110.0,
            last_interaction: None,
        }
    }
}

impl GlobeState {
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
        self.last_interaction = Some(std::time::Instant::now());
    }
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(40.0, 400.0);
        self.last_interaction = Some(std::time::Instant::now());
    }
    /// Total camera yaw: user offset + slow auto-spin (one turn ≈ 80 s),
    /// pausing a few seconds after user interaction.
    pub fn effective_yaw(&self, now: std::time::Instant) -> f64 {
        let idle = self
            .last_interaction
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(f64::INFINITY);
        let spin = if idle > 3.0 { now.elapsed().as_secs_f64() * 0.08 } else { 0.0 };
        self.yaw + spin
    }
}

/// Textured Earth: lazily-decoded Blue Marble texture.
pub struct Earth {
    tex: Vec<Vec<[u8; 3]>>,
    tex_w: usize,
    tex_h: usize,
}

impl Earth {
    pub fn load() -> Self {
        let img = image::load_from_memory(include_bytes!("../../../earth_day.jpg"))
            .expect("embedded earth texture")
            .to_rgb8();
        let img = image::imageops::resize(&img, 512, 256, image::imageops::FilterType::Triangle);
        let (w, h) = (img.width() as usize, img.height() as usize);
        let tex = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| {
                        let p = img.get_pixel(x as u32, y as u32);
                        [p[0], p[1], p[2]]
                    })
                    .collect()
            })
            .collect();
        Self { tex, tex_w: w, tex_h: h }
    }

    fn sample(&self, lat_deg: f64, lon_deg: f64) -> [u8; 3] {
        let x = ((lon_deg + 180.0) / 360.0 * (self.tex_w - 1) as f64)
            .round()
            .clamp(0.0, (self.tex_w - 1) as f64) as usize;
        let y = ((90.0 - lat_deg) / 180.0 * (self.tex_h - 1) as f64)
            .round()
            .clamp(0.0, (self.tex_h - 1) as f64) as usize;
        self.tex[y][x]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct V3(pub f64, pub f64, pub f64);

impl V3 {
    fn dot(self, o: V3) -> f64 {
        self.0 * o.0 + self.1 * o.1 + self.2 * o.2
    }
}

/// Earth-fixed (lat, lon, radius) → camera-space point.
/// `earth_rot` is GMST rotation: longitude 0 drawn at its true inertial angle.
fn to_camera(lat_deg: f64, lon_deg: f64, r: f64, cam: &GlobeState, earth_rot: f64) -> V3 {
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians() + earth_rot);
    let v = V3(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
    rotate_to_cam(v, r, cam)
}

/// World-frame unit direction → camera frame, scaled by r.
fn rotate_to_cam(v: V3, r: f64, cam: &GlobeState) -> V3 {
    let (cp, sp) = (cam.pitch.cos(), cam.pitch.sin());
    let y2 = v.1 * cp - v.2 * sp;
    let z2 = v.1 * sp + v.2 * cp;
    let (cy, sy) = (cam.yaw.cos(), cam.yaw.sin());
    V3(
        (v.0 * cy + z2 * sy) * r,
        y2 * r,
        (-v.0 * sy + z2 * cy) * r,
    )
}

fn project(v: V3, center: Pos2) -> (Pos2, f64) {
    (Pos2::new(center.x + v.0 as f32, center.y - v.1 as f32), v.2)
}

/// Render the textured, rotating Earth + focused satellite's orbit.
#[allow(clippy::too_many_arguments)]
pub fn show_globe(
    painter: &Painter,
    rect: Rect,
    cam: &GlobeState,
    earth: &Earth,
    sun_dir_ef: V3,     // sun direction in Earth-fixed frame (unit)
    earth_rot: f64,     // GMST rotation, radians
    sat: Option<&Sat>,
    orbit: &[GeoPoint],
    sat_pos: Option<GeoPoint>,
) {
    let center = rect.center();
    let r = cam.zoom;
    let now = std::time::Instant::now();
    // The visible spin: camera slowly orbits the globe.
    let spin_cam = GlobeState { yaw: cam.effective_yaw(now), pitch: cam.pitch, zoom: r, last_interaction: None };

    // Deep space + atmosphere limb.
    painter.rect_filled(rect, 0.0, Color32::from_rgb(6, 8, 14));
    painter.circle_filled(center, r + 5.0, Color32::from_rgba_unmultiplied(90, 140, 220, 50));
    painter.circle_filled(center, r + 1.5, Color32::from_rgb(80, 120, 190));

    // Sun direction in camera frame (sun is fixed in inertial space; the
    // Earth-fixed sun vector rotates with the globe).
    let sun_cam = rotate_to_cam(sun_dir_ef, 1.0, &spin_cam);

    // Textured sphere with day/night shading.
    draw_textured_sphere(painter, center, r, &spin_cam, earth, sun_cam, earth_rot);

    let Some(sat) = sat else { return };
    let color = sat.group.color();

    // Orbit ring — glow + line, fading where behind the globe.
    let mut prev: Option<(Pos2, f64)> = None;
    for p in orbit {
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.35);
        let cur = project(to_camera(p.lat_deg, p.lon_deg, alt_r, &spin_cam, earth_rot), center);
        if let Some((a, az)) = prev {
            let fade = if az > 0.0 && cur.1 > 0.0 { 0.95 } else { 0.30 };
            painter.line_segment([a, cur.0], Stroke::new(4.0, blend(color, 0.22 * fade)));
            painter.line_segment([a, cur.0], Stroke::new(1.6, blend(color, fade)));
        }
        prev = Some(cur);
    }

    // The satellite: glowing halo + white-outlined dot + label.
    if let Some(p) = sat_pos {
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.35);
        let (sp, z) = project(to_camera(p.lat_deg, p.lon_deg, alt_r, &spin_cam, earth_rot), center);
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

/// Per-pixel textured sphere via inverse mapping in CAMERA space:
/// the surface normal in camera space is just the normalized screen offset,
/// which makes lighting trivial and exact.
fn draw_textured_sphere(
    painter: &Painter,
    center: Pos2,
    r: f32,
    cam: &GlobeState,
    earth: &Earth,
    sun_cam: V3,
    earth_rot: f64,
) {
    let step = ((r / 50.0).max(2.5)).min(6.0) as f64;
    let ir = r as f64;
    let (cy, sy) = (cam.yaw.cos(), cam.yaw.sin());
    let (cp, sp) = (cam.pitch.cos(), cam.pitch.sin());

    let mut y = -r as f64;
    while y < ir {
        let half_width = (ir * ir - y * y).sqrt();
        let mut x = -half_width;
        while x < half_width {
            // Camera-space unit normal from the disc coordinates.
            let zc = (1.0 - x * x / (ir * ir) - y * y / (ir * ir)).max(0.0).sqrt();
            let n_cam = V3(x / ir, y / ir, zc);

            // Day/night: dot with camera-frame sun.
            let d = n_cam.dot(sun_cam).clamp(0.0, 1.0);
            let shade = 0.10 + 0.95 * d;

            // Inverse-rotate to world (undo yaw then pitch) to get the
            // Earth-fixed direction (with GMST rotation still applied).
            let x1 = n_cam.0 * cy - n_cam.2 * sy;
            let z1 = n_cam.0 * sy + n_cam.2 * cy;
            let y0 = n_cam.1 * cp + z1 * sp;
            let z0 = -n_cam.1 * sp + z1 * cp;

            // Undo GMST to get Earth-fixed lat/lon for the texture.
            let lat = y0.clamp(-1.0, 1.0).asin().to_degrees();
            let lon = (z0.atan2(x1).to_degrees() - earth_rot.to_degrees() + 180.0).rem_euclid(360.0) - 180.0;

            let tex = earth.sample(lat, lon);
            let c = Color32::from_rgb(
                (tex[0] as f64 * shade).min(255.0) as u8,
                (tex[1] as f64 * shade).min(255.0) as u8,
                (tex[2] as f64 * shade).min(255.0) as u8,
            );
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(center.x + x as f32, center.y + y as f32),
                    Vec2::new(step as f32 + 0.5, step as f32 + 0.5),
                ),
                0.0,
                c,
            );
            x += step;
        }
        y += step;
    }
}

fn blend(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)
}

/// Sun direction in Earth-fixed frame (unit vector) at `time`:
/// points from Earth's center toward the subsolar point.
pub fn sun_direction(time: DateTime<Utc>) -> V3 {
    // Solar declination (approx).
    let day = time.ordinal() as f64;
    let decl_deg = -23.44 * ((2.0 * std::f64::consts::PI * (day - 81.0) / 365.25).sin());
    // Subsolar longitude: where local solar noon is now.
    let utc_hours =
        time.hour() as f64 + time.minute() as f64 / 60.0 + time.second() as f64 / 3600.0;
    let lon_deg = 180.0 - utc_hours * 15.0;
    let lon_deg = lon_deg.rem_euclid(360.0) - 180.0;

    let (la, lo) = (decl_deg.to_radians(), lon_deg.to_radians());
    V3(la.cos() * lo.cos(), la.sin(), la.cos() * lo.sin())
}

/// Earth rotation angle (GMST, radians) — used to place longitudes correctly.
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
