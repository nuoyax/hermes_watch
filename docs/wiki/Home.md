# 🪽 Hermes Watch

**A native Rust desktop satellite monitor.** Hermes Watch fetches live two-line
element (TLE) sets from public catalogs, propagates every orbit with SGP4, and
renders the whole constellation in real time on a GPU-textured Earth.

![Hermes Watch](https://raw.githubusercontent.com/nuoyax/hermes_watch/master/docs/app-screenshot.png)

## What it is

A single native window — no web stack, no Electron — built on `egui`/`eframe`
with the `glow` renderer. Every pane is an independent viewport with its own
camera and its own target satellite.

| | |
|---|---|
| **Catalog** | ~18 000 objects fetched from 6 Celestrak groups on startup, auto-refreshed every 2 hours |
| **Propagation** | Full SGP4/SDP4 via the `sgp4` crate — TEME state vectors, geodetic sub-satellite points, ground tracks, true inertial (ECI) orbit ellipses |
| **Rendering** | GPU-textured rotating Earth with a day/night terminator; 3D satellite markers and orbit rings |
| **Layout** | 1 / 2H / 2V / 4 independent panes, each keeping its own view, camera and target |
| **Clock** | Accelerated sim time (×1 to ×1000) so you can watch orbits evolve without waiting |

## Pages

| Page | 内容 |
|---|---|
| [Installation](Installation) | Toolchain, platform dependencies, build, run, proxy setup · 安装与运行 |
| [Architecture](Architecture) | Threading model, shared state, module responsibilities · 架构 |
| [Views](Views) | The view types, split layouts, per-pane interaction · 视图说明 |
| [Data and SGP4](Data-and-SGP4) | Data sources, TLE parsing, propagation and caching · 数据源与 SGP4 |
| [Development](Development) | Project layout, tests, conventions, how to contribute · 开发指南 |
| [FAQ](FAQ) | Troubleshooting and known limitations · 常见问题 |

中文页面：[Home (中文)](Home-zh) · [安装](Installation-zh) · [架构](Architecture-zh) ·
[视图](Views-zh) · [数据源与 SGP4](Data-and-SGP4-zh) · [开发](Development-zh) · [常见问题](FAQ-zh)

## Quick start

```sh
git clone https://github.com/nuoyax/hermes_watch
cd hermes_watch
cargo run --release
```

Requires a stable Rust toolchain (MSVC target on Windows). See
[Installation](Installation) for platform dependencies and proxy configuration,
and [FAQ](FAQ) if the catalog comes up empty.

## 📜 License

MIT
