# FAQ

## Nothing loads — the catalog is empty or nearly so

Celestrak is unreachable from your network. The toolbar shows an orange `⚠` with
the error; hover it for the message.

Fix: set an HTTP(S) proxy. Either in `⚙ Settings` in the toolbar (then press
`✔ Apply & Refresh`), or through the environment:

```sh
SAT_PROXY=http://127.0.0.1:7890 cargo run --release
```

The UI value wins over `SAT_PROXY`; both take effect on the next refresh. See
[Installation](Installation).

## I get HTTP 403 from Celestrak

Bare HTTP clients are blocked. The app already sends a browser-like
`User-Agent`, so a 403 usually means your network path itself is being refused
(a corporate block or a regional restriction) rather than a header problem —
route through a proxy.

## The globe or map is empty, but the sidebar has satellites

The TLEs load but cannot be propagated — usually because their epoch is old.
Wait for the next 2-hour auto-refresh, or pick a satellite with a recent TLE
epoch. Satellites that cannot be propagated are drawn in **red** in the sidebar
list, so you can spot them before selecting one.

The Detail pane will say `Propagation failed for this TLE` in red for the same
reason.

## Linux build fails

Install the X11/Wayland development packages. eframe is built with both the
`x11` and `wayland` features enabled.

## The UI feels sluggish

Check the logs with `RUST_LOG=info` — the frame loop prints a `slow frame`
warning for any frame over 50 ms. Common causes: an unusually large catalog, or
a pane whose globe mesh is being rebuilt every frame (which should only happen
during an active drag or camera ease).

## Why is a satellite in the "Other" group?

Group membership is assigned by **data source**, not parsed from the name.
Everything from the `active` group lands in `Other`. See
[Data and SGP4](Data-and-SGP4).

## Why do "Military" and "Debris" always show (0)?

None of the six configured sources supplies those categories. They are kept in
the dropdown so the full group set is visible, and they will stay empty until a
source is added.

## Why does the Detail pane show a different time than the clock?

By design. The Detail pane reads real UTC (`Utc::now()`), while the globe runs on
the accelerated sim clock. So at ×1000 the globe shows orbits racing ahead while
Detail still reports the satellite's true current state.

## The satellite count keeps growing / satellites appear twice

Known behaviour: each successful refresh **appends** to the catalog with no
de-duplication, so a long-running session accumulates duplicates. Restarting the
process rebuilds the catalog from scratch. See [Data and SGP4](Data-and-SGP4).

## I can only switch a pane between 3D and 2D — where are Ground Track and Detail?

They are implemented and render correctly, but the pane title bar's toggle only
offers `3D` and `2D`. There is currently no UI control that sets a pane to
`GroundTrack` or `Detail`. See [Views](Views).

## Where are the image and mesh files?

Embedded into the binary at compile time (`include_bytes!`). There is no runtime
asset directory to ship alongside the executable. Two files in the repository —
`land110.json` and `stars_bright.json` — are currently unreferenced by the
source.

## Does it work offline?

The UI and rendering do, but the catalog needs Celestrak at startup —
there is no on-disk cache of TLEs.

## License

MIT.

---
[← Development](Development) · [Home →](Home)
