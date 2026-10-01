# 安装与运行

## 环境要求

| | |
|---|---|
| **Rust** | stable 工具链，edition 2021。已在 `rustc 1.95.0` / `cargo 1.95.0` 上验证。 |
| **Windows** | MSVC target（`x86_64-pc-windows-msvc`），无需额外系统包。release 构建会隐藏控制台窗口（`windows_subsystem = "windows"`）。 |
| **Linux** | 需安装 X11/Wayland 开发库 —— eframe 同时启用了 `x11` 与 `wayland` feature，渲染使用 `glow`。 |
| **macOS** | 无需额外包。 |

本工程除默认二进制目标外还显式声明了库目标（`src/lib.rs`），因此
`cargo build` 与 `cargo test` 会把同一份代码编译两遍 —— 原因见[开发指南](Development-zh)。

## 构建与运行

```sh
git clone https://github.com/nuoyax/hermes_watch
cd hermes_watch
cargo run --release
```

> 首次构建需数分钟（依赖图包含 `eframe`、`tokio`、`reqwest` 与 `image` 系列）。
> 之后增量编译很快 —— release profile 使用 `opt-level = 3` 与 thin LTO。

产物位于 `target/release/hermes_watch`（Windows 上为 `.exe`）。

## 首次启动

启动后立即发起**六个并发抓取** —— 每个 Celestrak 分组一个。抓取期间工具栏显示
`Sources {done}/{total} · {n} sats` 与一个转圈指示。每个源完成后立即并入目录，
因此卫星是逐步出现的，而非等全部完成再一次性显示。

若某个源失败 —— 网络受限、DNS 失败、超时 —— 错误会被记录并在工具栏显示为橙色 `⚠`
（悬停查看详情），其余源继续正常加载。**没有重试逻辑**；下一次刷新即下一次尝试。

## 代理配置

视网络环境，访问 Celestrak 可能需要走代理。两种设置方式：

**1. 工具栏设置对话框** —— 点击 `⚙ Settings`，填入代理地址，按 `✔ Apply & Refresh`
（会写入配置并立即重新抓取）：

```
HTTP proxy (optional)
[ 例如 http://127.0.0.1:7890 —— 留空表示直连 ]
```

**2. 环境变量** —— 未在 UI 中设置时，回退使用 `SAT_PROXY`。空值或纯空白视为「直连」。

```sh
SAT_PROXY=http://127.0.0.1:7890 cargo run --release
```

解析顺序为 **UI 值 → `SAT_PROXY` → 直连**。代理对 `http` 与 `https` 均生效。
UI 设置与环境变量的值都在**下一次**刷新生效，不会追溯修改进行中的请求。

## 日志

应用使用 `tracing_subscriber` 与 `RUST_LOG` 环境过滤器，默认级别 `info`：

```sh
RUST_LOG=info cargo run --release      # 默认
RUST_LOG=debug cargo run --release     # 详细
```

帧循环在单帧超过 50 ms 时会打印 `slow frame` 警告 —— 如果界面感觉卡顿，这是第一个该看的地方。

## 故障排查

若目录为空或地球空白，见[常见问题](FAQ-zh)。

---
[← 首页](Home-zh) · [架构 →](Architecture-zh)
