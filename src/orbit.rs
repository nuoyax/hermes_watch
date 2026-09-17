//! Orbit propagation (sub-satellite points / ground tracks) via SGP4.

use crate::data::model::{Sat, Tle};
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use sgp4::{Constants, Elements, MinutesSinceEpoch, Prediction};
use std::collections::HashMap;
use std::sync::Arc;

/// A sub-satellite point.
#[derive(Debug, Clone, Copy)]
pub struct GeoPoint {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_km: f64,
}

/// Per-satellite propagator cache. sgp4 2.x binds Constants to one element set,
/// so we build one Constants per satellite on demand.
pub struct Propagator {
    cache: parking_lot::RwLock<HashMap<u32, Arc<Constants>>>,
}

impl Propagator {
    pub fn new() -> Self {
        Self {
            cache: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    fn constants_for(&self, sat: &Sat) -> Option<Arc<Constants>> {
        {
            let c = self.cache.read();
            if let Some(k) = c.get(&sat.norad_id) {
                return Some(Arc::clone(k));
            }
        }
        let el = Elements::from_tle(
            Some(sat.name.clone()),
            sat.tle.line1.as_bytes(),
            sat.tle.line2.as_bytes(),
        )
        .ok()?;
        let consts = Arc::new(Constants::from_elements(&el).ok()?);
        self.cache.write().insert(sat.norad_id, Arc::clone(&consts));
        Some(consts)
    }

    /// Sub-satellite point at `time`.
    pub fn subpoint(&self, sat: &Sat, time: DateTime<Utc>) -> Option<GeoPoint> {
        let consts = self.constants_for(sat)?;
        let minutes = tle_minutes(&sat.tle, time)?;
        let pred = consts.propagate(MinutesSinceEpoch(minutes)).ok()?;
        Some(to_geodetic(pred, time))
    }

    /// Ground track points over `[time - past_min, time + future_min]`.
    pub fn ground_track(
        &self,
        sat: &Sat,
        time: DateTime<Utc>,
        past_min: f64,
        future_min: f64,
        step_min: f64,
    ) -> Vec<GeoPoint> {
        let Some(consts) = self.constants_for(sat) else {
            return Vec::new();
        };
        let Some(center) = tle_minutes(&sat.tle, time) else {
            return Vec::new();
        };
        let mut pts = Vec::new();
        let mut m = center + past_min;
        while m <= center + future_min {
            if let Ok(pred) = consts.propagate(MinutesSinceEpoch(m)) {
                pts.push(to_geodetic(pred, time));
            }
            m += step_min;
        }
        pts
    }

    /// Propagate all satellites' current positions (for the map view).
    pub fn all_positions(&self, sats: &[Sat], time: DateTime<Utc>) -> Vec<Option<GeoPoint>> {
        sats.iter().map(|s| self.subpoint(s, time)).collect()
    }
}

impl Default for Propagator {
    fn default() -> Self {
        Self::new()
    }
}

/// Minutes since TLE epoch.
fn tle_minutes(tle: &Tle, time: DateTime<Utc>) -> Option<f64> {
    let (year, day_frac) = tle.epoch()?;
    let epoch = day_of_year_to_datetime(year, day_frac)?;
    Some((time - epoch).num_seconds() as f64 / 60.0)
}

/// Convert "day of year (fractional)" to a UTC datetime.
fn day_of_year_to_datetime(year: i32, day: f64) -> Option<DateTime<Utc>> {
    let day_int = day.floor() as u32;
    let frac = day - day.floor();
    let jan1 = NaiveDate::from_ymd_opt(year, 1, 1)?;
    let date = jan1 + chrono::Duration::days(day_int as i64 - 1);
    let secs = frac * 86400.0;
    let naive = date
        .and_hms_opt(0, 0, 0)?
        + chrono::Duration::milliseconds((secs * 1000.0) as i64);
    Some(DateTime::from_naive_utc_and_offset(naive, Utc))
}

/// Convert TEME prediction + observation time to geodetic lat/lon/alt.
fn to_geodetic(pred: Prediction, time: DateTime<Utc>) -> GeoPoint {
    let x = pred.position[0];
    let y = pred.position[1];
    let z = pred.position[2];

    let r = (x * x + y * y + z * z).sqrt();
    let lat = (z / r).asin().to_degrees();
    let alt = r - 6371.0;

    let gmst = gmst_deg(time);
    let lon = wrap_degrees(y.atan2(x).to_degrees() - gmst);

    GeoPoint {
        lat_deg: lat,
        lon_deg: lon,
        alt_km: alt,
    }
}

fn wrap_degrees(d: f64) -> f64 {
    let mut d = d % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

/// Approximate Greenwich Mean Sidereal Time in degrees at `time`.
fn gmst_deg(time: DateTime<Utc>) -> f64 {
    // Julian date at 0h UT then fraction (IAU 1982 simplified).
    let (y, m, d) = (time.year(), time.month(), time.day());
    let day = NaiveDate::from_ymd_opt(y, m as u32, d as u32).unwrap_or_default();
    let jd0 = julian_day(day.num_days_from_ce()) - 0.5;
    let jd = jd0 + time.hour() as f64 / 24.0 + time.minute() as f64 / 1440.0
        + time.second() as f64 / 86400.0;
    let t = (jd - 2_451_545.0) / 36_525.0;
    let gmst = 280.46061837 + 36_079.8750114 * t + 0.000_387_933 * t * t;
    let mut g = gmst % 360.0;
    if g < 0.0 {
        g += 360.0;
    }
    g
}

/// Julian Day from days-since-CE (chrono proleptic Gregorian).
fn julian_day(days_from_ce: i32) -> f64 {
    // CE day 1 = JD 1721425.5 (0001-01-01 00:00 UTC, proleptic Gregorian).
    1_721_425.5 + days_from_ce as f64 - 1.0
}
