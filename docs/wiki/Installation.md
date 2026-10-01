# Installation

## Requirements

| | |
|---|---|
| **Rust** | Stable toolchain, edition 2021. Verified on `rustc 1.95.0` / `cargo 1.95.0`. |
| **Windows** | MSVC target (`x86_64-pc-windows-msvc`). No extra system packages. Release builds suppress the console window (`windows_subsystem = "windows"`). |
| **Linux** | X11/Wayland development headers — eframe is built with both the `x11` and `wayland` features, plus `glow` for rendering. |
| **macOS** | No extra packages. |

The crate declares an explicit library target (`src/lib.rs`) alongside the
default binary, so both `cargo build` and `cargo test` exercise the same code
twice — see [Development](Development) for why that matters.

## Build and run

```sh
git clone https://github.com/nuoyax/hermes_watch
cd hermes_watch
cargo run --release
```

> The first build takes a few minutes (the dependency graph includes `eframe`,
> `tokio`, `reqwest` and the `image` stack). Subsequent incremental builds are
> fast — the release profile uses `opt-level = 3` with thin LTO.

A release binary lands at `target/release/hermes_watch` (`.exe` on Windows).

## First launch

On startup the app immediately issues **six concurrent fetches** — one per
Celestrak group. While they are in flight the toolbar reads
`Sources {done}/{total} · {n} sats` with a spinner. Each source that finishes is
merged into the catalog immediately, so satellites appear progressively rather
than all at once.

If a source fails — network blocked, DNS failure, timeout — the error is
recorded and shown as an orange `⚠` in the toolbar (hover for the message), and
the remaining sources still load. There is no retry; the next refresh is the
next attempt.

## Proxy configuration

Celestrak requests may need to go through a proxy depending on your network.
There are two ways to set one:

**1. Toolbar settings dialog** — click `⚙ Settings`, enter the proxy URL, and
press `✔ Apply & Refresh` (this writes the value and re-fetches immediately):

```
HTTP proxy (optional)
[ e.g. http://127.0.0.1:7890 — empty = direct ]
```

**2. Environment variable** — `SAT_PROXY` is used as a fallback when no proxy is
set in the UI. An empty or blank value is treated as "direct".

```sh
SAT_PROXY=http://127.0.0.1:7890 cargo run --release
```

Resolution order is **UI value → `SAT_PROXY` → direct**. The proxy is registered
for both `http` and `https`. Both the UI-set and env-set values take effect on
the **next** refresh, not retroactively for in-flight requests.

## Logging

The app initialises `tracing_subscriber` with an `RUST_LOG` environment filter,
defaulting to `info`:

```sh
RUST_LOG=info cargo run --release      # default
RUST_LOG=debug cargo run --release     # verbose
```

The frame loop logs a `slow frame` warning whenever a frame exceeds 50 ms, which
is the first thing to look at if the UI feels unresponsive.

## Troubleshooting

If the catalog comes up empty or the globe is blank, see [FAQ](FAQ).

---
[← Home](Home) · [Architecture →](Architecture)
