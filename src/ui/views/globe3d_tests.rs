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
}
