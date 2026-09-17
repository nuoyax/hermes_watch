<div align="center">

# 🪽 Hermes Watch（赫尔墨斯之眼）

**Rust 原生桌面卫星监控 — 希腊神话中的信使赫尔墨斯，替你守望天穹。**

[![Rust](https://img.shields.io/badge/rust-stable-orange?logo=rust)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue)](#)
[![License](https://img.shields.io/badge/license-MIT-green)](#)

[English](README.md)

</div>

---

希腊神话中，**赫尔墨斯（Hermes）**脚踏飞鞋、头戴翼帽，是众神的信使、旅者与天空的守护神。*Hermes Watch* 把这个角色交给你的桌面：启动即抓取公开 TLE 轨道数据，用 SGP4 推演每颗卫星的轨道，并实时渲染整个星座。

## ✨ 功能

| | |
|---|---|
| 🌐 **全网数据源** | 启动即从 Celestrak 拉取（活跃卫星、空间站、气象、GPS、科学、GEO）— 约 18 000 个目标，每 2 小时自动刷新 |
| 🛰 **SGP4 轨道推演** | 星下点、地面轨迹（过去 45 分钟 → 未来 90 分钟）、真实惯性系（ECI）轨道椭圆 |
| 🖥 **原生 UI** | egui/eframe — 非 Web 技术栈、非 Electron。GPU 贴图旋转地球，含昼夜晨昏线 |
| 🪟 **分屏多窗格** | 1 / 2H / 2V / 4 个独立视口，每个窗格有独立相机与目标卫星 |
| 🔍 **目录侧栏** | 按名称 / NORAD 编号搜索 + 按分组（着色）过滤 |
| 🗺 **四种视图** | 3D 地球 · 世界地图 · 地面轨迹 · 详情 — 每个窗格可切换 |

## 🚀 构建与运行

```sh
cargo run --release
```

> 需要 stable Rust 工具链（Windows 上为 MSVC target）。

### 可选代理

部分网络环境无法直连 Celestrak，可设置 `SAT_PROXY` 环境变量走代理：

```sh
SAT_PROXY=http://127.0.0.1:7890 cargo run --release
```

## 📁 目录结构

```
src/
├── main.rs            入口：async runtime + eframe 启动
├── data/              TLE 模型 + 抓取器（Celestrak）
├── orbit.rs           SGP4 推演、TEME → 大地坐标转换
├── service.rs         后台抓取服务（tokio）
└── ui/
    ├── app.rs         应用状态、面板、分屏布局
    ├── panes.rs       窗格布局与边框
    └── views/         globe3d / world_map / ground_track / catalog / detail
```

## 📜 许可证

MIT
