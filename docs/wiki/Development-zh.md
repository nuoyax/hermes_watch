# 开发指南

## 目录结构

```
src/
├── main.rs            入口：tokio runtime + eframe 启动
├── lib.rs             为库目标重导出 data/orbit/service/ui
├── data/
│   ├── model.rs       Sat / Tle / SatGroup 类型、标签与配色映射
│   └── fetch/
│       ├── mod.rs     6 个 Celestrak 源、FetchConfig、HTTP 客户端
│       └── celestrak.rs   fetch_source + 纯函数 TLE 解析器（含测试）
├── orbit.rs           SGP4 封装：TEME → 大地坐标、轨迹、ECI、GMST
├── service.rs         后台抓取服务（tokio）、目录合并
└── ui/
    ├── app.rs         应用状态、帧循环、工具栏、侧栏、布局
    ├── earth.rs       海岸线加载与投影辅助
    ├── panes.rs       布局几何、窗格边框、绘制层
    ├── views.rs       模块重导出枢纽
    └── views/
        ├── globe3d.rs       贴图地球 + 相机、模型、光照
        ├── globe3d_tests.rs 其测试（体积原因单独成文件）
        ├── world_map.rs     等距圆柱地图 + 经纬网/投影辅助
        ├── ground_track.rs  地面轨迹渲染
        ├── detail.rs        轨道要素检查器
        └── catalog.rs       侧栏列表、过滤与右键菜单（含测试）
```

根目录编译期嵌入的资源：`earth_day.jpg`、`coastline110.json`、
`assets/iss_model_nasa.bin`。见[数据源与 SGP4](Data-and-SGP4-zh)。

## 测试

```sh
cargo test          # 两个 target
cargo test --lib    # 仅库目标
```

当前套件共 **48 个测试，全部通过**（每个 target 各跑一遍 —— lib 与 bin ——
因为 `src/lib.rs` 声明了同一批模块）。

```
running 48 tests
test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 为什么存在 `src/lib.rs`

它只声明 `pub mod data; pub mod orbit; pub mod service; pub mod ui;`，
没有公开函数。这是刻意为之，且与二进制目标**不冗余**。

若没有它，`src/ui/views/globe3d_tests.rs` 只会存在于**二进制**目标中。
`cargo test --lib` 于是会编译一个不含任何 UI 代码的库库，愉快地报告通过，
却**静默跳过全部 UI 测试**。把模块声明进库，可以让这些测试同时进入两个目标，
使 `--lib` 与默认运行的结论一致。

如果你要挪动 UI 测试代码，请用 `cargo test --lib` 确认数量没有下降。

### 测试覆盖什么

本套件异常偏重渲染回归，因为本项目有若干 bug 是视觉性的、极易重新引入：

- **GMST** —— 速率是一个恒星日一圈（模 360），并在五个时刻与 astropy
  参考值吻合到 0.01° 以内。地球自转辅助函数被断言为**委托**给
  `orbit::gmst_deg`，而非重复实现。
- **光照** —— 日下点被照亮；网格法线与地固光照向量一致；该一致性在全天 24 小时内成立。
- **相机与锁定** —— 锁定点投影、锁定/解锁间的偏航连续性、空闲时偏航不漂移、
  以及拖拽会解除锁定。
- **窗格隔离** —— 各窗格绘制在不同层；每个窗格回放自己的缓存网格；
  某个窗格拖拽时其他空闲窗格不闪烁。
- **网格重建节流** —— 缓动进行中每帧重建；空闲窗格保持 0.5 秒节流；
  锁定打断滑行后回到节流状态而非无限重建。（这些是「复位有卡顿」的修复。）
- **窗格交互** —— 点击只激活指针下的窗格；侧栏只作用于活动窗格且从不改写其视图；
  「加入第 N 个窗口」只作用于一个窗口、替换原有目标、弹出提示，越界时为无操作。
- **提示时序** —— 淡入、保持、淡出、消失。
- **侧栏过滤** —— 分类选择收窄列表；文本搜索与其**相交**而非替换；
  悬停与无效行配色在浅色侧栏上保持可读。
- **TLE 解析** —— 含对畸形块的优雅跳过。

UI 测试是**无窗口**运行的：它们用合成 `RawInput` 驱动 `egui::Context`，
不打开窗口，因此在 CI 与 SSH 环境下都能跑。

## 约定

- **一任务一提交。** 工作以任务编号跟踪；提交信息携带编号，
  例如 `fix(ui): tighten the sidebar layout (TASK-032)`。
- 提交主题遵循 conventional-commit 前缀：`feat`、`fix`、`docs`、`chore`、`refactor`。
- **注释解释「为什么」，不解释「做了什么」。** 若干非显然的约束都写在代码现场 ——
  `parking_lot` 的不可重入、ABBA 加锁顺序、窗格内容为何需要独立层、
  `lib.rs` 为何声明 UI。若你改动这些区域，请让注释保持真实。
- **不要为共享公式添加第二份拷贝。** GMST 与儒略日辅助函数都曾重复一次，
  也都由此产生了 bug。应当委托调用。

## 新增一种视图

1. 在 `ui/panes.rs` 里给 `ViewKind` 加变体（并加入 `ViewKind::ALL`）。
2. 在 `ui/views/` 写渲染器，接收窗格 rect 与所需状态。
3. 在 `ui/app.rs` 绘制窗格处按变体分派。
4. 若需要每窗格状态，在 `ui/panes.rs` 的 `Pane` 中加字段。
5. 在对应的 `#[cfg(test)]` 模块补测试。若该视图需要相机或缓存，
   **先读 `globe3d_tests.rs`** —— 那里面的层/网格缓存不变量很容易被破坏。

如果该视图需要对用户可选，注意窗格标题栏目前只提供 `3D` / `2D` ——
见[视图说明](Views-zh)。

## 性能注意事项

- 空闲时窗格至多每 **0.5 秒**重建一次地球网格；相机缓动或拖拽进行中则每帧重建。
- 侧栏列表是虚拟化的（`ScrollArea::show_rows`）—— 每帧构建约 18 000 个控件
  曾直接冻结 UI。
- 无效 TLE 扫描是 `O(catalog)`，最多每 180 秒执行一次。
- 帧循环在单帧超过 50 ms 时告警；`RUST_LOG=info` 下留意日志中的 `slow frame`。

---
[← 数据源与 SGP4](Data-and-SGP4-zh) · [常见问题 →](FAQ-zh)
