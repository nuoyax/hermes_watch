<div align="center">

# 🪽 Hermes Watch

**A native Rust desktop satellite monitor — Hermes, messenger of the gods, now watching the heavens for you.**

[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-green)](#)

[中文文档](README.zh-CN.md)

</div>

---

In Greek mythology, **Hermes** — winged sandals, winged cap — is the swift messenger of the gods, patron of travelers and the skies. *Hermes Watch* gives that role to your desktop: it fetches live two-line element (TLE) sets from public catalogs, propagates every orbit with SGP4, and renders the whole constellation in real time.

![Hermes Watch screenshot](docs/screenshot.png)
*<p align="center">3D globe pane — live SGP4 orbit on a textured Earth</p>*

---

## ✨ Features

- 🌐 **Live catalogs** — pulls TLEs from Celestrak on startup (~18 000 objects across 6 groups) and auto-refreshes every 2 hours
- 🛰 **SGP4 propagation** — full SGP4/SDP4 model, TEME → geodetic conversion, sub-satellite points and ground tracks (past 45 min → future 90 min), true inertial (ECI) orbit ellipses
- 🖥 **Native UI** — egui/eframe with the glow renderer; no web stack, no Electron. GPU-textured rotating Earth with day/night terminator
- 🪟 **Split panes** — 1 / 2H / 2V / 4 independent viewports, each with its own camera and selected satellite
- 🔍 **Catalog sidebar** — full-text filter (name / NORAD id) + per-group color-coded toggles
- 🗺 **Four view types** — 3D Globe · World Map · Ground Track · Detail — switchable independently per pane

## 🗺 Views

Each pane can independently render one of four views:

| View | What you see |
|---|---|
| **3D Globe** | GPU-textured rotating Earth with day/night terminator, satellite markers in 3D space and the full ECI orbit ellipse of the selected satellite. Drag to orbit the camera, scroll to zoom. |
| **World Map** | 2D equirectangular map with all catalog satellites at their current sub-satellite points, color-coded by group. |
| **Ground Track** | The selected satellite's ground track — past 45 minutes (fading) → future 90 minutes — plus sub-satellite point and footprint circle. |
| **Detail** | Live orbital elements of the selected satellite: lat/lon/alt, velocity, period, inclination, epoch age, and TLE text. |

## 🪟 Split Layout

Use the toolbar to switch the workspace between:

- **1** — single full-area viewport
- **2H / 2V** — two panes side by side / stacked
- **4** — quadrant layout

Every pane keeps its own view type, camera state and target satellite, so you can watch, e.g., the globe in one pane and the ground track in another simultaneously.

## 🚀 Build & Run

```sh
cargo run --release
```

> Requires a stable Rust toolchain (MSVC target on Windows). First build takes a few minutes; subsequent incremental builds are fast.

### First launch

On startup the app immediately fetches all 6 Celestrak groups in parallel. While fetching, the status bar shows progress (`sources done / total`, object count). If a source fails (network blocked, timeout), the error is shown in the status bar and the remaining sources still load.

### Network notes

- Requests go to `celestrak.org` with a browser-like `User-Agent` (bare clients get HTTP 403).
- An HTTP(S) proxy can be configured in the **toolbar settings dialog**; changes take effect on the next refresh. The `SAT_PROXY` environment variable (e.g. `SAT_PROXY=http://127.0.0.1:7890`) is used as a fallback when no proxy is set in the UI.

## 🏛 Architecture

```
┌─────────────┐    mpsc channel     ┌──────────────┐
│  tokio RT   │ ──────────────────▶ │   egui App   │
│  fetch svc  │   FetchMsg::Done    │  (UI thread) │
└──────┬──────┘                     └──────┬───────┘
       │ HTTP (Celestrak)                  │ reads
       ▼                                   ▼
  TLE text parse            Arc<RwLock<catalog>> / Arc<RwLock<status>>
                                             │
                                             ▼
                                    SGP4 propagate per frame
                                    (orbit.rs, cached)
```

- **`main.rs`** — builds a 4-worker tokio runtime, shared `Arc<RwLock>` state, launches eframe.
- **`data/fetch/`** — source list (6 Celestrak endpoints) + TLE parsing; `FetchConfig` carries the optional proxy.
- **`service.rs`** — spawns one task per source; each completed source is sent to the UI as a `FetchMsg` and merged into the catalog.
- **`orbit.rs`** — SGP4 wrapper: propagates TEME state vectors, converts to geodetic lat/lon/alt, generates ground tracks and ECI orbit polylines (results cached per epoch to keep the UI at 60 fps).
- **`ui/`** — `app.rs` owns state, toolbar, sidebar and split layout; `panes.rs` draws pane chrome; `views/` implements the four renderers.

## 📁 Project Layout

```
src/
├── main.rs            entry point: async runtime + eframe launch
├── data/
│   ├── model.rs       Sat / SatGroup types, TLE structs
│   └── fetch/         Celestrak sources, HTTP client, TLE parsing
├── orbit.rs           SGP4 propagation, TEME → geodetic conversion
├── service.rs         background fetch service (tokio)
└── ui/
    ├── app.rs         application state, toolbar, sidebar, split layout
    ├── panes.rs       pane layout & chrome
    └── views/
        ├── globe3d.rs       3D textured globe (+ tests)
        ├── world_map.rs     2D equirectangular map
        ├── ground_track.rs  ground track renderer
        ├── detail.rs        orbital element inspector
        └── catalog.rs       sidebar list & filter
```

## 🔧 Troubleshooting

| Symptom | Likely cause / fix |
|---|---|
| Source errors in status bar, few or no satellites | Celestrak unreachable from your network — set a proxy in the toolbar settings (or `SAT_PROXY`) and refresh |
| HTTP 403 in errors | Client blocked — the app already sends a browser UA; a proxy may still be needed |
| Empty globe/map but catalog sidebar populated | Propagation epoch too old — wait for the next 2 h auto-refresh and pick a satellite with a recent epoch |
| Build fails on Linux | Install X11/Wayland dev packages (eframe `x11`/`wayland` features) |

## 📜 License

MIT
