# Development

## Project layout

```
src/
├── main.rs            entry point: tokio runtime + eframe launch
├── lib.rs             re-exports data/orbit/service/ui for the library target
├── data/
│   ├── model.rs       Sat / Tle / SatGroup types, label and colour mappings
│   └── fetch/
│       ├── mod.rs     the 6 Celestrak sources, FetchConfig, HTTP clients
│       └── celestrak.rs   fetch_source + the pure TLE parser (+ tests)
├── orbit.rs           SGP4 wrapper: TEME → geodetic, tracks, ECI, GMST
├── service.rs         background fetch service (tokio), catalog merging
└── ui/
    ├── app.rs         application state, frame loop, toolbar, sidebar, layout
    ├── earth.rs       coastline loading + projection helpers
    ├── panes.rs       layout geometry, pane chrome, paint layers
    ├── views.rs       module re-export hub
    └── views/
        ├── globe3d.rs       textured globe + camera, models, lighting
        ├── globe3d_tests.rs its tests (kept separate for size)
        ├── world_map.rs     equirectangular map + grid/projection helpers
        ├── ground_track.rs  ground track renderer
        ├── detail.rs        orbital element inspector
        └── catalog.rs       the sidebar list, filter and context menu (+ tests)
```

Root assets embedded at compile time: `earth_day.jpg`, `coastline110.json`,
`assets/iss_model_nasa.bin`. See [Data and SGP4](Data-and-SGP4).

## Tests

```sh
cargo test          # both targets
cargo test --lib    # library target only
```

The current suite is **48 tests, all passing** (they run once per target —
lib and bin — because `src/lib.rs` declares the same modules).

```
running 48 tests
test result: ok. 48 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Why `src/lib.rs` exists

It declares `pub mod data; pub mod orbit; pub mod service; pub mod ui;` and
nothing else — no public functions. That is deliberate and it is **not**
redundant with the binary target.

Without it, `src/ui/views/globe3d_tests.rs` would live only in the **binary**
target. `cargo test --lib` would then compile a library with no UI code in it,
report a cheerful pass, and **silently skip every UI test**. Declaring the
modules in the library puts those tests on both targets, so `--lib` and the
default run agree.

If you ever move UI test code around, verify with `cargo test --lib` that the
count does not drop.

### What the tests cover

The suite is unusually weighted toward rendering regressions, because several of
the bugs in this project were visual and easy to reintroduce:

- **GMST** — rate is one revolution per sidereal day (mod 360), and matches
  astropy reference values to within 0.01° at five instants. The Earth-rotation
  helper is asserted to *delegate* to `orbit::gmst_deg` rather than duplicate it.
- **Lighting** — the subsolar point is lit; the mesh normal agrees with the
  Earth-fixed lighting vector; the agreement holds across all 24 hours.
- **Camera and locks** — lock projections, yaw continuity across lock/unlock,
  no yaw drift while idle, and that a drag releases the lock.
- **Per-pane isolation** — panes paint on distinct layers; each pane replays its
  own cached mesh; an idle pane does not flicker while another pane drags.
- **Mesh rebuild throttling** — an in-flight ease rebuilds every frame, an idle
  pane stays throttled at 0.5 s, and a lock that interrupts a glide returns to
  the throttle rather than rebuilding forever. (These are the "复位有卡顿"
  fixes.)
- **Pane interaction** — a click activates only the pane under the pointer; the
  sidebar targets exactly the active pane and never rewrites its view; the
  "Add to Window N" action targets exactly one window, replaces the occupant,
  raises a toast, and is a no-op when out of range.
- **Toast timing** — fade in, hold, fade out, then gone.
- **Sidebar filtering** — category selection narrows the list; text search
  intersects with it rather than replacing it; hover and invalid-row colours stay
  legible on the light sidebar surface.
- **TLE parsing** — including graceful skipping of malformed blocks.

UI tests run **headless**: they drive `egui::Context` with synthetic `RawInput`
rather than opening a window, so they work in CI and over SSH.

## Conventions

- **One task, one commit.** Work is tracked in task IDs; commit messages carry
  the id, e.g. `fix(ui): tighten the sidebar layout (TASK-032)`.
- Commit subjects follow a conventional-commit prefix: `feat`, `fix`, `docs`,
  `chore`, `refactor`.
- **Comments explain why, not what.** Several non-obvious constraints are
  documented at the point of the code — the non-reentrancy of `parking_lot`, the
  ABBA lock order, why pane content needs its own layer, why `lib.rs` declares
  the UI. If you change one of those areas, keep the comment truthful.
- **Do not add a second copy of a shared formula.** GMST and the Julian-date
  helpers both duplicated once and both drifted into bugs. Delegate instead.

## Adding a new view

1. Add a `ViewKind` variant in `ui/panes.rs` (and to `ViewKind::ALL`).
2. Write the renderer in `ui/views/`, taking the pane's rect and whatever state
   it needs.
3. Dispatch on the variant where panes are drawn in `ui/app.rs`.
4. If it needs per-pane state, add the field to `Pane` in `ui/panes.rs`.
5. Add tests to the appropriate `#[cfg(test)]` module. If the view needs a
   camera or a cache, read `globe3d_tests.rs` first — the layer/mesh-cache
   invariants there are easy to break.

If the view should be user-selectable, note that the pane title bar currently
offers only `3D` / `2D` — see [Views](Views).

## Performance notes

- Panes rebuild their globe mesh at most once per **0.5 s** when idle, and every
  frame while a camera ease or drag is in flight.
- The sidebar list is virtualized (`ScrollArea::show_rows`) — building ~18 000
  widgets per frame previously froze the UI.
- The invalid-TLE scan is `O(catalog)` and runs at most every 180 s.
- The frame loop warns when a frame exceeds 50 ms; watch for `slow frame` in the
  logs under `RUST_LOG=info`.

---
[← Data and SGP4](Data-and-SGP4) · [FAQ →](FAQ)
