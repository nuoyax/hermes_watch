//! World map (2D equirectangular) view for ONE satellite: coastline earth,
//! graticule labels, satellite position with label.

use crate::data::model::Sat;
use crate::orbit::GeoPoint;
use egui::{Color32, Painter, Pos2, Rect, Stroke};

use crate::ui::earth;

/// Render the world map view focused on one satellite.
pub fn show_world_map(
    painter: &Painter,
    rect: Rect,
    sat: Option<&Sat>,
) -> Option<GeoPoint> {
    // Ocean background.
    painter.rect_filled(rect, 2.0, Color32::from_rgb(18, 28, 48));
    draw_grid(painter, rect);
    draw_coastlines(painter, rect);
    sat.map(|_| ())?;
    None // caller should use show_world_map_full when a satellite is focused
}

/// Full version with propagation — kept separate to avoid orbit dep cycles here.
pub fn show_world_map_full(
    painter: &Painter,
    rect: Rect,
    sat: &Sat,
    pos: &GeoPoint,
) -> Option<GeoPoint> {
    painter.rect_filled(rect, 2.0, Color32::from_rgb(18, 28, 48));
    draw_grid(painter, rect);
    draw_coastlines(painter, rect);

    let Some(spot) = project(rect, pos.lat_deg, pos.lon_deg) else {
        return Some(*pos);
    };
    let color = sat.group.color();
    // Glowing marker with white outline.
    painter.circle_filled(spot, 9.0, blend(color, 0.30));
    painter.circle_filled(spot, 4.5, color);
    painter.circle_stroke(spot, 6.0, Stroke::new(1.5, Color32::WHITE));
    painter.text(
        spot + egui::Vec2::new(10.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{}\n{} km", sat.name, pos.alt_km as i32),
        egui::FontId::proportional(11.0),
        Color32::WHITE,
    );
    Some(*pos)
}

/// Map lat/lon (deg) to pixel coords.
pub fn project(rect: Rect, lat: f64, lon: f64) -> Option<Pos2> {
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let x = rect.min.x as f64 + (lon + 180.0) / 360.0 * rect.width() as f64;
    let y = rect.min.y as f64 + (90.0 - lat) / 180.0 * rect.height() as f64;
    Some(Pos2::new(x as f32, y as f32))
}

/// Grid drawing, public for sibling views (ground_track).
pub fn draw_grid_public(painter: &Painter, rect: Rect) {
    draw_grid(painter, rect);
}

/// Public re-export of projection for sibling views.
pub fn project_public(rect: Rect, lat: f64, lon: f64) -> Option<Pos2> {
    project(rect, lat, lon)
}

fn draw_grid(painter: &Painter, rect: Rect) {
    let c = Color32::from_rgb(45, 50, 60);
    let label_c = Color32::from_rgb(110, 115, 125);
    for lon in [-180, -120, -60, 0, 60, 120, 180] {
        if let Some(a) = project(rect, 90.0, lon as f64) {
            if let Some(b) = project(rect, -90.0, lon as f64) {
                painter.line_segment([a, b], Stroke::new(1.0, c));
            }
        }
        if lon != -180 && lon != 180 {
            let label = if lon == 0 { "0°".to_string() } else { format!("{lon}°") };
            if let Some(p) = project(rect, -84.0, lon as f64) {
                painter.text(
                    p,
                    egui::Align2::CENTER_TOP,
                    label,
                    egui::FontId::proportional(9.0),
                    label_c,
                );
            }
        }
    }
    for lat in [-60, -30, 0, 30, 60] {
        if let Some(a) = project(rect, lat as f64, -180.0) {
            if let Some(b) = project(rect, lat as f64, 180.0) {
                let stroke = if lat == 0 {
                    Stroke::new(1.5, Color32::from_rgb(60, 70, 90))
                } else {
                    Stroke::new(1.0, c)
                };
                painter.line_segment([a, b], stroke);
            }
        }
        let label = if lat == 0 { "Eq".to_string() } else { format!("{lat}°") };
        if let Some(p) = project(rect, lat as f64, -176.0) {
            painter.text(
                p,
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(9.0),
                label_c,
            );
        }
    }
}

/// Real coastlines from embedded Natural Earth 110m data.
fn draw_coastlines(painter: &Painter, rect: Rect) {
    let stroke = Stroke::new(1.1, Color32::from_rgb(120, 150, 120));
    for poly in earth::coastlines() {
        let mut prev: Option<Pos2> = None;
        for (lat, lon) in poly {
            let cur = project(rect, *lat, *lon);
            if let (Some(a), Some(b)) = (prev, cur) {
                // Skip segments that wrap the antimeridian.
                if (b.x - a.x).abs() < rect.width() / 2.0 {
                    painter.line_segment([a, b], stroke);
                }
            }
            prev = cur;
        }
    }
}

fn blend(c: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * alpha) as u8)
}
