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
        let yaw = cam.effective_yaw_for_test(std::time::Instant::now(), gmst);

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
        let rendered_before = cam.effective_yaw_for_test(std::time::Instant::now(), gmst);
        cam.current_yaw = rendered_before;

        cam.toggle_zone("Beijing", 116.4, 39.9); // toggle off
        assert!(cam.lock_lon.is_none());
        assert!(cam.lock_label.is_none());
        assert_eq!(cam.pitch_target, Some(GlobeState::DEFAULT_PITCH));

        // The next frame renders at the same heading it did while locked.
        let rendered_after = cam.effective_yaw_for_test(std::time::Instant::now(), gmst);
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
}
