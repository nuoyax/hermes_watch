//! Satellite detail (telemetry) view.

use crate::data::model::Sat;
use crate::orbit::Propagator;
use chrono::Utc;
use egui::Color32;

pub fn show_detail(ui: &mut egui::Ui, sat: &Sat, prop: &Propagator) {
    ui.heading(&sat.name);
    ui.monospace(format!("NORAD  #{}", sat.norad_id));
    ui.monospace(format!("Group   {}", sat.group.label()));
    ui.separator();

    let now = Utc::now();
    if let Some(p) = prop.subpoint(sat, now) {
        kv_row(ui, "Latitude", format!("{:.3}°", p.lat_deg));
        kv_row(ui, "Longitude", format!("{:.3}°", p.lon_deg));
        kv_row(ui, "Altitude", format!("{:.1} km", p.alt_km));
        kv_row(ui, "Speed", format!("{:.2} km/s", orbital_speed(p.alt_km)));
    } else {
        ui.colored_label(Color32::RED, "Propagation failed for this TLE");
    }

    ui.separator();
    if let Some((y, d)) = sat.tle.epoch() {
        kv_row(ui, "TLE epoch", format!("{} day {:.4}", y, d));
    }
    ui.collapsing("TLE lines", |ui| {
        ui.monospace(&sat.tle.line1);
        ui.monospace(&sat.tle.line2);
    });
}

fn kv_row(ui: &mut egui::Ui, k: &str, v: String) {
    ui.horizontal(|ui| {
        ui.strong(format!("{:<10}", k));
        ui.monospace(v);
    });
}

/// Approx orbital speed from altitude (vis-viva, circular assumption).
fn orbital_speed(alt_km: f64) -> f64 {
    const MU: f64 = 398_600.4418; // km^3/s^2
    const RE: f64 = 6371.0;
    (MU / (RE + alt_km)).sqrt()
}
