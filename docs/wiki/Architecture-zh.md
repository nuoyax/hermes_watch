# 架构

## 整体形态

两条控制流、一份共享状态，中间没有框架介入：

```
┌──────────────┐   unbounded mpsc    ┌───────────────┐
│  tokio RT    │ ──────────────────▶ │   egui App    │
│  抓取任务     │   FetchMsg::Done    │  (UI 线程)     │
└──────┬───────┘                     └───────┬───────┘
       │ HTTPS (celestrak.org)               │ 每帧读取
       ▼                                     ▼
  TLE 文本 → 解析                Arc<RwLock<Vec<Sat>>>     目录
                                Arc<RwLock<FetchStatus>>  进度
                                Arc<RwLock<FetchConfig>>  代理
                                             │
                                             ▼
                                    每帧 SGP4 推演
                                    (orbit.rs，带缓存)
```

`main.rs` 构建一个**4 工作线程的多线程 tokio runtime**，创建三个
`Arc<RwLock<…>>`，通过 `service::spawn` 启动首次抓取，再把一切交给
`eframe::run_native`。runtime 本身也被包进 `Arc`，以便 UI 能往上派发新的抓取任务。

## 共享状态

| 值 | 类型 | 写入方 | 读取方 |
|---|---|---|---|
| 目录 | `Arc<RwLock<Vec<Sat>>>` | 抓取任务（追加） | UI，每帧 |
| 抓取状态 | `Arc<RwLock<FetchStatus>>` | 抓取任务 | UI 工具栏 |
| 抓取配置 | `Arc<RwLock<FetchConfig>>` | UI 设置对话框 | 抓取任务（每次抓取取快照） |

锁用的是 `parking_lot::RwLock`，不是 `std`。**`parking_lot` 的锁不可重入**，
本工程已经踩过这个坑：早期版本持着 `catalog.read()` 的同时又让某个辅助函数
对同一把锁取第二次读，一旦有抓取任务排队请求 `write()`，UI 线程当场阻塞。
修复方式是改为把已借出的切片（`&[Sat]`）传进辅助函数，而不是让它重新加锁。
**若要新增触碰目录的代码，请传递切片，绝不要在 guard 内部重新加锁。**

还有一条更隐蔽的顺序规则：UI 的加锁顺序是 **catalog → status**，
而 `service.rs` 刻意先读目录长度、**再**取 status 写锁，正是为了避免以相反顺序
持有二者。这是源码里明确注释的 ABBA 规避，不是巧合 —— 请保持。

## 消息传递

`service::spawn` 返回**无界**的 `tokio::sync::mpsc::UnboundedReceiver<FetchMsg>`。
每个源完成后发送一条 `FetchMsg::SourceDone { source, result }`，
UI 在每帧开头排空该通道，然后追加卫星并更新进度。
任何地方都不会阻塞等待抓取；慢速或已死的源只是永远不投递消息。

## 模块职责

| 模块 | 职责 |
|---|---|
| `main.rs` | runtime 构建、共享状态、eframe 启动 |
| `lib.rs` | 重导出 `data`、`orbit`、`service`、`ui`，使库目标也能编译 UI（见[开发指南](Development-zh)） |
| `data/model.rs` | `Sat`、`Tle`、`SatGroup` —— 数据类型及其标签/配色映射 |
| `data/fetch/mod.rs` | 6 个数据源列表、`FetchConfig`、HTTP 客户端构建器 |
| `data/fetch/celestrak.rs` | `fetch_source`（异步）与纯函数式 TLE 解析器 |
| `orbit.rs` | SGP4 封装：TEME → 大地坐标、地面轨迹、ECI 折线、GMST |
| `service.rs` | 每源一个任务、合并结果、上报状态 |
| `ui/app.rs` | 全部应用状态、帧循环、工具栏、侧栏、分屏 |
| `ui/panes.rs` | 布局几何、窗格边框、每窗格绘制层 |
| `ui/earth.rs` | 海岸线数据加载与投影辅助 |
| `ui/views/` | 各渲染器（见[视图说明](Views-zh)） |

## 帧循环

`App::update` 依次执行：排空抓取消息 → 推进仿真时钟 → 视情况触发 2 小时刷新 →
`top_bar` → `settings_dialog` → `sim_speed_bar` → `sidebar` → `content` → 请求重绘。

其中两项状态维护值得了解：

- **侧栏快照。** 每帧克隆约 18 000 颗卫星是浪费，因此应用用 `catalog_version`
  单元维护一份 `Vec<Sat>` 快照，仅在目录长度变化时重建。
- **无效卫星扫描。** TLE 无法推演的卫星会被标红，让用户在选择前就知道。
  该扫描是 `O(catalog)`，最多每 **180 秒**重跑一次（或目录变化时），
  刻意**不**逐帧执行。

## 窗格与绘制层

每个窗格把边框画在共享的 `Order::Background` 层上，但把**内容**画在自己的层
`("pane-content", index)` 上。这一点是关键：3D 地球以 `painter.layer_id()`
为键缓存每窗格的三角网格，若多个窗格共用一个层，就会共用 —— 并互相争抢 ——
同一个缓存槽。曾出现的症状是窗格互相回放对方的网格，观感就是持续闪烁。

窗格内容 UI 用该层上的 `Ui::new` 构建，而非 `Ui::new_child`，
并用 `ctx.move_to_top(layer)` 把它提升以参与命中测试。egui 0.29 下子 UI
方案为何不可行，见 `ui/panes.rs` 的文档注释。

---
[← 安装与运行](Installation-zh) · [视图说明 →](Views-zh)
