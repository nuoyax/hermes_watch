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
    /// Last manual drag — 5 s of idleness after it triggers an auto-reset.
    pub last_drag: Option<std::time::Instant>,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.35,
            zoom: 100.0,
            last_interaction: None,
            lock_lon: None,
            current_yaw: 0.0,
            last_drag: None,
        }
    }
}

impl GlobeState {
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
        self.last_interaction = Some(std::time::Instant::now());
        self.last_drag = self.last_interaction;
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
        self.effective_yaw(now, gmst, 0.0)
    }
    fn effective_yaw(&self, now: std::time::Instant, gmst: f64, sun_yaw: f64) -> f64 {
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
        // Free camera drifts gently back toward the sun-facing yaw.
        sun_yaw
    }

    /// Auto-reset: 5 s after the last manual drag, ease the camera back to
    /// the default view (default pitch; follow-lock yaw if one is active,
    /// otherwise the neutral yaw the free camera had before the drag).
    pub fn auto_reset(&mut self, base_yaw: f64) {
        const IDLE_RESET: f64 = 5.0;
        let idle = self
            .last_drag
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(f64::INFINITY);
        if idle < IDLE_RESET {
            return;
        }
        if self.pitch != Self::default().pitch || self.yaw != base_yaw {
            // Small per-frame factor → exponential glide of ~2 s (at 60 fps).
            let k = 0.03f32;
            self.pitch += (Self::default().pitch - self.pitch) * k as f64;
            self.yaw += (base_yaw - self.yaw) * k as f64;
        }
        if idle > IDLE_RESET + 5.0 {
            self.last_drag = None; // settled — stop easing
        }
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
    // Default view = from the sun's side: the lit hemisphere faces the
    // viewer. The camera yaw tracks the subsolar point (world angle =
    // subsolar lon + GMST; the camera sits at world +90°), so as time
    // passes the view slowly follows the sun like a solar-locked observer.
    // A manual drag overrides it freely; auto-reset glides back to the sun.
    let sun_yaw = sun_dir_ef.0.atan2(sun_dir_ef.2) + earth_rot
        - std::f64::consts::FRAC_PI_2;
    cam.auto_reset(sun_yaw);
    let yaw = cam.effective_yaw(now, earth_rot, sun_yaw);
    cam.current_yaw = yaw; // remember for a smooth release of the follow-lock
    let pitch = cam.pitch;

    // Deep space + atmosphere limb. Painting is clipped to the pane rect so
    // multi-pane layouts never bleed into neighbouring panes.
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(6, 8, 14));
    painter.circle_filled(center, r + 6.0, Color32::from_rgba_unmultiplied(90, 140, 220, 45));
    painter.circle_filled(center, r + 2.0, Color32::from_rgb(80, 120, 190));
    // Opaque sphere interior: the mesh never fully covers the disc near the
    // silhouette, so without this the blue limb shows through as stripes.
    painter.circle_filled(center, r, Color32::from_rgb(10, 14, 20));

    // === Textured sphere as a triangle mesh with per-vertex UV + shading ===
    // Sun is fixed in INERTIAL space (a real direction the light comes
    // from): rotate the earth-fixed sun vector by GMST into the world frame
    // (same Y-axis rotation the mesh normals use). Dragging the globe
    // rotates the lit hemisphere together with the Earth, instantly.
    let g = earth_rot;
    let (sg, cg) = g.sin_cos();
    let sun_world = V3(
        sun_dir_ef.0 * cg + sun_dir_ef.2 * sg,
        sun_dir_ef.1,
        -sun_dir_ef.0 * sg + sun_dir_ef.2 * cg,
    );
    let sun_norm = sun_world.dot(sun_world).sqrt();
    let tex = earth.texture(painter.ctx());
    let (lat_bands, lon_bands) = (72usize, 144usize);
    // Rebuild the sphere mesh only when the camera/sun/earth-rotation actually
    // changed; otherwise replay the cached mesh. This keeps the CPU cost at
    // ~10.5k vertex evaluations per pane only on moving frames (drag, sim
    // clock) — static frames are free.
    let mesh_key = (
        yaw.to_bits(),
        pitch.to_bits(),
        r.to_bits(),
        sun_dir_ef.0.to_bits(),
        sun_dir_ef.1.to_bits(),
        sun_dir_ef.2.to_bits(),
        earth_rot.to_bits(),
    );
    thread_local! {
        static MESH_CACHE: std::cell::RefCell<Option<((u64, u64, u32, u64, u64, u64, u64), egui::Mesh)>> =
            const { std::cell::RefCell::new(None) };
    }
    let mesh_changed = MESH_CACHE
        .with(|c| c.borrow().as_ref().map_or(true, |(k, _)| *k != mesh_key));
    if mesh_changed {
        let mut vertices: Vec<egui::epaint::Vertex> = Vec::with_capacity((lat_bands + 1) * (lon_bands + 1));
        let mut indices: Vec<u32> = Vec::new();
        let gdeg = earth_rot.to_degrees();
        for la in 0..=lat_bands {
            let lat = -90.0 + 180.0 * la as f64 / lat_bands as f64;
            for lo in 0..=lon_bands {
                let lon = -180.0 + 360.0 * lo as f64 / lon_bands as f64;
                // Surface normal in Earth-fixed frame (with GMST rotation
                // applied, since the texture is Earth-fixed too — camera
                // spin comes from yaw).
                let (la_r, lo_r) = (lat.to_radians(), (lon + gdeg).to_radians());
                let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
                let cam_v = rotate_to_cam(n, r as f64, yaw, pitch);
                let (pos, _z) = project(cam_v, center);

                // Lighting: sun fixed in the WORLD frame.
                let d = n.dot(sun_world).clamp(0.0, 1.0) / sun_norm;
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
                vertices.push(egui::epaint::Vertex { pos, uv, color: c });
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
        let mesh = egui::Mesh {
            vertices,
            indices,
            texture_id: tex.id(),
        };
        MESH_CACHE.with(|c| {
            *c.borrow_mut() = Some((mesh_key, mesh.clone()));
        });
        painter.add(mesh);
    } else {
        MESH_CACHE.with(|c| {
            if let Some((_, m)) = c.borrow().as_ref() {
                painter.add(m.clone());
            }
        });
    }

    // Subtle atmosphere terminator glow on the night side edge.
    // (skip — the vertex shading handles it)

    let Some(sat) = sat else { return };
    let color = sat.group.color();

    // Orbit ring — the TRUE inertial ellipse (smooth, from ECI positions),
    // drawn as a thin white line hidden where it passes behind the globe.
    // ECI axes map straight into the renderer's world frame (the Earth mesh
    // is drawn at +GMST, so inertial lon == world lon); the ring therefore
    // stays fixed while the Earth turns underneath, and the satellite marker
    // — which uses the same mapping — rides exactly on it.
    let eci_to_n = |p: &[f64; 3]| -> V3 {
        let (x, y, z) = (p[0], p[1], p[2]);
        let rr = (x * x + y * y + z * z).sqrt();
        if rr < 1.0 {
            return V3(0.0, 0.0, 0.0);
        }
        // Renderer convention: n = (cosφ·cosλ, sinφ, cosφ·sinλ) with λ the
        // world (inertial) longitude = atan2(y, x). Map: n = (x/rr, z/rr, y/rr).
        V3(x / rr, z / rr, y / rr)
    };
    let mut prev: Option<(Pos2, bool)> = None;
    // Collect consecutive visible points into runs, then draw each run as a
    // single smooth polyline (one shape = uniform joints, no dotted look).
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    for p in orbit_eci {
        let v = eci_to_n(p);
        let alt = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 6371.0;
        // Modest exaggeration: hugs the globe visually while LEO orbits still
        // clear the surface (displayed km values stay true).
        let alt_r = (r as f64) * (1.0 + alt / 6371.0 * 0.4);
        let cam_v = rotate_to_cam(v, alt_r, yaw, pitch);
        let cur = project(cam_v, center);
        // Occlusion: a point is hidden when it's on the far side (z < 0) AND
        // its projection lands inside the globe disc.
        let visible = cur.1 > 0.0 || cur.0.distance(center) > r;
        if visible {
            match runs.last_mut() {
                Some(run) if prev.is_some() => run.push(cur.0),
                _ => runs.push(vec![cur.0]),
            }
        } else {
            runs.push(Vec::new()); // break the polyline at the globe's edge
        }
        prev = Some((cur.0, visible));
    }
    let _ = prev;
    // Glow pass for depth, then a crisp core line. Widths scale with zoom but
    // never fall below ~1 px, so dense points join into a smooth curve
    // instead of reading as separate dots.
    for run in &runs {
        if run.len() < 2 {
            continue;
        }
        painter.add(egui::Shape::line(
            run.clone(),
            Stroke::new((1.6 * scale as f32).max(1.0), blend(Color32::WHITE, 0.10)),
        ));
        painter.add(egui::Shape::line(
            run.clone(),
            Stroke::new((0.7 * scale as f32).max(0.8), blend(Color32::WHITE, 0.85)),
        ));
    }

    // The satellite: simple 3D model (body + two solar panels) oriented
    // toward Earth, like the classic satellite pictogram, plus label.
    if let Some(p) = sat_pos {
        // Same exaggeration as the orbit ring so the marker stays on it.
        let alt_r = (r as f64) * (1.0 + p.alt_km / 6371.0 * 0.4);
        let (la_r, lo_r) = (
            p.lat_deg.to_radians(),
            (p.lon_deg + earth_rot.to_degrees()).to_radians(),
        );
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        let cam_v = rotate_to_cam(n, alt_r, yaw, pitch);
        let (sp, z) = project(cam_v, center);
        let behind = z < 0.0 && sp.distance(center) < r;
        if !behind {
            draw_spacecraft(&painter, sp, center, sat, color, scale as f32);
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

/// Draw the focused spacecraft: a real photo sprite chosen by name/group
/// (ISS-style station, Hubble, generic satellite), with a soft glow so it
/// reads on both bright and dark ground. Falls back to the vector pictogram
/// if the texture hasn't loaded.
fn draw_spacecraft(
    painter: &Painter,
    sp: Pos2,
    center: Pos2,
    sat: &Sat,
    color: Color32,
    scale: f32,
) {
    // Soft glow behind the sprite.
    painter.circle_filled(sp, 14.0 * scale, blend(color, 0.25));

    if let Some(tex) = spacecraft_texture(painter.ctx(), &sat.name, sat.group) {
        // Photo sprite: rotate so "down" points at the globe center, size
        // scaled with the whole scene but capped so it never swamps the globe.
        let to_earth = (center - sp).normalized();
        let ang = to_earth.y.atan2(to_earth.x) + std::f32::consts::FRAC_PI_2;
        let size = (34.0 * scale).clamp(22.0, 64.0);
        let (w, h) = (tex.aspect_ratio * size, size);
        let mesh = sprite_mesh(tex.id(), sp, w, h, ang, scale);
        painter.add(mesh);
        return;
    }
    draw_satellite_model(painter, sp, center, color, scale);
}

/// Pick a real-photo texture for the spacecraft by name / group.
fn spacecraft_texture(
    ctx: &egui::Context,
    name: &str,
    group: crate::data::model::SatGroup,
) -> Option<&'static SpriteLoaded> {
    struct SpriteDef {
        id: &'static str,
        keywords: &'static [&'static str],
        img: &'static [u8],
    }
    const SPRITES: &[SpriteDef] = &[
        SpriteDef {
            id: "iss",
            keywords: &["ISS", "CSS", "TIANGONG", "TIANHE", "ZARYA", "MIR", "PROGRESS", "CYGNUS", "DRAGON", "SOYUZ", "SHENZHOU"],
            img: include_bytes!("../../../assets/iss.png"),
        },
        SpriteDef {
            id: "hst",
            keywords: &["HST", "HUBBLE"],
            img: include_bytes!("../../../assets/hst.png"),
        },
        SpriteDef {
            id: "sat",
            keywords: &[],
            img: include_bytes!("../../../assets/sat_generic.png"),
        },
    ];
    let upper = name.to_uppercase();
    let chosen = SPRITES
        .iter()
        .find(|s| !s.keywords.is_empty() && s.keywords.iter().any(|k| upper.contains(k)))
        .unwrap_or(&SPRITES[2]);
    let _ = group;
    Some(load_sprite(ctx, chosen.id, chosen.img))
}

struct SpriteLoaded {
    handle: egui::TextureHandle,
    aspect_ratio: f32,
}

impl SpriteLoaded {
    fn id(&self) -> egui::TextureId {
        self.handle.id()
    }
}

/// Lazily upload a sprite texture; leaked Box gives a stable 'static ref.
fn load_sprite(ctx: &egui::Context, id: &'static str, img_bytes: &'static [u8]) -> &'static SpriteLoaded {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static CACHE: OnceLock<parking_lot::Mutex<HashMap<&'static str, &'static SpriteLoaded>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
    let mut c = cache.lock();
    let leaked = c.entry(id).or_insert_with(move || {
        let img = image::load_from_memory(img_bytes)
            .expect("embedded spacecraft photo")
            .to_rgba8();
        let aspect = img.width() as f32 / img.height() as f32;
        let size = [img.width() as usize, img.height() as usize];
        let pixels: Vec<egui::Color32> = img
            .pixels()
            .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        let handle = ctx.load_texture(
            format!("craft-{}", id),
            egui::ColorImage { size, pixels },
            egui::TextureOptions::default(),
        );
        Box::leak(Box::new(SpriteLoaded { handle, aspect_ratio: aspect }))
    });
    *leaked
}

/// Build a rotated, textured quad mesh for a sprite.
fn sprite_mesh(tex: egui::TextureId, sp: Pos2, w: f32, h: f32, ang: f32, scale: f32) -> egui::Mesh {
    let (s, c) = ang.sin_cos();
    let u = Vec2::new(c, s) * (w * 0.5 * scale);
    let v = Vec2::new(-s, c) * (h * 0.5 * scale);
    let mut mesh = egui::Mesh::with_texture(tex);
    let corners = [
        (sp + u + v, egui::pos2(1.0, 1.0)),
        (sp - u + v, egui::pos2(0.0, 1.0)),
        (sp - u - v, egui::pos2(0.0, 0.0)),
        (sp + u - v, egui::pos2(1.0, 0.0)),
    ];
    let base = mesh.vertices.len() as u32;
    for (pos, uv) in corners {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    mesh
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

/// Sun direction in the EARTH-FIXED frame (unit vector) at `time`: points
/// from Earth's center toward the subsolar point. Lighting is done entirely
/// in this frame (normals without the GMST term), so the day/night pattern
/// is fixed to the geography and can never shift when the user drags.
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
