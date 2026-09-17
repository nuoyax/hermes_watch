# Satellite Monitor

A native Rust desktop application for monitoring satellites: fetches live TLE catalogs from public sources, propagates orbits with SGP4, and renders split-pane views.

[中文文档](README.zh-CN.md)

## Features

- **Live data sources** — pulls TLE catalogs from Celestrak (active sats, stations, weather, GPS, science, GEO) on startup and refreshes every 2 h
- **SGP4 orbit propagation** — sub-satellite points, ground tracks (past 45 min → future 90 min)
- **Native UI** (egui/eframe, no web stack) with **split panes**: 1 / 2H / 2V / 4 windows
- **Four view types** per pane — World Map, Ground Track, Catalog, Detail — click the `⇄` button in a pane's title bar to cycle
- **Catalog sidebar** — full-text filter (name / NORAD id) + per-group toggles with color coding

## Build & Run

```sh
cargo run --release
```

Requires a stable Rust toolchain (MSVC target on Windows).

## Layout

```
src/
  main.rs            entry: async runtime + eframe launch
  data/              TLE model + fetchers (Celestrak)
  orbit.rs           SGP4 propagation, TEME → geodetic conversion
  service.rs         background fetch service (tokio)
  ui/
    app.rs           application state, panels, split layout
    panes.rs         pane layout & chrome
    views/           world_map / ground_track / catalog / detail
```
