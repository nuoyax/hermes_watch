# 数据源与 SGP4

## 数据来源

TLE 取自 Celestrak 的 GP 接口，`FORMAT=tle`，每个分组一个请求。
共六个源，启动时**并发**发起，之后每 2 小时一次：

| 源名称 | Celestrak 分组 |
|---|---|
| `Celestrak-Active` | `active` |
| `Celestrak-Station` | `stations` |
| `Celestrak-Weather` | `weather` |
| `Celestrak-GPS` | `gps-ops` |
| `Celestrak-Science` | `science` |
| `Celestrak-Geo` | `geo` |

请求携带**浏览器风格 `User-Agent`**；裸客户端会被 Celestrak 返回 HTTP 403。
客户端超时 30 秒。

## 分组归属

卫星的 `SatGroup` **不是从名称解析的** —— 而是根据它来自哪个源，
由 `service.rs` 中的硬编码映射决定：

| 源 | → 分组 |
|---|---|
| Station | `Station` |
| Weather | `Weather` |
| GPS | `Navigation` |
| Science | `Science` |
| Active | `Other` |
| Geo | `Communications` |
| 其他 | `Other` |

八个分组为 `Station`、`Weather`、`Navigation`、`Science`、
`Communications`（显示名 "Comms"）、`Military`、`Debris`、`Other`。
每个分组有地球配色与一版变暗的 `color_on_light()` —— 分组色调按每通道
62% 缩放以适配浅色侧栏，因为地球配色是按近黑背景调的。

六个源中没有 `Military` 与 `Debris` 的数据，因此它们在下拉框中稳定显示 `(0)`。

## TLE 解析

解析器是对响应体的纯函数，无需网络即可测试。它按三行一组
（名称、第 1 行、第 2 行）遍历文本，并：

- 要求第 1 行以 `1` 开头、第 2 行以 `2` 开头，且第 1 行长度大于 20 字符，
- 从第 1 行第 3–7 列读取 NORAD 编号，
- 对畸形数据块**静默跳过**，而不是让整个源失败。

需要时会直接从原始行中取出两个要素值：

- `Tle::epoch()` —— 第 1 行第 18–32 列，采用标准的两位年份分界
  （≥ 57 视为 1900 年代，否则 2000 年代）；返回 `(年, 年内小数日)`。
- `Tle::mean_motion_revs_per_day()` —— 第 2 行第 53–63 列。

## 轨道推演

推演委托给 **`sgp4` crate 2.2 版**。`orbit.rs` 用 `Propagator` 封装它，
在 `parking_lot::RwLock<HashMap<u32, Arc<Constants>>>` 中按 NORAD 编号
缓存每颗卫星的 `sgp4::Constants`（按需由 TLE 构建）。

公开接口：

| 函数 | 返回 |
|---|---|
| `Propagator::new()` | 空的推演器 |
| `subpoint(&sat, time)` | `Option<GeoPoint>` —— 大地星下点 |
| `ground_track(&sat, time, past_min, future_min, step_min)` | `Vec<GeoPoint>` |
| `orbit_eci(&sat, time, past_min, future_min, step_min)` | `Vec<[f64; 3]>` —— TEME 位置（km） |
| `all_positions(&sats, time)` | `Vec<Option<GeoPoint>>` —— 地图视图使用 |
| `gmst_deg(time)` | GMST 角度（自由函数） |

`GeoPoint` 为 `{ lat_deg, lon_deg, alt_km }`，均为 `f64`。

应用调用时取 `past = 45 分钟`、`future = 90 分钟`、`step = 2 分钟`，
用于地面轨迹与轨道环。3D 地球还会为每个窗格缓存轨道环 ——
以 NORAD 编号、墙钟时间戳、轨道环的仿真时间中心为键 ——
仅在卫星变化、缓存过期，或仿真时钟偏离该中心过远时重新推演。

## 坐标框架与地球模型

SGP4 输出的是 **TEME/ECI** 千米向量。`orbit.rs` 用一个**球体**地球模型
把它转成类大地坐标的经纬高：

- 高度：`r − 6371.0` km，
- 纬度：`asin(z / r)` —— 地心纬度，非大地纬度（无扁率），
- 经度：TEME 赤经减去 GMST。

没有 WGS84 椭球，也没有黄赤交角/ECEF 旋转矩阵。对于可视化而言这是刻意简化，
但**不要把报告的纬度当作椭球大地纬度**。

## GMST —— 唯一真相源

地球自转角由 `orbit::gmst_deg` **统一计算一次**，其余地方（含
`globe3d::earth_rotation`）一律委托给它。这一点很关键：
此前该公式在两处重复，且两份都带同一个速率 bug —— 世纪系数用错，
算出 0.9878°/天 而不是 360.9856°/天，即地球自转速度错误。

实现为 IAU 1982 / Meeus 多项式：
`280.46061837 + 360.98564736629·d + 0.000387933·t²`，
其中 `d` 为 J2000 以来的天数，`t = d / 36525`，最后用 `rem_euclid(360)` 归一。

另一类相关 bug 藏在儒略日辅助函数里：旧的两份实现各自加上方向相反的 ±0.5 天，
导致地球自转相位差了半天的量。`julian_day` 返回的是 00:00 UT 的儒略日，
**调用方不得再加 `−0.5`**。单元测试钉住了参考值
`num_days_from_ce(1970-01-01) == 719163` → JD 2440587.5，
并用 astropy 参考值校验 GMST。

## 资源文件

| 文件 | 大小 | 使用方 | 内容 |
|---|---|---|---|
| `earth_day.jpg` | ~1.39 MB | 3D 地球 | 地球昼面贴图，4096×2048，上传时缩放至 2048×1024 并生成 mipmap |
| `assets/iss_model_nasa.bin` | ~526 KB | 3D 地球 | 烘焙的 ISS 网格，已抽减至 28k 三角面 |
| `coastline110.json` | ~237 KB | 世界地图 / 地面轨迹 | Natural Earth 110m 海岸线几何 |
| `land110.json` | ~237 KB | **无** | 在仓库中但 `src/` 未引用 |
| `stars_bright.json` | ~167 KB | **无** | 在仓库中但 `src/` 未引用 |

贴图与网格通过 `include_bytes!` 在编译期嵌入，因此 release 二进制自包含 ——
运行时不依赖任何资源目录。

**网格格式**（`iss_model_nasa.bin`）：24 字节头部
`[f32 cx, cy, cz; f32 ext; u32 nverts; u32 ntris]`，随后是 `nverts` × 3 个
`f32` 位置（已居中并除以 `ext`），再是 `ntris` ×
`[u32 a, b, c, u8 r, g, b]`。

## 已知缺口

以下是当前实现的真实性质，而非代码里遗留的 TODO：

- 源失败后**没有重试或退避**。下一次定时刷新即下一次尝试。
- 刷新之间**没有去重**。每次成功抓取都往目录追加；长时间运行的会话会累积重复项。
  （只有重启进程才会重建目录。）
- **地球模型是球体**，如上文所述。

---
[← 视图说明](Views-zh) · [开发指南 →](Development-zh)
