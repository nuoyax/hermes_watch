//! Library facade so examples and tests can exercise the internals.
//!
//! Exposes the crate's reusable modules: [`data`] (models + Celestrak
//! fetching/parsing), [`orbit`] (SGP4 propagation and coordinate conversion),
//! [`service`] (the background fetch service) and [`ui`] (the egui application
//! and its views).
//!
//! `ui` must be declared here, not only under the binary target: `src/ui/`
//! holds unit tests (e.g. `ui/views/globe3d_tests.rs`) that live inside the
//! module tree. If `ui` is reachable only from `main.rs`, those tests are
//! compiled solely into the bin target and `cargo test --lib` silently skips
//! every one of them while still reporting success.
pub mod data;
pub mod orbit;
pub mod service;
pub mod ui;
