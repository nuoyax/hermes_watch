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
    /// Effective yaw of the last rendered frame — lets a drag that releases
    /// the follow-lock continue smoothly from where the camera actually was.
    pub current_yaw: f64,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.35,
            zoom: 150.0,
            last_interaction: None,
            lock_lon: None,
            current_yaw: 0.0,
        }
    }
}

impl GlobeState {
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
        self.last_interaction = Some(std::time::Instant::now());
        if self.lock_lon.is_some() {
            // Releasing the follow-lock: adopt the camera's actual heading so
            // the view doesn't snap back to the stale manual yaw.
            self.yaw = self.current_yaw;
        }
        self.lock_lon = None; // manual drag releases the follow-lock
    }
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(40.0, 500.0);
        self.last_interaction = Some(std::time::Instant::now());
    }
    /// Camera yaw: either locked onto `lock_lon` (region faces the viewer,
    /// drifting with the real Earth rotation) or free (user yaw), paused 3 s
    /// after interaction.
    #[cfg(test)]
    pub(crate) fn effective_yaw_for_test(&self, now: std::time::Instant, gmst: f64) -> f64 {
        self.effective_yaw(now, gmst)
    }
    fn effective_yaw(&self, now: std::time::Instant, gmst: f64) -> f64 {
        if let Some(lon) = self.lock_lon {
            // Mesh places Earth-fixed lon L at world longitude (L + gmst).
            // rotate_to_cam maps world lon W to camera angle (W - yaw); the
            // camera sits on +z, which is world lon +90°. So the point faces
            // the viewer when (L + gmst) - yaw = 90°  ⇒  yaw = L + gmst - 90°.
            let lon_rot = lon.to_radians() + gmst;
            return lon_rot - std::f64::consts::FRAC_PI_2;
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
            // Mipmaps are essential: a 2048x1024 texture minified onto a few
            // hundred px sphere without them aliases into diagonal stripes.
            egui::TextureOptions {
                magnification: egui::TextureFilter::Linear,
                minification: egui::TextureFilter::Linear,
                mipmap_mode: Some(egui::TextureFilter::Linear),
                ..Default::default()
            },
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
/// Convention: camera looks down +z (a point faces the viewer when its
/// camera-space z equals +r). Yaw spins around the polar axis; pitch tilts.
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

/// Test hook: camera transform exposed for geometry unit tests.
#[cfg(test)]
pub fn rotate_to_cam_test_hook(v: V3, r: f64, yaw: f64, pitch: f64) -> V3 {
    rotate_to_cam(v, r, yaw, pitch)
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
    orbit_eci: &[[f64; 3]],
    sat_pos: Option<GeoPoint>,
) {
    let center = rect.center();
    let r = cam.zoom;
    // Whole-scene scale: everything except the backdrop (globe, satellite
    // model, orbit line, labels) scales together with the zoom level.
    let scale = (r / 150.0).clamp(0.25, 3.5);
    let now = std::time::Instant::now();
    let yaw = cam.effective_yaw(now, earth_rot);
    cam.current_yaw = yaw; // remember for a smooth release of the follow-lock
    let pitch = cam.pitch;

    // Deep space + atmosphere limb.
    painter.rect_filled(rect, 0.0, Color32::from_rgb(6, 8, 14));
    painter.circle_filled(center, r + 6.0, Color32::from_rgba_unmultiplied(90, 140, 220, 45));
    painter.circle_filled(center, r + 2.0, Color32::from_rgb(80, 120, 190));
    // Opaque sphere interior: the mesh never fully covers the disc near the
    // silhouette, so without this the blue limb shows through as stripes.
    painter.circle_filled(center, r, Color32::from_rgb(10, 14, 20));

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

    // Orbit ring — the TRUE inertial ellipse (smooth, from ECI positions),
    // drawn as a thin white line hidden where it passes behind the globe.
    // ECI → view: rotate the whole frame by -GMST so the Earth mesh (which is
    // drawn at +GMST) aligns with it; the ellipse keeps its real shape.
    let gmst = earth_rot;
    let eci_to_n = |p: &[f64; 3]| -> V3 {
        let (x, y, z) = (p[0], p[1], p[2]);
        let rr = (x * x + y * y + z * z).sqrt();
        if rr < 1.0 {
            return V3(0.0, 0.0, 0.0);
        }
        // Rotate ECI by -gmst about the z axis, then normalize.
        let (cg, sg) = gmst.sin_cos();
        let xr = x * cg + y * sg;
        let yr = -x * sg + y * cg;
        // Renderer convention: n = (cosφ·cosλ', sinφ, cosφ·sinλ') where the
        // mesh applies +gmst to Earth-fixed lon. ECI (x,y,z) with -gmst gives
        // (xr, z-height, yr) in that convention: x̂=cosφ·cosλ', ŷ=sinφ (up),
        // ẑ=cosφ·sinλ'. Map: n = (xr/rr, z/rr, yr/rr).
        V3(xr / rr, z / rr, yr / rr)
    };
    let mut prev: Option<(Pos2, bool)> = None;
    for p in orbit_eci {
        let v = eci_to_n(p);
        let alt = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 6371.0;
        let alt_r = (r as f64) * (1.0 + alt / 6371.0 * 0.45);
        let cam_v = rotate_to_cam(v, alt_r, yaw, pitch);
        let cur = project(cam_v, center);
        // Occlusion: a point is hidden when it's on the far side (z < 0) AND
        // its projection lands inside the globe disc.
        let visible = cur.1 > 0.0 || cur.0.distance(center) > r;
        if let Some((a, az)) = prev {
            if az && visible {
                painter.line_segment([a, cur.0], Stroke::new(2.5 * scale as f32, blend(Color32::WHITE, 0.10)));
                painter.line_segment([a, cur.0], Stroke::new((0.6 * scale as f32).max(0.4), blend(Color32::WHITE, 0.80)));
            }
        }
        prev = Some((cur.0, visible));
    }

    // The satellite: simple 3D model (body + two solar panels) oriented
    // toward Earth, like the classic satellite pictogram, plus label.
    if let Some(p) = sat_pos {
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.45);
        let (la_r, lo_r) = (
            p.lat_deg.to_radians(),
            (p.lon_deg + earth_rot.to_degrees()).to_radians(),
        );
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        let cam_v = rotate_to_cam(n, alt_r, yaw, pitch);
        let (sp, z) = project(cam_v, center);
        let behind = z < 0.0 && sp.distance(center) < r;
        if !behind {
            draw_satellite_model(&painter, sp, center, color, scale as f32);
            painter.text(
                sp + Vec2::new(14.0 * scale as f32, -12.0 * scale as f32),
                egui::Align2::LEFT_BOTTOM,
                format!("{} · {} km", sat.name, p.alt_km as i32),
                egui::FontId::proportional((11.0 * scale as f32).max(9.0)),
                Color32::WHITE,
            );
        }
    }
}

/// Draw a small satellite pictogram at `sp`: central body box + two solar
/// panel wings + a thin truss, tilted to point at the Earth's center (like
/// the standard satellite icon). Glow underneath keeps it readable.
fn draw_satellite_model(painter: &Painter, sp: Pos2, center: Pos2, color: Color32, scale: f32) {
    // Orientation: the satellite's panels face perpendicular to the line to
    // Earth; rotate the icon so "down" points at the globe center.
    let to_earth = (center - sp).normalized();
    let ang = to_earth.y.atan2(to_earth.x) - std::f32::consts::FRAC_PI_2;
    let rot = |v: Vec2| -> Vec2 {
        let (s, c) = ang.sin_cos();
        Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c) * scale
    };

    // Soft glow so the icon reads on both bright and dark ground.
    painter.circle_filled(sp, 12.0 * scale, blend(color, 0.25));

    // Solar panels: dark blue rectangles with cell lines, one on each side.
    let panel = |side: f32| {
        let c = sp + rot(Vec2::new(side * 10.0, 0.0));
        let (hw, hh) = (6.0, 3.5);
        let (u, v) = (rot(Vec2::new(hw, 0.0)), rot(Vec2::new(0.0, hh)));
        let corners = [c + u + v, c - u + v, c - u - v, c + u - v];
        painter.add(egui::Shape::convex_polygon(
            corners.to_vec(),
            Color32::from_rgb(40, 70, 170),
            Stroke::new(1.0, Color32::from_rgb(120, 150, 230)),
        ));
        // Two cell-divider lines across the panel.
        for f in [-0.33f32, 0.33] {
            painter.line_segment(
                [c + rot(Vec2::new(hw * f, -hh)), c + rot(Vec2::new(hw * f, hh))],
                Stroke::new(0.7, Color32::from_rgb(90, 120, 200)),
            );
        }
    };
    panel(-1.0);
    panel(1.0);

    // Body: light metal box.
    let (bw, bh) = (4.0, 4.5);
    let (u, v) = (rot(Vec2::new(bw, 0.0)), rot(Vec2::new(0.0, bh)));
    let corners = [sp + u + v, sp - u + v, sp - u - v, sp + u - v];
    painter.add(egui::Shape::convex_polygon(
        corners.to_vec(),
        Color32::from_rgb(225, 228, 235),
        Stroke::new(1.0, Color32::from_rgb(110, 115, 130)),
    ));
    // Dish antenna pointing at Earth.
    let tip = sp + rot(Vec2::new(0.0, 8.0));
    painter.line_segment([sp, tip], Stroke::new(1.0, Color32::from_rgb(200, 205, 215)));
    painter.circle_filled(tip, 1.6 * scale, Color32::WHITE);
}

fn blend(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)
}

/// Sun direction in EARTH-FIXED frame (unit vector) at `time`: points from
/// Earth's center toward the subsolar point. The renderer applies the same
/// +GMST rotation to both the mesh normals and this vector, so it must be
/// expressed in the plain Earth-fixed frame (NO GMST subtraction).
pub fn sun_direction(time: DateTime<Utc>) -> V3 {
    // Subsolar point: latitude = solar declination, longitude = where local
    // solar noon is right now (UTC hour angle).
    let day = time.ordinal() as f64;
    let decl_deg = -23.44 * ((2.0 * std::f64::consts::PI * (day - 81.0) / 365.25).sin());
    let utc_hours =
        time.hour() as f64 + time.minute() as f64 / 60.0 + time.second() as f64 / 3600.0;
    // Subsolar longitude: −15° per hour from local noon (12:00 UTC → 0°).
    let subsolar_lon = -15.0 * (utc_hours - 12.0);

    let (la, lo) = (decl_deg.to_radians(), subsolar_lon.to_radians());
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
