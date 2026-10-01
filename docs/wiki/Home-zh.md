# 🪽 Hermes Watch（赫尔墨斯之眼）

**Rust 原生桌面卫星监控。** 启动即从公开目录抓取实时两行根数（TLE），用 SGP4
推演每一条轨道，并在 GPU 贴图地球上实时渲染整个星座。

![Hermes Watch](https://raw.githubusercontent.com/nuoyax/hermes_watch/master/docs/app-screenshot.png)

## 这是什么

一个原生窗口程序 —— 非 Web 技术栈、非 Electron —— 基于 `egui`/`eframe`
与 `glow` 渲染器。每个窗格都是独立视口，各自持有相机与目标卫星。

| | |
|---|---|
| **目录数据** | 启动时从 6 个 Celestrak 分组抓取约 18 000 个目标，每 2 小时自动刷新 |
| **轨道推演** | 通过 `sgp4` crate 实现完整 SGP4/SDP4 —— TEME 状态向量、大地星下点、地面轨迹、真实惯性系（ECI）轨道椭圆 |
| **渲染** | GPU 贴图旋转地球（含昼夜晨昏线）、3D 卫星标记与轨道环 |
| **布局** | 1 / 2H / 2V / 4 个独立窗格，各自保留视图、相机与目标 |
| **时钟** | 可加速仿真时间（×1 ~ ×1000），无需久等即可观察轨道演化 |

## 页面索引

| 页面 | 内容 |
|---|---|
| [安装与运行](Installation-zh) | 工具链、平台依赖、构建、运行、代理配置 |
| [架构](Architecture-zh) | 线程模型、共享状态、各模块职责 |
| [视图说明](Views-zh) | 视图类型、分屏布局、窗格交互 |
| [数据源与 SGP4](Data-and-SGP4-zh) | 数据来源、TLE 解析、推演与缓存 |
| [开发指南](Development-zh) | 目录结构、测试、代码约定、参与方式 |
| [常见问题](FAQ-zh) | 故障排查与已知限制 |

English pages: [Home](Home) · [Installation](Installation) · [Architecture](Architecture) ·
[Views](Views) · [Data and SGP4](Data-and-SGP4) · [Development](Development) · [FAQ](FAQ)

## 快速开始

```sh
git clone https://github.com/nuoyax/hermes_watch
cd hermes_watch
cargo run --release
```

需要 stable Rust 工具链（Windows 上为 MSVC target）。平台依赖与代理配置见
[安装与运行](Installation-zh)；若目录抓取为空，见 [常见问题](FAQ-zh)。

## 📜 许可证

MIT
