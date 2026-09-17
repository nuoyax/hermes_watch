#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod data;
mod orbit;
mod service;
mod ui;

use anyhow::Result;
use parking_lot::RwLock;
use std::sync::Arc;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Local proxy for Celestrak (direct TLS revocation check fails offline).
    std::env::set_var("SAT_PROXY", "http://127.0.0.1:7890");

    // Async runtime for fetching (runs on its own threads, not the UI thread).
    let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?);

    let catalog = Arc::new(RwLock::new(Vec::<data::model::Sat>::new()));
    let status = Arc::new(RwLock::new(service::FetchStatus::default()));

    // Kick off initial fetch.
    let fetch_rx = service::spawn(&runtime, Arc::clone(&status), Arc::clone(&catalog));

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Satellite Monitor")
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Satellite Monitor",
        opts,
        Box::new(move |cc| {
            Ok(Box::new(ui::app::App::new(
                cc,
                catalog,
                status,
                fetch_rx,
                runtime,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
