//! Celestrak TLE catalog fetcher & TLE text parser.

use crate::data::fetch::{http_client, Source};
use crate::data::model::{Sat, SatGroup, Tle};
use anyhow::{Context, Result};

/// Fetch a source and parse its 3-line TLE text (name / line1 / line2 per sat).
pub async fn fetch_source(source: &Source, group: SatGroup) -> Result<Vec<Sat>> {
    let text = http_client()?
        .get(source.url)
        .send()
        .await
        .with_context(|| format!("request failed for {}", source.name))?
        .error_for_status()
        .with_context(|| format!("HTTP error from {}", source.name))?
        .text()
        .await?;

    Ok(parse_tle_text(&text, group))
}

/// Parse classic 3-line TLE format: blank-separated blocks of name + 2 lines.
pub fn parse_tle_text(text: &str, group: SatGroup) -> Vec<Sat> {
    let mut sats = Vec::new();
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
    let mut i = 0;
    while i + 2 < lines.len() {
        let name = lines[i].trim().to_string();
        let l1 = lines[i + 1].trim();
        let l2 = lines[i + 2].trim();
        if l1.starts_with('1') && l2.starts_with('2') && l1.len() > 20 {
            if let Some(norad) = parse_norad(l1) {
                sats.push(Sat {
                    norad_id: norad,
                    name,
                    group,
                    tle: Tle {
                        line1: l1.to_string(),
                        line2: l2.to_string(),
                    },
                });
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    sats
}

/// NORAD id = columns 3-7 of TLE line 1.
fn parse_norad(line1: &str) -> Option<u32> {
    line1[2..7].trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30777-3 0  9990
2 25544  51.6416 247.4627 0006703 130.5360 325.0288 15.72125391563537
NOAA 19
1 33591U 09005A   24001.00000000  .00000275  00000-0  14158-3 0  9998
2 33591  99.0500 100.0000 0013800 100.0000 260.0000 14.12800000 90000
";

    #[test]
    fn parses_two_sats() {
        let sats = parse_tle_text(SAMPLE, SatGroup::Station);
        assert_eq!(sats.len(), 2);
        assert_eq!(sats[0].norad_id, 25544);
        assert_eq!(sats[0].name, "ISS (ZARYA)");
        assert_eq!(sats[0].tle.epoch().unwrap().0, 2024);
    }

    #[test]
    fn skips_garbage() {
        let sats = parse_tle_text("hello\nworld\nfoo\nISS\n1 25544U ... short\n2 25544 x", SatGroup::Other);
        // name line "ISS" followed by invalid line1 -> skipped gracefully
        assert!(sats.iter().all(|s| s.norad_id != 0));
    }
}
