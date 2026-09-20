//! Real-texture 3D Earth rendered as a triangulated GPU mesh (Google-Earth
//! style): textured sphere with per-vertex UV mapping, correct day/night
//! lighting via a dark overlay pass, continuous auto-rotation, and the
//! focused satellite's orbit ring.

use crate::data::model::Sat;
use crate::orbit::GeoPoint;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Sphere tessellation, in latitude bands × longitude bands (vertices per
/// frame ≈ (lat+1)·(lon+1) ≈ 4.8k, → ~9.4k triangles). Trade-off: more bands
/// smooth the silhouette but every rebuild (drag / sim clock) walks the whole
/// grid, so this stays well below what a 4-pane 60 fps layout can afford.
const GLOBE_LAT_BANDS: usize = 48;
const GLOBE_LON_BANDS: usize = 96;

/// A pane counts as "the user is dragging" for this long (s) after the last
/// pointer interaction. Deliberately separate from the mesh rebuild throttle
/// below even though both happen to be 0.5 s: this one is gesture state
/// (latency matters), the other is a render budget.
const DRAG_ACTIVE_SECS: f64 = 0.5;

/// Minimum wall-clock gap between full sphere-mesh rebuilds. The sim clock
/// advances every frame (`earth_rot` changes constantly), so without this the
/// 10.5k-vertex sphere would be re-evaluated every frame in every pane — 4
/// panes at ~60 fps sustained is enough to hang the AMD OpenGL driver. Kept at
/// 0.5 s (2 Hz) rather than the 100 ms an earlier comment implied: 4 panes
/// rebuilding at 10 Hz was judged too risky for that driver. A drag bypasses
/// the throttle (see `dragging`) because gesture latency matters more there.
const MESH_REBUILD_INTERVAL_SECS: f64 = 0.5;

/// ISS whitewash: fraction of the baked material colour mixed toward white.
/// Trade-off between night-side readability (module/panel geometry stays
/// visible against the dark limb) and colour fidelity of the real model.
const ISS_NIGHT_WHITEWASH: f32 = 0.40;

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
    /// Latitude of the locked location (for the marker + pitch centering).
    pub lock_lat: Option<f64>,
    /// Display name of the locked location (drawn next to the marker).
    pub lock_label: Option<&'static str>,
    /// Effective yaw of the last rendered frame — lets a drag (or an unlock)
    /// continue smoothly from where the camera actually was. Updated every
    /// rendered frame, including while a follow-lock is active.
    pub current_yaw: f64,
    /// Last manual drag — 5 s of idleness after it triggers an auto-reset.
    pub last_drag: Option<std::time::Instant>,
    /// Pitch the camera is easing toward (timezone jump centers on the
    /// zone's latitude; unlocking eases back to `DEFAULT_PITCH`). `None`
    /// once settled.
    pub pitch_target: Option<f64>,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: Self::DEFAULT_PITCH,
            zoom: 100.0,
            last_interaction: None,
            lock_lon: None,
            lock_lat: None,
            lock_label: None,
            current_yaw: 0.0,
            last_drag: None,
            pitch_target: None,
        }
    }
}

impl GlobeState {
    /// Neutral camera tilt (radians) — the default view the pitch eases back
    /// to when a follow-lock is released.
    pub const DEFAULT_PITCH: f64 = 0.35;

    /// Zoom (globe radius in px) limits for the scroll wheel. The lower bound
    /// keeps the sphere large enough that the ISS model and its label stay
    /// legible; the upper bound keeps a zoomed-in pane from pushing the orbit
    /// ring and panes' overlays far outside the clip rect at 4 panes.
    pub const ZOOM_MIN: f32 = 40.0;
    pub const ZOOM_MAX: f32 = 500.0;

    /// Timezone quick-jump button: lock the camera onto `(lon, lat)` and ease
    /// the pitch to that latitude. Clicking the SAME zone again toggles the
    /// lock off and eases the pitch back to `DEFAULT_PITCH` — the yaw is left
    /// alone so it continues from the current heading (see `unlock`).
    ///
    /// Returns `true` whenever the camera changed (either direction), so the
    /// caller can treat it as "the view jumped".
    pub fn toggle_zone(&mut self, label: &'static str, lon: f64, lat: f64) -> bool {
        if self.lock_label == Some(label) {
            self.unlock();
        } else {
            self.lock_lon = Some(lon);
            self.lock_lat = Some(lat);
            self.lock_label = Some(label);
            self.pitch_target = Some(lat.to_radians());
            // Keep `yaw` in sync with the locked heading so `current_yaw`
            // starts accumulating from the right value this frame.
            self.yaw = self.current_yaw;
            self.last_interaction = Some(std::time::Instant::now());
        }
        true
    }

    /// Release the follow-lock (toggle-off or manual drag): ease the pitch
    /// back to the default and keep the yaw exactly where the camera is
    /// looking right now — `current_yaw` is refreshed on every rendered frame
    /// (lock or not), so adopting it can never snap back to a stale heading.
    pub fn unlock(&mut self) {
        // While locked, `current_yaw` holds the yaw the last frame actually
        // rendered (== `lon + gmst - 90°` for that frame). Adopting it as the
        // free-camera yaw continues the exact same heading: the frame before
        // and after the unlock render at the same angle, so nothing jumps.
        self.yaw = self.current_yaw;
        self.lock_lon = None;
        self.lock_lat = None;
        self.lock_label = None;
        self.pitch_target = Some(Self::DEFAULT_PITCH);
        self.last_interaction = Some(std::time::Instant::now());
    }

    /// Per-frame pitch easing toward `pitch_target` (exponential glide,
    /// ~0.7 s at 60 fps). Cheap and stateless: the target lives in a field,
    /// the step is a fixed fraction of the remaining distance.
    fn settle_pitch(&mut self) {
        let Some(target) = self.pitch_target else { return };
        let k = 0.06_f64;
        self.pitch += (target - self.pitch) * k;
        if (target - self.pitch).abs() < 1e-3 {
            self.pitch = target;
            self.pitch_target = None; // settled — stop easing
        }
    }

    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
        self.last_interaction = Some(std::time::Instant::now());
        self.last_drag = self.last_interaction;
        self.pitch_target = None; // a drag cancels any pending pitch ease
        if self.lock_lon.is_some() {
            // Releasing the follow-lock: adopt the camera's actual heading so
            // the view doesn't snap back to the stale manual yaw. (The pitch
            // is deliberately left where the drag put it.)
            self.yaw = self.current_yaw;
        }
        self.lock_lon = None; // manual drag releases the follow-lock
        self.lock_lat = None;
        self.lock_label = None;
    }
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(Self::ZOOM_MIN, Self::ZOOM_MAX);
        self.last_interaction = Some(std::time::Instant::now());
    }
    /// Camera yaw: either locked onto `lock_lon` (region faces the viewer,
    /// drifting with the real Earth rotation) or free (the user's yaw,
    /// untouched until the user drags — see `auto_reset`).
    #[cfg(test)]
    pub(crate) fn effective_yaw_for_test(&self, gmst: f64) -> f64 {
        self.effective_yaw(gmst)
    }
    fn effective_yaw(&self, gmst: f64) -> f64 {
        if let Some(lon) = self.lock_lon {
            // Mesh places Earth-fixed lon L at world longitude (L + gmst).
            // rotate_to_cam maps world lon W to camera angle (W - yaw); the
            // camera sits on +z, which is world lon +90°. So the point faces
            // the viewer when (L + gmst) - yaw = 90°  ⇒  yaw = L + gmst - 90°.
            let lon_rot = lon.to_radians() + gmst;
            return lon_rot - std::f64::consts::FRAC_PI_2;
        }
        // With no follow-lock the camera yaw is simply the user's yaw: an
        // interactive drag moves it and nothing else does, so an untouched
        // pane holds its heading forever. A pane only ever eases back toward
        // the sun-facing view via `auto_reset`, which requires a previous
        // *manual drag* (see there) — never on its own.
        self.yaw
    }

    /// Auto-reset: eases the camera back to the sun-facing default view, but
    /// only after the user actually *dragged* and then stopped — and only on
    /// panes that are not follow-locked. `last_drag` tracks pointer drags
    /// exclusively (`toggle_zone` / `unlock` touch `last_interaction`, not
    /// this), so a pane that was never touched starts at `last_drag == None`
    /// and its idle gate never trips: it holds the user's heading exactly (see
    /// `effective_yaw`). Once a drag happened, 5 s of idleness glides yaw/pitch
    /// back to the sun-facing view (or the active lock's yaw).
    pub fn auto_reset(&mut self, base_yaw: f64) {
        const IDLE_RESET: f64 = 5.0;
        let idle = self
            .last_drag
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(f64::INFINITY);
        if idle < IDLE_RESET {
            return;
        }
        // Stay out of the way while a deliberate pitch ease is in flight
        // (`settle_pitch`): a zone jump centres on the zone's latitude and an
        // unlock eases to `DEFAULT_PITCH`. Both write `pitch` every frame, so
        // easing toward `DEFAULT_PITCH` here too would fight them — with a
        // fresh `GlobeState` (`last_drag == None`) the idle gate never trips,
        // so a zone jump's latitude centring could never actually settle.
        if self.pitch_target.is_some() {
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
    // 1) yaw about the polar axis (the camera orbits in longitude)
    let (cy, sy) = (yaw.cos(), yaw.sin());
    let x1 = v.0 * cy + v.2 * sy;
    let z1 = -v.0 * sy + v.2 * cy;
    // 2) pitch about the camera's own horizontal axis (camera latitude)
    let (cp, sp) = (pitch.cos(), pitch.sin());
    V3(x1 * r, (v.1 * cp - z1 * sp) * r, (v.1 * sp + z1 * cp) * r)
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
    // The subsolar point's world longitude is `atan2(sun_ef.z, sun_ef.x) +
    // earth_rot` (the mesh places Earth-fixed lon L at world lon L + GMST, and
    // `sun_dir_ef` is the Earth-fixed subsolar direction). With the new
    // yaw-then-pitch `rotate_to_cam` a point faces the viewer at
    // `yaw = world_lon - 90°`, hence:
    let sun_yaw = sun_dir_ef.2.atan2(sun_dir_ef.0) + earth_rot
        - std::f64::consts::FRAC_PI_2;
    // When the camera is follow-locked (e.g. a timezone jump), `auto_reset`
    // must NOT ease `yaw` toward the sun-facing yaw — the lock's yaw is
    // computed fresh in `effective_yaw` every frame, so easing toward
    // `sun_yaw` fights the lock and the two yaws alternate between frames
    // (that was the flickering lighting after "切到北京").
    let locked = cam.lock_lon.is_some();
    if !locked {
        cam.auto_reset(sun_yaw);
    }
    // Pitch easing runs in BOTH states: a zone jump eases toward the zone's
    // latitude, an unlock eases back to the default tilt.
    cam.settle_pitch();
    let yaw = cam.effective_yaw(earth_rot);
    // ALWAYS record the yaw actually rendered this frame, lock or not:
    // `current_yaw` means "the previous frame's rendered yaw", and freezing
    // it while locked made the toggle-off (unlock) snap back to the heading
    // the camera had at the moment the lock started.
    cam.current_yaw = yaw;
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
    let (lat_bands, lon_bands) = (GLOBE_LAT_BANDS, GLOBE_LON_BANDS);
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
    // PER-PANE mesh cache, keyed by the pane's egui LayerId. A single shared
    // slot made the 4 panes invalidate each other's cache every frame (each
    // pane has its own camera pose), so pane A's rebuild forced pane B to
    // replay a stale mesh with mismatched overlays — the persistent flicker.
    // Keying by LayerId gives every pane its own independent cache.
    thread_local! {
        static MESH_CACHE: std::cell::RefCell<Option<std::collections::HashMap<egui::LayerId, ((u64, u64, u32, u64, u64, u64, u64), egui::Mesh, std::time::Instant, (f64, f64))>>> =
            const { std::cell::RefCell::new(None) };
    }
    let mesh_layer = painter.layer_id();
    let dragging = cam.last_interaction
        .map(|t| now.duration_since(t).as_secs_f64() < DRAG_ACTIVE_SECS)
        .unwrap_or(false);
    // Which yaw/pitch the CACHED mesh was built with. While a rebuild is
    // throttled (replaying the stale mesh), everything else drawn this frame
    // (orbit ring, satellite, marker) must use the SAME yaw/pitch — mixing a
    // stale sphere with fresh overlays makes the terminator/geometry visibly
    // jump between frames (the "闪" after the timezone jump).
    let mut mesh_yaw = yaw;
    let mut mesh_pitch = pitch;
    let mesh_changed = MESH_CACHE
        .with(|c| {
            let mut b = c.borrow_mut();
            let m = b.get_or_insert_with(std::collections::HashMap::new);
            match m.get(&mesh_layer) {
                None => true,
                Some((k, _, built, (cy, cpth))) => {
                    mesh_yaw = *cy;
                    mesh_pitch = *cpth;
                    if *k != mesh_key {
                        // Throttle rebuilds: the sim clock advances every frame
                        // (earth_rot changes constantly), which would rebuild the
                        // full 10.5k-vertex sphere every frame per pane and can
                        // deadlock the AMD OpenGL driver under sustained load.
                        // Rebuild at most once per `MESH_REBUILD_INTERVAL_SECS` —
                        // unless the user is dragging, where latency matters.
                        dragging
                            || now.duration_since(*built).as_secs_f64()
                                > MESH_REBUILD_INTERVAL_SECS
                    } else {
                        false
                    }
                }
            }
        });
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
            let mut b = c.borrow_mut();
            let m = b.get_or_insert_with(std::collections::HashMap::new);
            m.insert(mesh_layer, (mesh_key, mesh.clone(), now, (yaw, pitch)));
            // Safety cap (layout toggles can rotate layer ids): rebuild from
            // scratch rather than grow unboundedly.
            if m.len() > 16 {
                m.clear();
                m.insert(mesh_layer, (mesh_key, mesh.clone(), now, (yaw, pitch)));
            }
        });
        painter.add(mesh);
    } else {
        MESH_CACHE.with(|c| {
            if let Some((_, m, _, _)) = c
                .borrow()
                .as_ref()
                .and_then(|map| map.get(&mesh_layer))
            {
                painter.add(m.clone());
            }
        });
    }

    // Subtle atmosphere terminator glow on the night side edge.
    // (skip — the vertex shading handles it)

    // While a mesh rebuild is throttled, the sphere on screen corresponds to
    // the cached camera pose. Draw every overlay (orbit, satellite, marker)
    // against THAT pose so the whole frame is internally consistent —
    // otherwise the overlays lead the globe and the terminator flickers.
    let render_yaw = if mesh_changed { yaw } else { mesh_yaw };
    let render_pitch = if mesh_changed { pitch } else { mesh_pitch };

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
    // Collect consecutive visible points into runs, then draw each run as a
    // single smooth polyline (one shape = uniform joints, no dotted look).
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    for p in orbit_eci {
        let v = eci_to_n(p);
        let alt = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 6371.0;
        // Modest exaggeration: hugs the globe visually while LEO orbits still
        // clear the surface (displayed km values stay true).
        let alt_r = (r as f64) * (1.0 + alt / 6371.0 * 0.4);
        let cam_v = rotate_to_cam(v, alt_r, render_yaw, render_pitch);
        let cur = project(cam_v, center);
        // Occlusion: a point is hidden when it's on the far side (z < 0) AND
        // its projection lands inside the globe disc.
        let visible = cur.1 > 0.0 || cur.0.distance(center) > r;
        if visible {
            // Continue the run in progress, or start one on the first point.
            match runs.last_mut() {
                Some(run) => run.push(cur.0),
                None => runs.push(vec![cur.0]),
            }
        } else {
            runs.push(Vec::new()); // break the polyline at the globe's edge
        }
    }
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
        let cam_v = rotate_to_cam(n, alt_r, render_yaw, render_pitch);
        let (sp, z) = project(cam_v, center);
        let behind = z < 0.0 && sp.distance(center) < r;
        if !behind {
            draw_spacecraft(
                &painter,
                sp,
                center,
                sat,
                color,
                scale as f32,
                render_yaw,
                render_pitch,
                now.elapsed().as_secs_f32(),
                sun_world,
            );
            painter.text(
                sp + Vec2::new(14.0 * scale as f32, -12.0 * scale as f32),
                egui::Align2::LEFT_BOTTOM,
                format!("{} · {} km", sat.name, p.alt_km as i32),
                egui::FontId::proportional((11.0 * scale as f32).max(9.0)),
                Color32::WHITE,
            );
        }
    }

    // Locked-location marker (timezone jump): a crosshair + label so the
    // viewer can see exactly where the locked point is on the globe.
    if let (Some(lon), Some(lat), Some(label)) = (cam.lock_lon, cam.lock_lat, cam.lock_label) {
        let (la_r, lo_r) = (lat.to_radians(), (lon + earth_rot.to_degrees()).to_radians());
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        let cam_v = rotate_to_cam(n, r as f64 * 1.002, render_yaw, render_pitch);
        let (mp, z) = project(cam_v, center);
        let behind = z < 0.0 && mp.distance(center) < r;
        if !behind {
            let cross = 6.0 * scale as f32;
            let col = Color32::from_rgb(255, 210, 80);
            painter.line_segment(
                [mp - Vec2::new(cross, 0.0), mp + Vec2::new(cross, 0.0)],
                Stroke::new(1.5, col),
            );
            painter.line_segment(
                [mp - Vec2::new(0.0, cross), mp + Vec2::new(0.0, cross)],
                Stroke::new(1.5, col),
            );
            painter.circle_stroke(mp, 9.0 * scale as f32, Stroke::new(1.2, col));
            painter.text(
                mp + Vec2::new(12.0 * scale as f32, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional((10.0 * scale as f32).max(9.0)),
                col,
            );
        }
    }
}

/// Draw the focused spacecraft: a flat-shaded 3D wireframe/solid model of
/// the station (real ISS geometry: long truss + 4 solar panel pairs + module
/// bodies), projected with the same camera (yaw/pitch) as the globe and
/// slowly rotating around its own truss axis. Falls back to the vector
/// pictogram for non-station satellites.
fn draw_spacecraft(
    painter: &Painter,
    sp: Pos2,
    center: Pos2,
    sat: &Sat,
    color: Color32,
    scale: f32,
    yaw: f64,
    pitch: f64,
    t: f32,
    sun_world: V3,
) {
    let upper = sat.name.to_uppercase();
    if upper.contains("ISS")
        || upper.contains("ZARYA")
        || upper.contains("CSS")
        || upper.contains("TIANGONG")
        || upper.contains("TIANHE")
        || upper.contains("MIR")
    {
        draw_iss_model(painter, sp, yaw, pitch, t, scale, sun_world);
        return;
    }
    if upper.contains("HST") || upper.contains("HUBBLE") {
        draw_hubble_model(painter, sp, yaw, pitch, t, scale);
        return;
    }
    draw_satellite_model(painter, sp, center, color, scale);
}

/// Real ISS geometry baked from the public SpaceX ISS docking simulator model
/// (github.com/matthewgiarra/spacex-iss-sim, `iss_mobile.glb`): full truss,
/// 8 solar array wings, pressurized modules. Baked to a compact binary mesh
/// with Python (scene-graph transformed, area-weighted decimated to 28k tris).
/// Binary layout: 24-byte header [f32 cx, cy, cz; f32 ext; u32 nverts, ntris],
/// then nverts × f32×3 (pre-centered, divided by ext), then ntris ×
/// [u32 a, b, c + u8 r, g, b].
const ISS_MODEL_BIN: &[u8] = include_bytes!("../../../assets/iss_model_nasa.bin");

struct IssMesh {
    verts: Vec<[f32; 3]>,
    tris: Vec<([u32; 3], [u8; 3])>,
}

/// Parse the baked binary once and cache it for every frame.
fn iss_mesh() -> &'static IssMesh {
    static MESH: std::sync::OnceLock<IssMesh> = std::sync::OnceLock::new();
    MESH.get_or_init(|| {
        let rd = |off: usize| -> f32 {
            f32::from_le_bytes(ISS_MODEL_BIN[off..off + 4].try_into().unwrap())
        };
        let ru = |off: usize| -> u32 {
            u32::from_le_bytes(ISS_MODEL_BIN[off..off + 4].try_into().unwrap())
        };
        let nverts = ru(16) as usize;
        let ntris = ru(20) as usize;
        let vbase = 24;
        let mut verts = Vec::with_capacity(nverts);
        for i in 0..nverts {
            let o = vbase + i * 12;
            verts.push([rd(o), rd(o + 4), rd(o + 8)]);
        }
        let tbase = vbase + nverts * 12;
        let mut tris = Vec::with_capacity(ntris);
        for i in 0..ntris {
            let o = tbase + i * 15;
            tris.push((
                [ru(o), ru(o + 4), ru(o + 8)],
                [ISS_MODEL_BIN[o + 12], ISS_MODEL_BIN[o + 13], ISS_MODEL_BIN[o + 14]],
            ));
        }
        IssMesh { verts, tris }
    })
}

/// Draw the ISS as a rotating flat-shaded 3D model from the baked mesh.
/// Lighting: sun-direction Lambert from the same `sun_world` used for the
/// globe, so the station shows a real day/night side like the Earth.
fn draw_iss_model(
    painter: &Painter,
    sp: Pos2,
    yaw: f64,
    pitch: f64,
    t: f32,
    scale: f32,
    sun_world: V3,
) {
    let mesh_data = iss_mesh();
    // Model rotation: slow spin around the truss axis (Z) + fixed tilt so
    // the panels read at an angle.
    let spin = (t * 0.35) as f64;
    let (cs, sn) = spin.sin_cos();
    let tilt = 0.5_f64;
    let (ct, st) = tilt.sin_cos();

    // Camera basis (same convention as rotate_to_cam: camera looks +z).
    let (cp, spn) = (pitch.cos(), pitch.sin());
    let (cy, sy) = (yaw.cos(), yaw.sin());

    let model_size = (46.0 * scale) as f64;
    // Verts are normalized to [-0.5, 0.5] (divided by extent at bake time),
    // so the full span is 1 unit → scale directly by model_size.
    let s = model_size;

    // Transform all vertices once per frame (spin+tilt, then camera yaw/pitch).
    // Keep the spin+tilt-space 3D coords so per-tri normals can be lit by the
    // real sun direction.
    let rot3: Vec<[f64; 3]> = mesh_data
        .verts
        .iter()
        .map(|v| {
            // spin around Z
            let (x, y) = (
                v[0] as f64 * cs - v[1] as f64 * sn,
                v[0] as f64 * sn + v[1] as f64 * cs,
            );
            // fixed tilt around X
            let y2 = y * ct - v[2] as f64 * st;
            let z2 = y * st + v[2] as f64 * ct;
            [x, y2, z2]
        })
        .collect();

    let proj: Vec<[f32; 2]> = rot3
        .iter()
        .map(|v| {
            // camera transform (yaw around Y, then pitch around X) — same as globe
            let y3 = v[1] * cp - v[2] * spn;
            let z3 = v[1] * spn + v[2] * cp;
            let x4 = v[0] * cy + z3 * sy;
            [sp.x + (x4 * s) as f32, sp.y - (y3 * s) as f32]
        })
        .collect();

    // Inverse camera rotation (camera space → world) for normals, and the
    // normalized sun direction in the world frame.
    let sun_len = (sun_world.0 * sun_world.0
        + sun_world.1 * sun_world.1
        + sun_world.2 * sun_world.2)
        .sqrt()
        .max(1e-9);
    let (sux, suy, suz) = (sun_world.0 / sun_len, sun_world.1 / sun_len, sun_world.2 / sun_len);

    // Backface culling: skip degenerate/wound-away triangles. Depth: use the
    // rotated model z (post spin+tilt) averaged per tri, painted far first.
    let rotated_z: Vec<f32> = mesh_data
        .verts
        .iter()
        .map(|v| {
            (v[0] as f64 * sn + v[1] as f64 * cs) * st + v[2] as f64 * ct
        } as f32)
        .collect();

    let nz_of = |tri: &[u32; 3]| -> f32 {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let e1 = [proj[b][0] - proj[a][0], proj[b][1] - proj[a][1]];
        let e2 = [proj[c][0] - proj[a][0], proj[c][1] - proj[a][1]];
        e1[0] * e2[1] - e1[1] * e2[0]
    };

    let mut order: Vec<usize> = (0..mesh_data.tris.len()).collect();
    order.retain(|&i| nz_of(&mesh_data.tris[i].0) > 0.0);
    order.sort_by(|&a, &b| {
        let da = rotated_z[mesh_data.tris[a].0[0] as usize];
        let db = rotated_z[mesh_data.tris[b].0[0] as usize];
        db.partial_cmp(&da).unwrap()
    });

    // Build one egui mesh: 3 fresh vertices per triangle (flat shading).
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(order.len() * 3);
    mesh.indices.reserve(order.len() * 3);
    for &ti in &order {
        let (idx, base) = &mesh_data.tris[ti];
        let nz = nz_of(idx) as f64;
        // Real sun Lambert: world-space normal from the spin+tilt coords,
        // then through the camera rotation (pitch then yaw, matching
        // rotate_to_cam), dotted with the sun direction. Plus a mild
        // screen-space term so geometry stays readable on the night side.
        let (a, b, c) = (
            rot3[idx[0] as usize],
            rot3[idx[1] as usize],
            rot3[idx[2] as usize],
        );
        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        // world normal = cross(e1, e2) pushed through camera rotation
        let nx0 = e1[1] * e2[2] - e1[2] * e2[1];
        let ny0 = e1[2] * e2[0] - e1[0] * e2[2];
        let nz0 = e1[0] * e2[1] - e1[1] * e2[0];
        // pitch around X
        let ny1 = ny0 * cp - nz0 * spn;
        let nz1 = ny0 * spn + nz0 * cp;
        // yaw around Y
        let nx = nx0 * cy + nz1 * sy;
        let d = (nx * sux + ny1 * suy + nz1 * suz).clamp(-1.0, 1.0);
        let facing = (nz / (nz * nz + 1.0).sqrt()).min(1.0) as f64;
        let shade = if d > 0.0 {
            (0.35 + 0.65 * d) * (0.75 + 0.25 * facing)
        } else {
            // night side: dim but not black, keep a hint of the silhouette
            0.18 + 0.10 * facing
        };
        let shade = shade as f32;
        // Whitewash: mix the baked color toward white so the module/panel
        // geometry stays readable even on the night side.
        let mix = ISS_NIGHT_WHITEWASH;
        let col = Color32::from_rgb(
            (base[0] as f32 * shade * (1.0 - mix) + 255.0 * mix * shade) as u8,
            (base[1] as f32 * shade * (1.0 - mix) + 255.0 * mix * shade) as u8,
            (base[2] as f32 * shade * (1.0 - mix) + 255.0 * mix * shade) as u8,
        );
        let base_vi = mesh.vertices.len() as u32;
        for &i in idx.iter() {
            let p = proj[i as usize];
            mesh.colored_vertex(egui::pos2(p[0], p[1]), col);
        }
        mesh.indices
            .extend_from_slice(&[base_vi, base_vi + 1, base_vi + 2]);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw Hubble as a simple rotating 3D cylinder-ish model (tube + solar
/// panels + aperture door).
fn draw_hubble_model(painter: &Painter, sp: Pos2, yaw: f64, pitch: f64, t: f32, scale: f32) {
    let spin = (t * 0.3) as f64;
    let (cs, sn) = spin.sin_cos();
    let (cp, spn) = (pitch.cos(), pitch.sin());
    let (cy, sy) = (yaw.cos(), yaw.sin());
    let s = (30.0 * scale / 2.0) as f64;

    // Cylinder along Z: ring of 10 points at both ends + end caps.
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let n = 10;
    for k in 0..n {
        let a = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
        verts.push([a.cos() * 0.35, a.sin() * 0.35, -1.0]);
    }
    for k in 0..n {
        let a = 2.0 * std::f64::consts::PI * (k as f64) / (n as f64);
        verts.push([a.cos() * 0.35, a.sin() * 0.35, 1.0]);
    }
    // solar panels at one end
    verts.push([-0.9, -0.15, 1.0]); // 20
    verts.push([0.9, -0.15, 1.0]); // 21
    verts.push([0.9, 0.15, 1.0]); // 22
    verts.push([-0.9, 0.15, 1.0]); // 23

    let mut proj = Vec::new();
    let mut depth = Vec::new();
    for v in &verts {
        let (x, y) = (v[0] * cs - v[1] * sn, v[0] * sn + v[1] * cs);
        let y2 = y * cp - v[2] * spn;
        let z2 = y * spn + v[2] * cp;
        let x3 = x * cy + z2 * sy;
        let z3 = -x * sy + z2 * cy;
        proj.push([sp.x + (x3 * s) as f32, sp.y - (y2 * s) as f32]);
        depth.push(z3 as f32);
    }

    let mut faces: Vec<(Vec<usize>, [u8; 3])> = Vec::new();
    for k in 0..n {
        let k2 = (k + 1) % n;
        faces.push((vec![k, k2, n + k2, n + k], [200, 205, 215]));
    }
    faces.push((vec![20, 21, 22, 23], [40, 70, 170]));

    let mut order: Vec<usize> = (0..faces.len()).collect();
    order.sort_by(|&a, &b| {
        let da: f32 = faces[a].0.iter().map(|&i| depth[i]).sum::<f32>() / faces[a].0.len() as f32;
        let db: f32 = faces[b].0.iter().map(|&i| depth[i]).sum::<f32>() / faces[b].0.len() as f32;
        da.partial_cmp(&db).unwrap()
    });
    for &fi in &order {
        let (idx, base) = &faces[fi];
        let pts: Vec<egui::Pos2> = idx
            .iter()
            .map(|&i| egui::pos2(proj[i][0], proj[i][1]))
            .collect();
        let e1 = [pts[1].x - pts[0].x, pts[1].y - pts[0].y];
        let e2 = [pts[2].x - pts[0].x, pts[2].y - pts[0].y];
        if e1[0] * e2[1] - e1[1] * e2[0] <= 0.0 {
            continue;
        }
        painter.add(egui::Shape::convex_polygon(
            pts,
            Color32::from_rgb(base[0], base[1], base[2]),
            Stroke::new(0.5 * scale, Color32::from_rgb(30, 34, 44)),
        ));
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
