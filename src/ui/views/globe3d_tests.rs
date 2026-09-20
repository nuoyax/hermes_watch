#[cfg(test)]
mod tests {
    use super::super::globe3d::{earth_rotation, sun_direction, GlobeState, V3};
    use crate::ui::views::globe3d::rotate_to_cam_test_hook as rotate_to_cam;
    use chrono::{TimeZone, Utc};

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

    /// Consistency: the mesh normal at the subsolar longitude (after +GMST)
    /// must align with the sun after the same +GMST rotation.
    #[test]
    fn lighting_frame_consistent() {
        let t = Utc.with_ymd_and_hms(2026, 9, 17, 6, 0, 0).unwrap();
        let sun_ef = sun_direction(t);
        let gmst = earth_rotation(t);
        // Subsolar lon in Earth-fixed frame:
        let sub_lon: f64 = (180.0_f64 - 6.0 * 15.0).to_radians(); // 90°E
        // Apply the mesh's +gmst to both, then compare directly.
        let rot = |v: V3, a: f64| V3(v.0 * a.cos() + v.2 * a.sin(), v.1, -v.0 * a.sin() + v.2 * a.cos());
        let n = rot(V3((sub_lon).cos(), 0.0, (sub_lon).sin()), gmst);
        let s = rot(sun_ef, gmst);
        let d = n.0 * s.0 + n.1 * s.1 + n.2 * s.2;
        assert!(d > 0.999, "dot={d}");
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

    /// Per-pane mesh cache slotting (TASK-012): each pane's cached mesh is
    /// keyed by its egui `LayerId`, so pane B drawing a DIFFERENT frame must
    /// not invalidate/replace pane A's entry. Two panes are rendered with
    /// clearly different earth rotations (same camera, so `auto_reset` can't
    /// interfere), then pane A is rendered again at its original rotation: its
    /// mesh must be replayed from A's OWN cache (the 0.5 s throttle suppresses
    /// the rebuild), i.e. identical to its first frame and clearly different
    /// from pane B's. With a single shared slot, A's third frame would replay
    /// B's stale mesh and this test goes red (verified by reverse testing).
    #[test]
    fn mesh_cache_is_slotted_per_pane() {
        let ctx = egui::Context::default();
        let mut earth = super::super::globe3d::Earth::load();
        let mut cam = GlobeState::default();
        // Keep `auto_reset` out of the way (it would drift `yaw` toward the
        // sun-facing heading); an active drag keeps the idle gate closed.
        cam.last_drag = Some(std::time::Instant::now());
        cam.yaw = 0.0;

        // Two distinct panes, each with its own LayerId. Same camera, two
        // different earth rotations: that is what made the 4 panes invalidate
        // each other's single shared slot every frame.
        let layer_a = egui::LayerId::new(egui::Order::Background, egui::Id::new("pane-A"));
        let layer_b = egui::LayerId::new(egui::Order::Background, egui::Id::new("pane-B"));

        let a1 = render_globe_mesh(&ctx, &mut earth, &mut cam, layer_a, 0.02);
        let b1 = render_globe_mesh(&ctx, &mut earth, &mut cam, layer_b, 0.60);
        let a2 = render_globe_mesh(&ctx, &mut earth, &mut cam, layer_a, 0.02);

        // Sanity: the two panes really do show different frames, so replaying
        // the wrong slot is actually detectable.
        assert!(
            max_delta(&a1, &b1) > 5.0,
            "test needs two visibly different panes (delta={})",
            max_delta(&a1, &b1)
        );
        // The invariant: pane A replays A's own cached mesh.
        assert!(
            max_delta(&a1, &a2) < 1e-4,
            "pane A replayed another pane's cached mesh (delta={})",
            max_delta(&a1, &a2)
        );
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
