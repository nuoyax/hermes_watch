//! Satellite data model.

use serde::{Deserialize, Serialize};

/// A satellite with TLE orbital elements and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sat {
    /// NORAD catalog number (unique id).
    pub norad_id: u32,
    /// Common name, e.g. "ISS (ZARYA)".
    pub name: String,
    /// Group/category the satellite belongs to (station, weather, gps...).
    pub group: SatGroup,
    /// Epoch of the TLE (ISO-ish string from line 1).
    pub tle: Tle,
}

/// Two-line element set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tle {
    /// Line 1 raw text.
    pub line1: String,
    /// Line 2 raw text.
    pub line2: String,
}

impl Tle {
    /// Parse TLE epoch (columns 18-32 of line 1) into a rough year/day.
    pub fn epoch(&self) -> Option<(i32, f64)> {
        let l = &self.line1;
        if l.len() < 32 {
            return None;
        }
        let year2: i32 = l[18..20].trim().parse().ok()?;
        let year = if year2 < 57 { 2000 + year2 } else { 1900 + year2 };
        let day: f64 = l[20..32].trim().parse().ok()?;
        Some((year, day))
    }
}

/// Satellite category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SatGroup {
    Station,   // crewed / space stations
    Weather,
    Navigation,
    Science,
    Communications,
    Military,
    Debris,
    Other,
}

impl SatGroup {
    pub const ALL: [SatGroup; 8] = [
        SatGroup::Station,
        SatGroup::Weather,
        SatGroup::Navigation,
        SatGroup::Science,
        SatGroup::Communications,
        SatGroup::Military,
        SatGroup::Debris,
        SatGroup::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SatGroup::Station => "Station",
            SatGroup::Weather => "Weather",
            SatGroup::Navigation => "Navigation",
            SatGroup::Science => "Science",
            SatGroup::Communications => "Comms",
            SatGroup::Military => "Military",
            SatGroup::Debris => "Debris",
            SatGroup::Other => "Other",
        }
    }

    /// egui color for the group.
    pub fn color(self) -> egui::Color32 {
        use egui::Color32;
        match self {
            SatGroup::Station => Color32::from_rgb(255, 99, 71),
            SatGroup::Weather => Color32::from_rgb(80, 200, 255),
            SatGroup::Navigation => Color32::from_rgb(255, 215, 0),
            SatGroup::Science => Color32::from_rgb(150, 255, 150),
            SatGroup::Communications => Color32::from_rgb(200, 160, 255),
            SatGroup::Military => Color32::from_rgb(255, 120, 120),
            SatGroup::Debris => Color32::from_rgb(120, 120, 120),
            SatGroup::Other => Color32::from_rgb(180, 180, 180),
        }
    }
}
