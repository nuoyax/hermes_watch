//! 3D globe view: draggable/zoomable earth with live satellites and orbit rings.

use crate::data::model::Sat;
use crate::orbit::GeoPoint;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

/// Per-pane camera state for the globe.
#[derive(Debug, Clone, Copy)]
pub struct GlobeState {
    /// Rotation around the polar axis (radians).
    pub yaw: f64,
    /// Tilt toward/away from the viewer (radians).
    pub pitch: f64,
    /// Globe radius in pixels.
    pub zoom: f32,
}

impl Default for GlobeState {
    fn default() -> Self {
        Self {
            yaw: -1.2,
            pitch: 0.45,
            zoom: 100.0,
        }
    }
}

impl GlobeState {
    pub fn drag(&mut self, delta: Vec2) {
        self.yaw += delta.x as f64 * 0.01;
        self.pitch = (self.pitch + delta.y as f64 * 0.01).clamp(-1.5, 1.5);
    }
    pub fn zoom(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(40.0, 400.0);
    }
}

#[derive(Debug, Clone, Copy)]
struct V3(f64, f64, f64);

/// lat/lon (deg) + radius → camera-space 3D point.
/// Camera-space z > 0 means "toward viewer" (near hemisphere).
fn to_camera(lat_deg: f64, lon_deg: f64, r: f64, cam: &GlobeState) -> V3 {
    let (lat, lon) = (lat_deg.to_radians(), lon_deg.to_radians());
    // Earth-fixed basis: x = cos lat cos lon, y = sin lat (north up), z = cos lat sin lon
    let x = lat.cos() * lon.cos();
    let y = lat.sin();
    let z = lat.cos() * lon.sin();
    // Pitch: rotate around x-axis (tilt poles toward/away viewer).
    let (cp, sp) = (cam.pitch.cos(), cam.pitch.sin());
    let y2 = y * cp - z * sp;
    let z2 = y * sp + z * cp;
    // Yaw: rotate around y-axis (spin the globe).
    let (cy, sy) = (cam.yaw.cos(), cam.yaw.sin());
    let x3 = x * cy + z2 * sy;
    let z3 = -x * sy + z2 * cy;
    V3(x3 * r, y2 * r, z3 * r)
}

fn project(v: V3, center: Pos2) -> (Pos2, f64) {
    (Pos2::new(center.x + v.0 as f32, center.y - v.1 as f32), v.2)
}

/// Render the 3D globe. `positions` are live sub-satellite points aligned with `sats`;
/// `focus_orbit` is the precomputed orbit ring of the focused satellite.
pub fn show_globe(
    painter: &Painter,
    _rect: Rect,
    cam: &GlobeState,
    stars: &crate::ui::stars::StarField,
    sats: &[Sat],
    positions: &[Option<GeoPoint>],
    focus: Option<&Sat>,
    focus_orbit: &[GeoPoint],
    groups_enabled: &std::collections::HashSet<crate::data::model::SatGroup>,
) {
    let center = _rect.center();
    let r = cam.zoom;

    // Real star field (HYG catalog), rotating with the camera, behind everything.
    stars.paint(painter, center, (r * 2.2).max(120.0), cam.yaw, cam.pitch);

    // Ocean disc + atmosphere limb.
    painter.circle_filled(center, r + 4.0, Color32::from_rgba_unmultiplied(90, 140, 220, 60));
    painter.circle_filled(center, r + 1.0, Color32::from_rgb(80, 120, 190));
    painter.circle_filled(center, r, Color32::from_rgb(25, 40, 70));

    // Graticule + real coastlines.
    let grid = Color32::from_rgb(60, 80, 110);
    for lat in [-60.0, -30.0, 0.0, 30.0, 60.0] {
        ring(painter, center, cam, r, &|_t| (lat, t_fix(_t)), grid, 1.0);
    }
    for lon in (-180..180).step_by(30) {
        ring(painter, center, cam, r, &|t| (t, lon as f64), grid, 1.0);
    }

    // Real Natural Earth coastlines (embedded).
    let coast = Color32::from_rgb(110, 165, 110);
    for poly in super::super::earth::coastlines() {
        let mut prev: Option<(Pos2, f64)> = None;
        for (lat, lon) in poly {
            let cur = project(to_camera(*lat, *lon, r as f64, cam), center);
            if let Some((a, az)) = prev {
                seg(painter, a, az, cur.0, cur.1, coast, 1.4);
            }
            prev = Some(cur);
        }
    }

    // Focus orbit ring — brighter, drawn under satellite dots, with glow.
    if let Some(sat) = focus {
        let color = sat.group.color();
        let mut prev: Option<(Pos2, f64)> = None;
        for p in focus_orbit {
            let alt_r = (r as f64) + p.alt_km * ((r as f64) / 6371.0) * 0.35;
            let cur = project(to_camera(p.lat_deg, p.lon_deg, alt_r, cam), center);
            if let Some((a, az)) = prev {
                // Soft glow underneath + crisp bright line on top.
                seg(painter, a, az, cur.0, cur.1, blend(color, 0.25), 4.5);
                seg(painter, a, az, cur.0, cur.1, blend(color, 0.95), 1.8);
            }
            prev = Some(cur);
        }
    }

    // Unfocused satellites: all orbit tracks stay faint (drawn only when focused
    // via `focus_orbit`); here just the position dots, back-to-front.
    let mut pts: Vec<(f64, Pos2, Color32, bool)> = Vec::new();
    for (sat, pos) in sats.iter().zip(positions.iter()) {
        if !groups_enabled.contains(&sat.group) {
            continue;
        }
        if let Some(p) = pos {
            let alt_r = (r as f64) + p.alt_km * ((r as f64) / 6371.0) * 0.35;
            let (sp, z) = project(to_camera(p.lat_deg, p.lon_deg, alt_r, cam), center);
            let is_focus = focus.is_some_and(|f| f.norad_id == sat.norad_id);
            pts.push((z, sp, sat.group.color(), is_focus));
        }
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (z, sp, color, is_focus) in pts {
        let behind = z < 0.0 && sp.distance(center) < r;
        if behind {
            continue;
        }
        if is_focus {
            // Focused satellite: glowing halo + white outline to stand out.
            painter.circle_filled(sp, 7.0, blend(color, 0.30));
            painter.circle_filled(sp, 4.0, color);
            painter.circle_stroke(sp, 5.5, Stroke::new(1.5, Color32::WHITE));
        } else {
            let c = if z > 0.0 { blend(color, 0.85) } else { blend(color, 0.35) };
            painter.circle_filled(sp, 2.5, c);
        }
    }
}

fn t_fix(t: f64) -> f64 {
    t
}

/// Draw a parametric ring over the full lon range.
fn ring(
    painter: &Painter,
    center: Pos2,
    cam: &GlobeState,
    radius: f32,
    f: &impl Fn(f64) -> (f64, f64),
    color: Color32,
    width: f32,
) {
    let mut prev: Option<(Pos2, f64)> = None;
    let steps = 90;
    for i in 0..=steps {
        let t = -180.0 + 360.0 * i as f64 / steps as f64;
        let (lat, lon) = f(t);
        let cur = project(to_camera(lat, lon, radius as f64, cam), center);
        if let Some((a, az)) = prev {
            seg(painter, a, az, cur.0, cur.1, color, width);
        }
        prev = Some(cur);
    }
}

/// Draw a segment only where both endpoints are on the near hemisphere.
fn seg(painter: &Painter, a: Pos2, az: f64, b: Pos2, bz: f64, color: Color32, w: f32) {
    if az > 0.0 && bz > 0.0 {
        painter.line_segment([a, b], Stroke::new(w, color));
    } else if az > -0.2 || bz > -0.2 {
        painter.line_segment([a, b], Stroke::new(w, blend(color, 0.35)));
    }
}

fn blend(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)
}
