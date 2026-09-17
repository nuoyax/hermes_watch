//! World map view: equirectangular projection with live satellite positions.

use crate::orbit::GeoPoint;
use crate::data::model::Sat;
use egui::{Color32, Painter, Pos2, Rect};

/// Render the world map view into `rect`.
pub fn show_world_map(
    painter: &Painter,
    rect: Rect,
    sats: &[Sat],
    positions: &[Option<GeoPoint>],
    groups_enabled: &std::collections::HashSet<crate::data::model::SatGroup>,
) {
    draw_grid(painter, rect);
    draw_continents(painter, rect);

    for (sat, pos) in sats.iter().zip(positions.iter()) {
        if !groups_enabled.contains(&sat.group) {
            continue;
        }
        let Some(p) = pos else { continue };
        let Some(spot) = project(rect, p.lat_deg, p.lon_deg) else {
            continue;
        };
        painter.circle_filled(spot, 3.0, sat.group.color());
    }
}

/// Map lat/lon (deg) to pixel coords; None if outside the visible wrap.
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
    for lon in [-180, -120, -60, 0, 60, 120, 180] {
        if let Some(a) = project(rect, 90.0, lon as f64) {
            if let Some(b) = project(rect, -90.0, lon as f64) {
                painter.line_segment([a, b], egui::Stroke::new(1.0, c));
            }
        }
    }
    for lat in [-60, -30, 0, 30, 60] {
        if let Some(a) = project(rect, lat as f64, -180.0) {
            if let Some(b) = project(rect, lat as f64, 180.0) {
                let stroke = if lat == 0 {
                    egui::Stroke::new(1.5, Color32::from_rgb(60, 70, 90))
                } else {
                    egui::Stroke::new(1.0, c)
                };
                painter.line_segment([a, b], stroke);
            }
        }
    }
}

/// Very coarse continent outlines (lat, lon polylines) for orientation.
const CONTINENTS: &[&[(f64, f64)]] = &[
    // North America (rough)
    &[(70.0, -160.0), (60.0, -140.0), (48.0, -125.0), (30.0, -115.0), (23.0, -110.0), (18.0, -95.0), (25.0, -80.0), (40.0, -70.0), (47.0, -55.0), (60.0, -65.0), (70.0, -80.0), (70.0, -160.0)],
    // South America
    &[(12.0, -72.0), (0.0, -80.0), (-15.0, -75.0), (-35.0, -72.0), (-55.0, -68.0), (-50.0, -65.0), (-20.0, -40.0), (-5.0, -35.0), (5.0, -50.0), (12.0, -72.0)],
    // Europe + Asia (rough)
    &[(36.0, -10.0), (43.0, 5.0), (55.0, 10.0), (60.0, 25.0), (70.0, 30.0), (75.0, 60.0), (70.0, 100.0), (65.0, 140.0), (60.0, 160.0), (55.0, 160.0), (45.0, 135.0), (30.0, 122.0), (20.0, 110.0), (10.0, 105.0), (8.0, 98.0), (22.0, 88.0), (15.0, 73.0), (22.0, 60.0), (25.0, 55.0), (38.0, 48.0), (36.0, 20.0), (36.0, -10.0)],
    // Africa
    &[(35.0, -5.0), (32.0, 22.0), (30.0, 33.0), (12.0, 43.0), (0.0, 42.0), (-10.0, 40.0), (-25.0, 33.0), (-34.0, 20.0), (-25.0, 15.0), (-10.0, 13.0), (4.0, 9.0), (5.0, -5.0), (10.0, -15.0), (20.0, -17.0), (28.0, -13.0), (35.0, -5.0)],
    // Australia
    &[(-12.0, 131.0), (-18.0, 122.0), (-33.0, 115.0), (-38.0, 145.0), (-28.0, 153.0), (-20.0, 149.0), (-12.0, 131.0)],
    // Greenland
    &[(60.0, -45.0), (70.0, -55.0), (78.0, -35.0), (70.0, -20.0), (60.0, -45.0)],
];

fn draw_continents(painter: &Painter, rect: Rect) {
    let stroke = egui::Stroke::new(1.2, Color32::from_rgb(90, 100, 115));
    let fill = Color32::from_rgb(50, 58, 70);
    for poly in CONTINENTS {
        let pts: Vec<Pos2> = poly
            .iter()
            .filter_map(|(lat, lon)| project(rect, *lat, *lon))
            .collect();
        if pts.len() > 2 {
            painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
        } else if pts.len() > 1 {
            painter.line_segment([pts[0], pts[1]], stroke);
        }
    }
}
