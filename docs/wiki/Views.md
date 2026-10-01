# Views

Every pane renders one view. A pane owns its view kind, its camera state and its
target satellite, so you can watch the globe in one pane and the ground track in
another at the same time.

## View kinds

`ViewKind` has five variants, all defined in `ui/panes.rs`:

### 🌐 3D Globe

The main view. Renders:

- a dark space backdrop with a blue atmosphere limb around the planet,
- a GPU-textured Earth sphere — an egui triangle mesh at 48 latitude × 96
  longitude bands (roughly 4.8 k vertices / 9.4 k triangles) with per-vertex
  equirectangular UVs and Lambert day/night shading from the real subsolar point,
- 3D satellite markers and the focused satellite's **true ECI orbit ellipse**
  (drawn white, occluded when it passes behind the planet, altitude exaggerated
  ×0.4 so low orbits remain readable),
- a spacecraft model for the focused satellite.

**Interaction:** drag to orbit the camera (yaw from horizontal movement, pitch
clamped to ±1.5 rad), scroll to zoom (zoom range 40–500). A manual drag releases
any location lock. Five seconds after a drag, the camera glides back to a
sun-facing view. Pitch easing runs at `k = 0.06` per frame (≈0.7 s) and the
auto-reset glide at `k = 0.03` (≈2 s).

**Follow lock:** the timezone quick-jump buttons lock the camera onto a location
while the Earth turns underneath. A crosshair and label mark the locked point.
Because the lock is anchored to Earth-fixed coordinates, unlocking resumes from
the heading the camera was actually showing, with no jump.

**Models:** ISS-family satellites (ISS / ZARYA / CSS / TIANGONG / TIANHE / MIR)
load a baked binary mesh; HST / HUBBLE use a cylinder model; everything else
falls back to a vector pictogram. See [Data and SGP4](Data-and-SGP4) for the
mesh format.

### 🗺 World Map

A 2D equirectangular map: ocean fill, a graticule (meridians at ±180/±120/±60/0
and parallels at ±60/±30/0, each labelled, equator drawn thicker), Natural Earth
110 m coastlines, and a marker with a `name / altitude` label for the focused
satellite. **No interaction** — it is a static render, with no camera state.

### 🛰 Ground Track

The focused satellite's ground track from **45 minutes in the past to 90 minutes
in the future**, sampled every 2 minutes, with the current sub-satellite point
marked by a circle and a white ring, labelled with the name and altitude.
Antimeridian crossings are skipped rather than drawn as a line across the map.
Draws its grid using the World Map helpers. If no satellite is selected the pane
shows `Select a satellite in the sidebar`.

### 📋 Detail

Live orbital elements for the focused satellite: NORAD id, group, latitude,
longitude, altitude (km), speed (km/s, derived via vis-viva with
µ = 398 600.4418 and R⊕ = 6371 km), TLE epoch, and the raw TLE lines behind a
collapsing header. If the TLE cannot be propagated it says so in red instead of
showing stale numbers.

> Note: the Detail pane reads `Utc::now()`, not the accelerated sim clock — so
> it reports the *real* current state of the satellite even while the globe is
> running at ×1000.

### 📃 Catalog

The same list the sidebar shows, in pane form.

## Which views you can actually reach

Be aware of this when reading the code: the per-pane toggle in the title bar
offers only **`3D` and `2D`** — that is, `Globe3D` and `WorldMap`. `GroundTrack`
and `Detail` are fully implemented and rendered if a pane's view is set to them,
but no control in the running UI sets that. They are reachable programmatically
(and are exercised by tests); the README's "four view types" describes the
implemented set, not four toolbar buttons.

The sidebar always uses the Catalog renderer, independently of pane views.

## Split layouts

The toolbar's `Layout:` control switches the workspace between:

| Label | Panes | Arrangement |
|---|---|---|
| `1` | 1 | single full-area viewport |
| `2H` | 2 | side by side (left / right) |
| `2V` | 2 | stacked (top / bottom) |
| `4` | 4 | 2×2 grid, order top-left, top-right, bottom-left, bottom-right |

Pane rectangles are computed in normalized `[0,1]²` space by `Layout::panes()`
and then scaled to the available area, so the layout is resolution-independent.

Switching layouts preserves panes **by index** — pane 0's satellite and camera
survive a `1 → 4` switch. One deliberate exception: pane 0 is forced from
`WorldMap` back to `Globe3D`, and the active pane resets to 0.

## Pane chrome and focus

Each pane is drawn with an 18 px title bar carrying the focused satellite's name,
plus a border: a 2 px blue border when the pane is active, a 1 px grey one
otherwise. Clicking anywhere in a pane makes it the active pane — this is the
pane that sidebar clicks and right-click "Add to Window N" actions target.

The title bar also holds the timezone quick-jump buttons (`Beijing` / `DC`) and
the `3D` / `2D` view toggle.

## The transient notice

Retargeting a pane from the row context menu flashes a short white
`Added <name> to Window N` notice in that pane's top-left corner. It fades in
over 0.45 s, holds for 2.4 s, fades out over 0.45 s, and is then gone — 3.3 s
total. Only one notice exists at a time; a newer one replaces it. Left-click
focus and the copy actions deliberately raise no notice.

---
[← Architecture](Architecture) · [Data and SGP4 →](Data-and-SGP4)
