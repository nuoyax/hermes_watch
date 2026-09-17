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

## ✨ Features

| | |
|---|---|
| 🌐 **Live catalogs** | Pulls TLEs from Celestrak on startup (active sats, stations, weather, GPS, science, GEO) — ~18 000 objects — and auto-refreshes every 2 h |
| 🛰 **SGP4 propagation** | Sub-satellite points and ground tracks (past 45 min → future 90 min), true inertial (ECI) orbit ellipses |
| 🖥 **Native UI** | egui/eframe — no web stack, no Electron. GPU-textured rotating Earth with day/night terminator |
| 🪟 **Split panes** | 1 / 2H / 2V / 4 independent viewports, each with its own camera and satellite |
| 🔍 **Catalog sidebar** | Full-text filter (name / NORAD id) + per-group color-coded toggles |
| 🗺 **Four view types** | 3D Globe · World Map · Ground Track · Detail — switchable per pane |

## 🚀 Build & Run

```sh
cargo run --release
```

> Requires a stable Rust toolchain (MSVC target on Windows).

### Optional proxy

Celestrak may be unreachable from some networks. Set the `SAT_PROXY` environment variable to route requests through a proxy:

```sh
SAT_PROXY=http://127.0.0.1:7890 cargo run --release
```

## 📁 Project Layout

```
src/
├── main.rs            entry point: async runtime + eframe launch
├── data/              TLE model + fetchers (Celestrak)
├── orbit.rs           SGP4 propagation, TEME → geodetic conversion
├── service.rs         background fetch service (tokio)
└── ui/
    ├── app.rs         application state, panels, split layout
    ├── panes.rs       pane layout & chrome
    └── views/         globe3d / world_map / ground_track / catalog / detail
```

## 📜 License

MIT
