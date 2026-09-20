#[cfg(test)]
mod tests {
    use super::super::globe3d::{
        earth_fixed_to_world_test_hook as earth_fixed_to_world, earth_rotation, sun_direction,
        GlobeState, V3,
    };
    use crate::orbit::gmst_deg;
    use crate::ui::views::globe3d::rotate_to_cam_test_hook as rotate_to_cam;
    use chrono::{TimeZone, Utc};

    /// Regression for TASK-017 (the 365×-slow sidereal rate): the linear term
    /// is per DAY (360.98564736629°/day), so a full day of simulated time must
    /// turn the Earth almost exactly one full revolution. With the old
    /// per-century coefficient this was 0.9878°/day — this test would fail by
    /// two orders of magnitude.
    #[test]
    fn gmst_rate_is_one_revolution_per_day() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 17, 0, 0, 0).unwrap();
        let t1 = Utc.with_ymd_and_hms(2026, 9, 18, 0, 0, 0).unwrap();
        // Unwrap the 360° per sidereal day: sidereal − solar rotation per
        // solar day is 360.9856°, i.e. ~0.9856° more than a full turn.
        let per_day = (gmst_deg(t1) - gmst_deg(t0)).rem_euclid(360.0);
        assert!(
            (per_day - 0.985_647).abs() < 0.01,
            "GMST advanced {per_day}° in one day (expected that minus a full turn)"
        );
    }

    /// GMST must match the standard value (IAU 1982 / Meeus) at several
    /// instants. Reference values are astropy's `sidereal_time('mean','G')`
    /// in the UT1 scale (agreeing with the IAU 2006 chain to ~2e-5°); the
    /// polynomial used here is good to ~0.001° over 2000–2040, well inside the
    /// 0.01° tolerance. This is the anti-regression guard for the TASK-017 bug.
    #[test]
    fn gmst_matches_standard_values() {
        let cases = [
            ((2000, 1, 1, 12, 0, 0), 280.460_62),
            ((2026, 9, 17, 0, 0, 0), 355.943_51),
            ((2026, 9, 17, 12, 0, 0), 176.436_34),
            ((2030, 1, 1, 0, 0, 0), 100.691_65),
            ((2036, 6, 15, 18, 0, 0), 174.601_74),
        ];
        for ((y, m, d, h, mi, s), want) in cases {
            let t = Utc.with_ymd_and_hms(y, m, d, h, mi, s).unwrap();
            let got = gmst_deg(t);
            let diff = ((got - want + 180.0).rem_euclid(360.0)) - 180.0;
            assert!(
                diff.abs() < 0.01,
                "gmst({y}-{m:02}-{d:02} {h:02}:{mi:02} UT) = {got}°, want {want}° (diff {diff:+.4}°)"
            );
        }
    }

    /// `globe3d::earth_rotation` must be exactly `orbit::gmst_deg` in radians:
    /// the renderer and the ground-track now share one GMST, not two copies
    /// that can drift apart.
    #[test]
    fn earth_rotation_delegates_to_orbit_gmst() {
        let t = Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap();
        assert_eq!(earth_rotation(t), gmst_deg(t).to_radians());
    }

    /// The locked longitude must be horizontally centered (camera-space x ≈ 0)
    /// and on the visible disc (z > 0) when locked.
    #[test]
    fn lock_lon_faces_viewer() {
        let cam = GlobeState {
            yaw: 0.0,
            pitch: 0.35,
            zoom: 100.0,
            last_interaction: None,
            lock_lon: Some(116.4), // Beijing
            ..Default::default()
        };
        let gmst = 152.0_f64.to_radians();
        let yaw = cam.effective_yaw_for_test(gmst);

        // Beijing's surface normal at world angle (lon + gmst).
        let (lat, lon) = (39.9_f64.to_radians(), (116.4_f64 + gmst.to_degrees()).to_radians());
        let n = V3(lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin());
        let v = rotate_to_cam(n, 1.0, yaw, cam.pitch);
        assert!(v.0.abs() < 0.02, "x={}", v.0);
        assert!(v.2 > 0.2, "z={}", v.2);
    }

    /// The subsolar point (Earth-fixed) must be fully lit by the sun vector.
    #[test]
    fn subsolar_point_is_lit() {
        let t = Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap();
        let sun = sun_direction(t);
        // Subsolar lon at UTC noon ≈ 0°.
        let n = V3(1.0, 0.0, 0.0);
        let d = n.0 * sun.0 + n.1 * sun.1 + n.2 * sun.2;
        assert!(d > 0.99, "dot={d}");
    }

    /// Consistency: the mesh normal at the subsolar point, constructed EXACTLY
    /// as `show_globe` builds its vertex normals (`lo_r = lon + gmst`), must
    /// align with the lighting vector produced by `earth_fixed_to_world` — the
    /// same function, and therefore the same +GMST map, that `show_globe` uses
    /// for `sun_world`.
    ///
    /// Anchoring BOTH sides to the production map is the point: the earlier
    /// version of this test applied a self-consistent helper rotation to both
    /// operands, so it was an identity that held under EITHER sign of the
    /// rotation and could never detect the TASK-018 frame mismatch. Verified
    /// by reverse testing — restoring the old `−GMST` sign makes this red.
    #[test]
    fn lighting_frame_consistent() {
        let t = Utc.with_ymd_and_hms(2026, 9, 17, 6, 0, 0).unwrap();
        let sun_ef = sun_direction(t);
        let gmst = earth_rotation(t);
        // Subsolar lon in the Earth-fixed frame: 90°E at 06:00 UTC.
        let sub_lon = 90.0_f64.to_radians();
        // Mesh normal, built the way `show_globe` builds it: world lon = lon + gmst.
        let la_r = sun_ef.1.asin();
        let lo_r = sub_lon + gmst;
        let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());
        // Lighting vector via the production map.
        let s = earth_fixed_to_world(sun_ef, gmst);
        let d = n.0 * s.0 + n.1 * s.1 + n.2 * s.2;
        assert!(d > 0.999, "dot={d}");
    }

    /// TASK-018 regression: at the TRUE subsolar point the mesh normal must be
    /// fully lit. The frame mismatch was invisible to any test that reused one
    /// rotation for both operands, so this one deliberately constructs the
    /// normal from the mesh's own formula — `lo_r = (lon + gmst)` — and the
    /// lighting vector from the production `earth_fixed_to_world`.
    ///
    /// The offset between the two frames is `2·GMST`, so it ranges over a
    /// whole revolution within a day: a single sample can be near-aligned by
    /// coincidence (at 00:00Z on the fixture date the wrong sign is only 8.1°
    /// off). Sweeping the clock is therefore part of the test — the WORST
    /// instant in the day (`2·GMST ≡ 180°`, ~06:15Z here) is what a real
    /// terminator shows, and the bug turns it into a ≈ −1 dot product.
    #[test]
    fn subsolar_normal_aligned_with_sun_world() {
        let mut worst = (f64::MAX, 0u32);
        for hour in 0..24 {
            let t = Utc.with_ymd_and_hms(2026, 9, 17, hour, 0, 0).unwrap();
            let sun_ef = sun_direction(t);
            let gmst = earth_rotation(t);

            // The true subsolar point, recovered from the Earth-fixed sun
            // vector: latitude = declination, longitude = atan2(z, x).
            let decl = sun_ef.1.asin();
            let subsolar_lon = sun_ef.2.atan2(sun_ef.0);

            // Mesh normal at that point, using `show_globe`'s construction:
            // `lo_r = (lon + gmst).to_radians()`.
            let (la_r, lo_r) = (decl, subsolar_lon + gmst);
            let n = V3(la_r.cos() * lo_r.cos(), la_r.sin(), la_r.cos() * lo_r.sin());

            // Lighting vector via the production +GMST map (same as show_globe).
            let s = earth_fixed_to_world(sun_ef, gmst);
            let d = n.0 * s.0 + n.1 * s.1 + n.2 * s.2;
            if d < worst.0 {
                worst = (d, hour);
            }
        }
        let (d, hour) = worst;
        assert!(
            d > 0.999,
            "subsolar point not lit at {hour:02}:00Z: worst dot={d} (angle {:.3}°)",
            d.clamp(-1.0, 1.0).acos().to_degrees()
        );
    }

    /// Clicking the SAME timezone button again must release the lock and hand
    /// the camera over to the heading it was actually showing (no yaw jump).
    #[test]
    fn zone_toggle_off_keeps_yaw() {
        let gmst = 152.0_f64.to_radians();
        let mut cam = GlobeState::default();

        cam.toggle_zone("Beijing", 116.4, 39.9);
        assert_eq!(cam.lock_label, Some("Beijing"));
        assert_eq!(cam.pitch_target, Some(39.9_f64.to_radians()));

        // One rendered frame while locked: `show_globe` refreshes
        // `current_yaw` from the yaw it actually drew.
        let rendered_before = cam.effective_yaw_for_test(gmst);
        cam.current_yaw = rendered_before;

        cam.toggle_zone("Beijing", 116.4, 39.9); // toggle off
        assert!(cam.lock_lon.is_none());
        assert!(cam.lock_label.is_none());
        assert_eq!(cam.pitch_target, Some(GlobeState::DEFAULT_PITCH));

        // The next frame renders at the same heading it did while locked.
        let rendered_after = cam.effective_yaw_for_test(gmst);
        assert!(
            (rendered_after - rendered_before).abs() < 1e-9,
            "yaw jumped on unlock: {rendered_before} -> {rendered_after}"
        );
    }

    /// A manual drag still releases the lock, and drops any pending pitch ease
    /// so the user's own tilt is not fought by the easing.
    #[test]
    fn drag_releases_lock_and_cancels_pitch_ease() {
        let mut cam = GlobeState::default();
        cam.toggle_zone("DC", -77.0, 38.9);
        assert!(cam.lock_lon.is_some());
        assert!(cam.pitch_target.is_some());

        cam.drag(egui::Vec2::new(4.0, 0.0));
        assert!(cam.lock_lon.is_none());
        assert!(cam.lock_label.is_none());
        assert!(cam.pitch_target.is_none());
    }

    /// Vertical geometry of the locked point. `toggle_zone` eases the pitch to
    /// the zone's latitude (asserted here via `pitch_target`); the geometry
    /// asserted is the disc position of the point itself: on a LEVEL camera
    /// (pitch = 0) it keeps its true latitude — camera-space y == sin(lat),
    /// z == cos(lat) — with x ≈ 0 (horizontal centring).
    ///
    /// Why a level camera rather than the settled `pitch == lat`: the ease
    /// (`settle_pitch`) is private, and `rotate_to_cam` applies its pitch
    /// rotation about the world X axis BEFORE the yaw that aligns the locked
    /// meridian. Consequently the settled tilt does NOT give y == sin(lat)
    /// (measured y ≈ 0.98 for Beijing at gmst = 152°); the vertical component
    /// also picks up `cos(lat)·sin(lon_world)·sin(pitch)`. A yaw-then-pitch
    /// composition would centre the latitude (y == sin(lat - pitch), 0 at
    /// pitch == lat) — that is an implementation change, deliberately NOT made
    /// in this test task. This test pins the geometry the lock does guarantee.
    #[test]
    fn lock_latitude_keeps_true_latitude_on_disc() {
        let lat_deg = 39.9_f64; // Beijing
        let lat = lat_deg.to_radians();
        let mut cam = GlobeState::default();
        cam.toggle_zone("Beijing", 116.4, lat_deg);
        // The tilt the zone jump eases toward (== the zone's latitude).
        assert_eq!(cam.pitch_target, Some(lat));
        // Pin the vertical invariant where it is exact (see doc comment).
        cam.pitch = 0.0;

        let gmst = 152.0_f64.to_radians();
        let yaw = cam.effective_yaw_for_test(gmst);
        // Beijing's surface normal at its world angle (lon + gmst).
        let lon_w = (116.4_f64 + gmst.to_degrees()).to_radians();
        let n = V3(lat.cos() * lon_w.cos(), lat.sin(), lat.cos() * lon_w.sin());
        let v = rotate_to_cam(n, 1.0, yaw, cam.pitch);

        assert!(v.0.abs() < 0.02, "x={}", v.0);
        assert!(
            (v.1 - lat.sin()).abs() < 1e-9,
            "y={} sin(lat)={}",
            v.1,
            lat.sin()
        );
        assert!(
            (v.2 - lat.cos()).abs() < 1e-9,
            "z={} cos(lat)={}",
            v.2,
            lat.cos()
        );
    }

    /// Render one frame of `show_globe` headless and return the sphere mesh's
    /// projected vertex positions. The mesh is the only `Shape::Mesh` drawn
    /// with no satellite and no active lock.
    fn render_globe_mesh(
        ctx: &egui::Context,
        earth: &mut super::super::globe3d::Earth,
        cam: &mut GlobeState,
        layer: egui::LayerId,
        earth_rot: f64,
    ) -> Vec<egui::Pos2> {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(240.0, 240.0));
        let sun = sun_direction(Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap());
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = egui::Painter::new(ctx.clone(), layer, rect);
            super::super::globe3d::show_globe(&p, rect, cam, earth, sun, earth_rot, None, &[], None);
        });
        let mut shapes: Vec<egui::epaint::Shape> =
            out.shapes.into_iter().map(|s| s.shape).collect();
        let i = shapes
            .iter()
            .position(|s| matches!(s, egui::epaint::Shape::Mesh(_)))
            .expect("show_globe drew no mesh");
        match shapes.remove(i) {
            egui::epaint::Shape::Mesh(m) => m.vertices.iter().map(|v| v.pos).collect(),
            _ => unreachable!(),
        }
    }

    fn max_delta(a: &[egui::Pos2], b: &[egui::Pos2]) -> f32 {
        assert_eq!(a.len(), b.len(), "mesh vertex count changed");
        a.iter()
            .zip(b)
            .map(|(p, q)| (p.x - q.x).abs().max((p.y - q.y).abs()))
            .fold(0.0_f32, f32::max)
    }

    // ---- TASK-023: panes must not share a mesh-cache slot -----------------

    /// Layout for `n` panes side by side, with their centres far apart so
    /// `render_panes` can check that each returned mesh really came from the
    /// pane it is attributed to.
    fn pane_rects(n: usize) -> Vec<egui::Rect> {
        (0..n)
            .map(|i| {
                egui::Rect::from_min_size(
                    egui::pos2(320.0 * i as f32, 0.0),
                    egui::vec2(240.0, 240.0),
                )
            })
            .collect()
    }

    /// A pane camera with `auto_reset` disabled (a pending `last_drag` keeps its
    /// idle gate shut, so the yaw stays exactly where the test put it) and an
    /// old `last_interaction` (not dragging unless a frame says so).
    fn pane_cam(yaw: f64) -> GlobeState {
        let mut cam = GlobeState::default();
        cam.yaw = yaw;
        cam.last_drag = Some(std::time::Instant::now());
        cam.last_interaction =
            Some(std::time::Instant::now() - std::time::Duration::from_secs(10));
        cam
    }

    /// Draw every pane in ONE frame through the REAL pane path — the content Ui
    /// comes from `panes::pane_ui_at` inside a `CentralPanel`, exactly as
    /// `App::content` obtains it — and return each pane's sphere-mesh vertices,
    /// in pane order, with the `LayerId` each pane actually painted on.
    ///
    /// `drag[i]` marks pane i as dragging for this frame: `show_globe` rebuilds
    /// a dragging pane's mesh unconditionally, which is how a test guarantees a
    /// fresh cache entry. With `drag[i] == false` and no time passing between
    /// frames the rebuild throttle holds, so the pane replays whatever its own
    /// cache slot holds — exactly what a window the user is not touching does
    /// in the running app.
    ///
    /// `earth_rot` is constant across the frames of these tests on purpose: an
    /// idle pane replaying its own cache is then pixel-identical to its
    /// previous frame, so any mismatch is provably another pane's mesh. (A
    /// varying rotation would rebuild every pane and mask the bug.)
    fn render_panes(
        ctx: &egui::Context,
        earth: &mut super::super::globe3d::Earth,
        cams: &mut [GlobeState],
        drag: &[bool],
        earth_rot: f64,
    ) -> (Vec<Vec<egui::Pos2>>, Vec<egui::LayerId>) {
        let rects = pane_rects(cams.len());
        let sun = sun_direction(Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap());
        let mut layers = Vec::with_capacity(cams.len());
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {
                for (i, cam) in cams.iter_mut().enumerate() {
                    if drag[i] {
                        cam.last_interaction = Some(std::time::Instant::now());
                    }
                    let ui = crate::ui::panes::pane_ui_at(ctx, i, rects[i]);
                    layers.push(ui.painter().layer_id());
                    super::super::globe3d::show_globe(
                        ui.painter(),
                        rects[i],
                        cam,
                        earth,
                        sun,
                        earth_rot,
                        None,
                        &[],
                        None,
                    );
                }
            });
        });
        let meshes: Vec<Vec<egui::Pos2>> = out
            .shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::epaint::Shape::Mesh(m) => Some(m.vertices.iter().map(|v| v.pos).collect()),
                _ => None,
            })
            .collect();
        assert_eq!(meshes.len(), cams.len(), "unexpected number of sphere meshes");
        // Attribute every mesh to its pane by position (the sphere is centred on
        // its pane): without this, a shifted pairing could satisfy the
        // assertions below while the panes were misordered.
        for (i, m) in meshes.iter().enumerate() {
            let centroid =
                m.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / m.len() as f32;
            assert!(
                (centroid - rects[i].center().to_vec2()).length() < 5.0,
                "mesh {i} is not pane {i}'s (centroid {centroid:?}, pane centre {:?})",
                rects[i].center()
            );
        }
        (meshes, layers)
    }

    /// Register the pane-activation widget exactly as `App::content` does
    /// (`ui.interact(content_rect, ("pane", i), Sense::click())` on the pane's
    /// own Ui) and return which panes reported a click this frame.
    fn click_frame(
        ctx: &egui::Context,
        n: usize,
        input: egui::RawInput,
    ) -> Vec<bool> {
        let rects = pane_rects(n);
        let mut clicked = vec![false; n];
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |_ui| {
                for i in 0..n {
                    let content = rects[i];
                    let ui = crate::ui::panes::pane_ui_at(ctx, i, content);
                    if ui
                        .interact(content, egui::Id::new(("pane", i)), egui::Sense::click())
                        .clicked()
                    {
                        clicked[i] = true;
                    }
                }
            });
        });
        clicked
    }

    fn pointer_button(pos: egui::Pos2, pressed: bool, time: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(time),
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        }
    }

    /// TASK-023 guard: pane activation must still hit the pane under the
    /// pointer. The panes now paint on their own layers, which also moves their
    /// widget registration there — hit-testing is per layer (`hit_test` keeps
    /// only the top-most layer's widgets), so an activation widget left behind
    /// on the panel's layer, or a pane layer that has not been lifted above the
    /// background, would make clicks land on the wrong pane or nowhere.
    #[test]
    fn pane_click_activates_that_pane_only() {
        let ctx = egui::Context::default();
        let rects = pane_rects(2);
        let target = rects[1].center();

        // Frame 0 registers the widgets, then a press/release pair inside pane 1.
        click_frame(&ctx, 2, egui::RawInput::default());
        click_frame(&ctx, 2, pointer_button(target, true, 0.0));
        let clicked = click_frame(&ctx, 2, pointer_button(target, false, 0.05));

        assert!(clicked[1], "the pane under the pointer was not activated");
        assert!(!clicked[0], "a click on pane 1 also activated pane 0");
    }

    /// The mesh-cache key is the pane's `LayerId`; if two panes share one, the
    /// cache is not per-pane no matter what the drawing code says. On the
    /// pre-TASK-023 `Ui::new_child` path this is 1 distinct id out of 4 (`backg
    /// 5A3E` four times) and this test goes red.
    #[test]
    fn panes_paint_on_distinct_layers() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cams = vec![pane_cam(0.0), pane_cam(0.5), pane_cam(1.0), pane_cam(1.5)];
        let (_, layers) = render_panes(&ctx, &mut earth, &mut cams, &[false; 4], 0.02);
        let distinct: std::collections::HashSet<_> = layers.iter().collect();
        assert_eq!(
            distinct.len(),
            layers.len(),
            "panes share a paint layer ({} distinct of {}): {:?}",
            distinct.len(),
            layers.len(),
            layers
        );
    }

    /// One pane's picture rendered on its own, on a private layer: the reference
    /// for "what this pane should be showing". The private layer is deliberate —
    /// this is the yardstick for the measurement, not the code under test.
    fn render_isolated(
        ctx: &egui::Context,
        earth: &mut super::super::globe3d::Earth,
        cam: &mut GlobeState,
        rect: egui::Rect,
        earth_rot: f64,
    ) -> Vec<egui::Pos2> {
        let sun = sun_direction(Utc.with_ymd_and_hms(2026, 9, 17, 12, 0, 0).unwrap());
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            let p = egui::Painter::new(
                ctx.clone(),
                egui::LayerId::new(
                    egui::Order::Background,
                    egui::Id::new("task023-isolated-reference"),
                ),
                rect,
            );
            super::super::globe3d::show_globe(&p, rect, cam, earth, sun, earth_rot, None, &[], None);
        });
        let mut shapes: Vec<egui::epaint::Shape> =
            out.shapes.into_iter().map(|s| s.shape).collect();
        let i = shapes
            .iter()
            .position(|s| matches!(s, egui::epaint::Shape::Mesh(_)))
            .expect("show_globe drew no mesh");
        match shapes.remove(i) {
            egui::epaint::Shape::Mesh(m) => m.vertices.iter().map(|v| v.pos).collect(),
            _ => unreachable!(),
        }
    }

    /// Per-pane mesh cache slotting (TASK-012), driven through the REAL pane
    /// path so it tests the thing that actually broke: `panes::pane_ui_at` must
    /// give each pane a distinct egui `LayerId`, because that LayerId is the
    /// mesh-cache key. Before TASK-023 `pane_ui_at` built the pane Ui with
    /// `Ui::new_child`, which CLONES the parent painter — LayerId included — so
    /// every pane hashed to one slot and the second frame below replayed the
    /// other pane's mesh (measured 640 px off on that code; see TASK-023).
    #[test]
    fn mesh_cache_is_slotted_per_pane() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cams = vec![pane_cam(0.0), pane_cam(0.9)];

        // Frame 1 rebuilds both panes (both marked dragging), leaving one entry
        // per pane; frames 2 and 3 cannot rebuild anything (nothing drags, the
        // throttle holds), so a pane can only show whatever its slot holds.
        let (first, _) = render_panes(&ctx, &mut earth, &mut cams, &[true, true], 0.02);
        let (second, _) = render_panes(&ctx, &mut earth, &mut cams, &[false, false], 0.02);
        let (third, _) = render_panes(&ctx, &mut earth, &mut cams, &[false, false], 0.02);

        assert!(
            max_delta(&first[0], &first[1]) > 5.0,
            "test needs two visibly different panes (delta={})",
            max_delta(&first[0], &first[1])
        );
        // Pane 0 replays its OWN slot, not pane 1's...
        assert!(
            max_delta(&first[0], &second[0]) < 1e-4,
            "pane 0 replayed another pane's cached mesh (delta={})",
            max_delta(&first[0], &second[0])
        );
        // ...because a replay really happened (pane 0's picture differs from
        // pane 1's, so the assertion above would have caught the shared slot).
        assert!(
            max_delta(&second[0], &first[1]) > 5.0,
            "test needs the two slots to hold different meshes (delta={})",
            max_delta(&second[0], &first[1])
        );
        // And the replay is stable: pane 0's slot still holds pane 0's mesh.
        assert!(
            max_delta(&second[0], &third[0]) < 1e-4,
            "pane 0's own cached mesh changed between replays (delta={})",
            max_delta(&second[0], &third[0])
        );
    }

    /// The symptom the user reported ("光怎么突然在闪"): with a shared cache slot
    /// an idle pane replays the dragging pane's mesh, so it alternates between
    /// its own view and the dragger's even though nothing about it changed.
    /// Every frame the idle pane must show its own picture, and the dragging
    /// pane must not be able to affect it.
    #[test]
    fn idle_pane_keeps_its_own_view_while_another_pane_drags() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cams = vec![pane_cam(0.0), pane_cam(0.9)];
        let rects = pane_rects(2);

        // Pane 1's own correct picture, alone on a private layer.
        let mut ref_cam = pane_cam(0.9);
        let reference = render_isolated(&ctx, &mut earth, &mut ref_cam, rects[1], 0.02);

        // Frame 0: both panes build (pane 1's entry has to be its own before the
        // idle frames can be checked). Then six frames in which ONLY pane 0
        // drags — mirroring the real app, whose sim clock keeps the dragging
        // pane rebuilding every frame.
        let (first, _) = render_panes(&ctx, &mut earth, &mut cams, &[true, true], 0.02);
        assert!(
            max_delta(&first[1], &reference) < 1e-4,
            "frame 0: pane 1 did not draw its own view (delta={})",
            max_delta(&first[1], &reference)
        );
        assert!(
            max_delta(&first[0], &reference) > 5.0,
            "test needs the two panes to differ (delta={})",
            max_delta(&first[0], &reference)
        );
        for frame in 1..7 {
            let (m, _) = render_panes(&ctx, &mut earth, &mut cams, &[true, false], 0.02);
            assert!(
                max_delta(&m[1], &reference) < 1e-4,
                "idle pane showed something other than its own view on frame {frame} \
                 (delta={})",
                max_delta(&m[1], &reference)
            );
        }
    }

    /// A locked pane must look the same as the Earth turns underneath: the
    /// camera yaw absorbs `gmst`, so the mesh drawn at gmst = 0.0 and at
    /// gmst = 0.5 is the SAME picture (the locked region does not slide off).
    /// If the lock yaw ever stops tracking GMST, the mesh rotates → red.
    #[test]
    fn locked_render_stays_put_while_earth_turns() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cam = GlobeState::default();
        cam.toggle_zone("Beijing", 116.4, 39.9);
        cam.pitch = 0.0;
        cam.pitch_target = None; // hold the tilt still for this comparison

        let layer_a = egui::LayerId::new(egui::Order::Background, egui::Id::new("lock-frame-A"));
        let layer_b = egui::LayerId::new(egui::Order::Background, egui::Id::new("lock-frame-B"));
        let a = render_globe_mesh(&ctx, &mut earth, &mut cam, layer_a, 0.0);
        let b = render_globe_mesh(&ctx, &mut earth, &mut cam, layer_b, 0.5);

        assert!(
            max_delta(&a, &b) < 1e-3,
            "locked pane rotated with the Earth (delta={})",
            max_delta(&a, &b)
        );
    }

    /// Regression for the EPIC-004 unlock snap, driven through the real
    /// renderer: two locked frames are drawn 0.5 rad of earth rotation apart.
    /// `show_globe` must refresh `current_yaw` to the live heading on BOTH of
    /// them, and releasing the lock must continue from that live heading — the
    /// pre-fix code froze `current_yaw` while locked, so the unlock jumped
    /// back to the heading of the first frame.
    #[test]
    fn unlock_continues_the_live_locked_heading() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cam = GlobeState::default();
        cam.toggle_zone("Beijing", 116.4, 39.9);
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("lock-pane"));

        // Frame 1 and frame 2 while locked, 0.5 rad of earth rotation apart.
        render_globe_mesh(&ctx, &mut earth, &mut cam, layer, 0.0);
        render_globe_mesh(&ctx, &mut earth, &mut cam, layer, 0.5);
        let stale = cam.effective_yaw_for_test(0.0);
        let live = cam.effective_yaw_for_test(0.5);
        assert!((live - stale).abs() > 0.4, "frames not distinguishable");

        // `current_yaw` tracks the frame that was actually drawn...
        assert!(
            (cam.current_yaw - live).abs() < 1e-9,
            "current_yaw frozen while locked: current={} live={} stale={}",
            cam.current_yaw,
            live,
            stale
        );
        // ...and the unlock continues from it, with no jump.
        cam.toggle_zone("Beijing", 116.4, 39.9); // toggle off -> unlock
        assert!(cam.lock_lon.is_none());
        assert!(
            (cam.yaw - live).abs() < 1e-9,
            "unlock jumped: yaw={} live={} stale={}",
            cam.yaw,
            live,
            stale
        );
    }

    /// Free (unlocked) yaw is the user's yaw and nothing else: two different
    /// GMSTs must yield the *identical* value, so an untouched pane can never
    /// drift with the Earth's rotation. Exact equality is intentional here.
    #[test]
    fn idle_yaw_does_not_drift_with_gmst() {
        let cam = GlobeState {
            yaw: 0.4,
            ..Default::default()
        };
        let gmst_a = 10.0_f64.to_radians();
        let gmst_b = 200.0_f64.to_radians();
        let yaw_a = cam.effective_yaw_for_test(gmst_a);
        let yaw_b = cam.effective_yaw_for_test(gmst_b);
        assert_eq!(yaw_a, yaw_b, "idle yaw drifted: {yaw_a} vs {yaw_b}");
        assert_eq!(yaw_a, 0.4);
    }

    /// Locked yaw follows the real Earth rotation: over `gmst_a -> gmst_b` it
    /// must advance by exactly that difference — same invariant that keeps the
    /// locked city facing the viewer, checked on the yaw itself.
    #[test]
    fn locked_yaw_advances_with_earth_rotation() {
        let mut cam = GlobeState::default();
        cam.toggle_zone("Beijing", 116.4, 39.9);
        let gmst_a = 0.0_f64;
        let gmst_b = 0.5_f64; // distinguishable, per regression requirement
        let yaw_a = cam.effective_yaw_for_test(gmst_a);
        let yaw_b = cam.effective_yaw_for_test(gmst_b);
        assert!(
            (yaw_b - yaw_a).abs() > 1e-6,
            "locked yaw did not advance: {yaw_a} vs {yaw_b}"
        );
        assert!(
            ((yaw_b - yaw_a) - (gmst_b - gmst_a)).abs() < 1e-9,
            "locked yaw advanced by {} but gmst advanced by {}",
            yaw_b - yaw_a,
            gmst_b - gmst_a
        );
    }

    /// Regression for the core EPIC-004 bug: while locked, `current_yaw` must
    /// be refreshed every rendered frame, so unlocking adopts the *latest*
    /// heading rather than the one frozen when the lock started.
    #[test]
    fn unlock_adopts_latest_rendered_yaw() {
        let gmst_a = 0.0_f64;
        let gmst_b = 0.5_f64; // 0.5 rad apart: the freeze bug is visible
        let mut cam = GlobeState::default();
        cam.toggle_zone("Beijing", 116.4, 39.9);

        // Two rendered frames while locked (`show_globe` writes current_yaw
        // from the yaw it just drew, in both lock states).
        let first = cam.effective_yaw_for_test(gmst_a);
        cam.current_yaw = first;
        let second = cam.effective_yaw_for_test(gmst_b);
        cam.current_yaw = second;
        assert!((second - first).abs() > 1e-6, "test needs distinguishable frames");

        cam.unlock();
        assert!(
            (cam.yaw - second).abs() < 1e-9,
            "unlock adopted a stale yaw: yaw={} latest={} first={}",
            cam.yaw,
            second,
            first
        );
        assert!(
            (cam.yaw - first).abs() > 1e-6,
            "unlock adopted the frozen first-frame yaw ({first})"
        );
    }
}
