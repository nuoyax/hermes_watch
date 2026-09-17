//! Fetchers for public satellite data sources (TLE catalogs).

pub mod celestrak;

use anyhow::Result;
use std::time::Duration;

/// A named remote source of TLEs.
pub struct Source {
    pub name: &'static str,
    pub url: &'static str,
}

/// All built-in sources (the "whole network" catalog set).
pub const SOURCES: &[Source] = &[
    Source {
        name: "Celestrak-Active",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=active&FORMAT=tle",
    },
    Source {
        name: "Celestrak-Station",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=stations&FORMAT=tle",
    },
    Source {
        name: "Celestrak-Weather",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=weather&FORMAT=tle",
    },
    Source {
        name: "Celestrak-GPS",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=gps-ops&FORMAT=tle",
    },
    Source {
        name: "Celestrak-Science",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=science&FORMAT=tle",
    },
    Source {
        name: "Celestrak-Geo",
        url: "https://celestrak.org/NORAD/elements/gp.php?GROUP=geo&FORMAT=tle",
    },
];

/// HTTP client with browser-like UA. Proxy is optional: read from the
/// SAT_PROXY env var (configurable in the toolbar settings).
pub fn http_client() -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) sat-monitor/0.1")
        .timeout(Duration::from_secs(30));
    if let Ok(proxy) = std::env::var("SAT_PROXY") {
        if let Ok(p) = reqwest::Proxy::http(&proxy) {
            builder = builder.proxy(p);
        }
        if let Ok(p) = reqwest::Proxy::https(&proxy) {
            builder = builder.proxy(p);
        }
    }
    Ok(builder.build()?)
}
