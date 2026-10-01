# Architecture

## The shape of it

Two threads of control, one shared state, and no framework in between:

```
┌──────────────┐   unbounded mpsc    ┌───────────────┐
│  tokio RT    │ ──────────────────▶ │   egui App    │
│  fetch tasks │   FetchMsg::Done    │  (UI thread)  │
└──────┬───────┘                     └───────┬───────┘
       │ HTTPS (celestrak.org)               │ reads, per frame
       ▼                                     ▼
  TLE text → parse                Arc<RwLock<Vec<Sat>>>      catalog
                                  Arc<RwLock<FetchStatus>>   progress
                                  Arc<RwLock<FetchConfig>>   proxy
                                             │
                                             ▼
                                    SGP4 propagate per frame
                                    (orbit.rs, cached)
```

`main.rs` builds a **multi-thread tokio runtime with 4 worker threads**, creates
the three `Arc<RwLock<…>>` values, starts the initial fetch through
`service::spawn`, and hands everything to `eframe::run_native`. The runtime is
itself wrapped in an `Arc` so the UI can spawn further fetches on it.

## Shared state

| Value | Type | Written by | Read by |
|---|---|---|---|
| Catalog | `Arc<RwLock<Vec<Sat>>>` | fetch tasks (append) | UI, every frame |
| Fetch status | `Arc<RwLock<FetchStatus>>` | fetch tasks | UI toolbar |
| Fetch config | `Arc<RwLock<FetchConfig>>` | UI settings dialog | fetch tasks (snapshot per fetch) |

Locks are `parking_lot::RwLock`, not `std`. **`parking_lot` locks are not
reentrant**, and this codebase has been bitten by that: an earlier version held
a `catalog.read()` guard while a helper took a second read of the same lock,
which deadlocked the UI thread the moment a fetch task queued a `write()`. The
fix was to pass the already-borrowed slice (`&[Sat]`) into the helper instead of
letting it re-lock. If you add code that touches the catalog, **pass the slice,
never re-acquire inside a guard.**

There is a second, subtler ordering rule: the UI locks **catalog → status**,
while `service.rs` deliberately reads the catalog length **before** taking the
status write lock, precisely so it never holds them in the opposite order. That
is an ABBA-avoidance comment in the source, not an accident — preserve it.

## Message passing

`service::spawn` returns an **unbounded** `tokio::sync::mpsc::UnboundedReceiver<FetchMsg>`.
One `FetchMsg::SourceDone { source, result }` is sent per finished source, and
the UI drains the channel at the top of every frame. The UI then appends the
satellites and updates progress. Nothing blocks on a fetch; a slow or dead
source simply never delivers a message.

## Module responsibilities

| Module | Responsibility |
|---|---|
| `main.rs` | runtime construction, shared state, eframe launch |
| `lib.rs` | re-exports `data`, `orbit`, `service`, `ui` so the library target can compile the UI (see [Development](Development)) |
| `data/model.rs` | `Sat`, `Tle`, `SatGroup` — the data types and their label/color mappings |
| `data/fetch/mod.rs` | the 6-source list, `FetchConfig`, the HTTP client builders |
| `data/fetch/celestrak.rs` | `fetch_source` (async) and the pure TLE parser |
| `orbit.rs` | SGP4 wrapper: TEME → geodetic, ground tracks, ECI polylines, GMST |
| `service.rs` | spawns one task per source, merges results, reports status |
| `ui/app.rs` | all application state, the frame loop, toolbar, sidebar, split layout |
| `ui/panes.rs` | layout geometry, pane chrome, per-pane paint layers |
| `ui/earth.rs` | coastline data loading and projection helpers |
| `ui/views/` | the renderers (see [Views](Views)) |

## The frame loop

`App::update` runs, in order: drain fetch messages → advance the sim clock →
maybe trigger the 2-hour refresh → `top_bar` → `settings_dialog` →
`sim_speed_bar` → `sidebar` → `content` → repaint request.

Two things worth knowing about the state it maintains:

- **The sidebar snapshot.** Cloning ~18 000 satellites every frame is wasteful,
  so the app keeps a `Vec<Sat>` snapshot behind a `catalog_version` cell, and
  rebuilds it only when the catalog length changes.
- **The invalid-satellite scan.** Satellites whose TLEs cannot be propagated are
  flagged red so the user finds out before selecting one. The scan is
  `O(catalog)` and reruns at most every **180 s** (or when the catalog changes)
  — deliberately *not* per frame.

## Panes and paint layers

Each pane paints its chrome on one shared `Order::Background` layer but its
*content* on its own layer, `("pane-content", index)`. This is load-bearing: the
3D globe keys its per-pane triangle-mesh cache by `painter.layer_id()`, so
panes sharing a layer would share — and fight over — one cache slot. The
symptom of getting this wrong was panes replaying each other's mesh, which read
as constant flickering.

Pane content UIs are built with `Ui::new` on that layer rather than
`Ui::new_child`, and `ctx.move_to_top(layer)` lifts it for hit-testing. See the
doc comments in `ui/panes.rs` for why the child-UI route does not work in
egui 0.29.

---
[← Installation](Installation) · [Views →](Views)
