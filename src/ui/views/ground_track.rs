//! Ground track view for one focused satellite.

use crate::orbit::Propagator;
use crate::data::model::Sat;
use chrono::{DateTime, Utc};
use egui::{Color32, Painter, Rect};

pub fn show_ground_track(
    painter: &Painter,
    rect: Rect,
    sat: &Sat,
    prop: &Propagator,
    sim_time: chrono::DateTime<chrono::Utc>,
) {
    let now = sim_time;
    let track = prop.ground_track(sat, now, 45.0, 90.0, 2.0);

    // Grid backdrop (reuse world map helpers).
    super::world_map::draw_grid_public(painter, rect);

    let color = sat.group.color();
    let mut prev: Option<egui::Pos2> = None;
    for p in &track {
        if let Some(spot) = super::world_map::project_public(rect, p.lat_deg, p.lon_deg) {
            if let Some(a) = prev {
                if (spot.x - a.x).abs() < rect.width() / 2.0 {
                    painter.line_segment([a, spot], egui::Stroke::new(2.0, color));
                }
            }
            prev = Some(spot);
        } else {
            prev = None;
        }
    }

    // Current position marker.
    if let Some(p) = prop.subpoint(sat, now) {
        if let Some(spot) = super::world_map::project_public(rect, p.lat_deg, p.lon_deg) {
            painter.circle_filled(spot, 5.0, color);
            painter.circle_stroke(spot, 9.0, egui::Stroke::new(1.5, Color32::WHITE));
            painter.text(
                spot + egui::Vec2::new(12.0, -12.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{} ({} km)", sat.name, p.alt_km as i32),
                egui::FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
    }
}
