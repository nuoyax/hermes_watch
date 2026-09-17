# 卫星监控 (Satellite Monitor)

Rust 原生桌面卫星监控应用：抓取公开 TLE 数据源，SGP4 轨道推演，分屏多窗口显示。

[English](README.md)

## 功能

- **全网数据源** — 启动即从 Celestrak 拉取（活跃卫星、空间站、气象、GPS、科学、GEO），每 2 小时自动刷新
- **SGP4 轨道推演** — 星下点、地面轨迹（过去 45 分钟 → 未来 90 分钟）
- **原生 UI**（egui/eframe，非 Web 技术栈），支持 **分屏**：1 / 2H / 2V / 4 窗口
- **每个窗格 4 种视图** — 世界地图、地面轨迹、目录列表、详情 — 点击窗格标题栏的 `⇄` 按钮切换
- **目录侧栏** — 按名称/NORAD 编号搜索 + 按分组（着色）过滤

## 构建与运行

```sh
cargo run --release
```

需要 stable Rust 工具链（Windows 上为 MSVC target）。

## 目录结构

```
src/
  main.rs            入口：async runtime + eframe 启动
  data/              TLE 模型 + 抓取器（Celestrak）
  orbit.rs           SGP4 推演、TEME → 大地坐标转换
  service.rs         后台抓取服务（tokio）
  ui/
    app.rs           应用状态、面板、分屏布局
    panes.rs         窗格布局与边框
    views/           world_map / ground_track / catalog / detail
```
