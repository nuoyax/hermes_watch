# Data and SGP4

## Data sources

TLEs come from Celestrak's GP endpoint, `FORMAT=tle`, one request per group.
Six sources, all issued **concurrently** at startup and every 2 hours:

| Source name | Celestrak group |
|---|---|
| `Celestrak-Active` | `active` |
| `Celestrak-Station` | `stations` |
| `Celestrak-Weather` | `weather` |
| `Celestrak-GPS` | `gps-ops` |
| `Celestrak-Science` | `science` |
| `Celestrak-Geo` | `geo` |

Requests carry a **browser-like `User-Agent`**; Celestrak returns HTTP 403 to
bare clients. The client timeout is 30 s.

## Group assignment

A satellite's `SatGroup` is **not parsed from its name** — it is assigned from
which source it came from, by a hardcoded map in `service.rs`:

| Source | → Group |
|---|---|
| Station | `Station` |
| Weather | `Weather` |
| GPS | `Navigation` |
| Science | `Science` |
| Active | `Other` |
| Geo | `Communications` |
| anything else | `Other` |

The eight groups are `Station`, `Weather`, `Navigation`, `Science`,
`Communications` (labelled "Comms"), `Military`, `Debris`, `Other`. Each has a
globe colour and a darkened `color_on_light()` variant — group tints are scaled
to 62 % per channel for legibility on the light sidebar, since the globe colours
are tuned for a near-black background.

`Military` and `Debris` have no data source among the six, so they consistently
show `(0)` in the category dropdown.

## TLE parsing

The parser is a pure function over the response body, and is testable without a
network. It walks the text in three-line blocks (name, line 1, line 2) and:

- requires line 1 to start with `1`, line 2 with `2`, and line 1 to be longer
  than 20 characters,
- reads the NORAD id from line 1, columns 3–7,
- silently **skips malformed blocks** rather than failing the whole source.

Two element values are pulled directly out of the raw lines when needed:

- `Tle::epoch()` — line 1, columns 18–32, with the standard 2-digit year pivot
  (≥ 57 means 1900s, otherwise 2000s); returns `(year, fractional day-of-year)`.
- `Tle::mean_motion_revs_per_day()` — line 2, columns 53–63.

## Propagation

Propagation is delegated to the **`sgp4` crate, version 2.2**. `orbit.rs` wraps
it in a `Propagator` that caches one `sgp4::Constants` per NORAD id (built on
demand from the TLE) behind a `parking_lot::RwLock<HashMap<u32, Arc<Constants>>>`.

Public surface:

| Function | Returns |
|---|---|
| `Propagator::new()` | an empty propagator |
| `subpoint(&sat, time)` | `Option<GeoPoint>` — geodetic sub-satellite point |
| `ground_track(&sat, time, past_min, future_min, step_min)` | `Vec<GeoPoint>` |
| `orbit_eci(&sat, time, past_min, future_min, step_min)` | `Vec<[f64; 3]>` — TEME positions in km |
| `all_positions(&sats, time)` | `Vec<Option<GeoPoint>>` — used by the map view |
| `gmst_deg(time)` | GMST in degrees (free function) |

`GeoPoint` is `{ lat_deg, lon_deg, alt_km }`, all `f64`.

The app calls these with `past = 45 min`, `future = 90 min`, `step = 2 min` for
ground tracks and orbit rings. The 3D globe additionally caches its orbit ring
per pane — keyed by NORAD id, a wall-clock timestamp, and the sim-time centre of
the ring — and re-propagates only when the satellite changes, the cache goes
stale, or the sim clock drifts too far from that centre.

## Frames and the Earth model

Positions come out of SGP4 as **TEME/ECI** kilometre vectors. `orbit.rs`
converts them to geodetic-ish lat/lon/alt with a **spherical** Earth:

- altitude: `r − 6371.0` km,
- latitude: `asin(z / r)` — geocentric, not geodetic (no flattening),
- longitude: TEME right ascension minus GMST.

There is no WGS84 ellipsoid and no obliquity/ECEF rotation matrix. For a
visualisation this is a deliberate simplification, but do not treat the reported
latitude as an ellipsoidal geodetic latitude.

## GMST — the single source of truth

Earth's spin phase is computed **once**, in `orbit::gmst_deg`, and everything
else delegates to it (`globe3d::earth_rotation` included). This is load-bearing:
the formula was previously duplicated in two places and both copies carried the
same rate bug — a per-century coefficient misapplied, producing 0.9878°/day
instead of 360.9856°/day, i.e. Earth turning at the wrong speed.

The implementation is the IAU 1982 / Meeus polynomial,
`280.46061837 + 360.98564736629·d + 0.000387933·t²` with `d` days since
J2000 and `t = d / 36525`, normalised with `rem_euclid(360)`.

A related bug class lives in the Julian-date helpers: two old copies each added
a ±0.5 day in opposite directions, putting the Earth half a day's spin off.
`julian_day` returns the JD at 00:00 UT and **callers must not add `−0.5`**.
The unit tests pin the reference value `num_days_from_ce(1970-01-01) == 719163`
→ JD 2440587.5, and check GMST against astropy values.

## Assets

| File | Size | Used by | What it is |
|---|---|---|---|
| `earth_day.jpg` | ~1.39 MB | 3D globe | Earth day texture, 4096×2048, resized to 2048×1024 on upload with mipmaps |
| `assets/iss_model_nasa.bin` | ~526 KB | 3D globe | Baked ISS mesh, decimated to 28 k triangles |
| `coastline110.json` | ~237 KB | World Map / Ground Track | Natural Earth 110 m coastline geometry |
| `land110.json` | ~237 KB | **nothing** | present in the repo but unreferenced by `src/` |
| `stars_bright.json` | ~167 KB | **nothing** | present in the repo but unreferenced by `src/` |

Images and the mesh are embedded at compile time with `include_bytes!`, so the
release binary is self-contained — there is no runtime asset directory.

**Mesh format** (`iss_model_nasa.bin`): a 24-byte header of
`[f32 cx, cy, cz; f32 ext; u32 nverts; u32 ntris]`, followed by `nverts` × 3
`f32` positions (already centred and divided by `ext`), then `ntris` ×
`[u32 a, b, c, u8 r, g, b]`.

## Known gaps

These are real properties of the current implementation, not TODOs left in code:

- **No retry or backoff** on a failed source. The next scheduled refresh is the
  next attempt.
- **No de-duplication across refreshes.** Each successful fetch appends to the
  catalog; a long-running session accumulates duplicates. (The catalog is
  rebuilt from scratch only on a process restart.)
- **The Earth model is spherical**, as described above.

---
[← Views](Views) · [Development →](Development)
