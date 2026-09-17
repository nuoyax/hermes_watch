//! Main application state and eframe glue.

use crate::ui::panes;
use crate::ui::views;

use crate::data::model::{Sat, SatGroup};
use crate::orbit::Propagator;
use crate::service::{FetchMsg, FetchStatus};
use panes::{Layout, Pane, ViewKind};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use views::catalog::CatalogFilter;
use views::show_catalog;

pub struct App {
    pub catalog: Arc<RwLock<Vec<Sat>>>,
    pub status: Arc<RwLock<FetchStatus>>,
    pub fetch_rx: tokio::sync::mpsc::UnboundedReceiver<FetchMsg>,
    pub runtime: Arc<tokio::runtime::Runtime>,

    pub prop: Propagator,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub active_pane: usize,
    pub filter: CatalogFilter,
    pub selected: Option<u32>,
    pub groups_enabled: HashSet<SatGroup>,
    pub last_refresh: std::time::Instant,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        catalog: Arc<RwLock<Vec<Sat>>>,
        status: Arc<RwLock<FetchStatus>>,
        fetch_rx: tokio::sync::mpsc::UnboundedReceiver<FetchMsg>,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> Self {
        // Dark theme.
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(24, 26, 32);
        visuals.window_fill = egui::Color32::from_rgb(28, 30, 38);
        visuals.extreme_bg_color = egui::Color32::from_rgb(16, 18, 22);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            catalog,
            status,
            fetch_rx,
            runtime,
            prop: Propagator::new(),
            layout: Layout::Four,
            panes: (0..4).map(|_| Pane::default()).collect(),
            active_pane: 0,
            filter: CatalogFilter::default(),
            selected: None,
            groups_enabled: SatGroup::ALL.iter().copied().collect(),
            last_refresh: std::time::Instant::now(),
        }
    }

    fn sat_by_norad(&self, norad: u32) -> Option<Sat> {
        self.catalog.read().iter().find(|s| s.norad_id == norad).cloned()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain fetch messages.
        while let Ok(FetchMsg::SourceDone { source, result }) = self.fetch_rx.try_recv() {
            match result {
                Ok(n) => tracing::info!("{source}: {} sats", n.len()),
                Err(e) => tracing::warn!("{source}: {e}"),
            }
        }

        // Periodic refresh every 2 h + repaint at ~1 fps for orbit motion.
        if self.last_refresh.elapsed() > std::time::Duration::from_secs(2 * 3600) {
            self.last_refresh = std::time::Instant::now();
            let status = Arc::clone(&self.status);
            let catalog = Arc::clone(&self.catalog);
            let rt = Arc::clone(&self.runtime);
            std::thread::spawn(move || {
                // Simple re-fetch: reuse service::spawn on a fresh channel.
                let rx = crate::service::spawn(&rt, status, catalog);
                std::mem::forget(rx);
            });
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(1000));

        self.top_bar(ctx);
        self.sidebar(ctx);
        self.content(ctx);
    }
}

impl App {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("🛰 Satellite Monitor");
                ui.separator();
                ui.label("Layout:");
                for l in Layout::ALL {
                    if ui
                        .selectable_label(self.layout == l, l.label())
                        .clicked()
                    {
                        self.layout = l;
                        self.panes = (0..l.pane_count())
                            .map(|i| self.panes.get(i).cloned().unwrap_or_default())
                            .collect();
                        self.active_pane = 0;
                    }
                }
                ui.separator();
                let st = self.status.read().clone();
                ui.label(format!(
                    "Sources {}/{} · {} sats",
                    st.sources_done, st.sources_total, st.total_sats
                ));
                if st.sources_done < st.sources_total {
                    ui.spinner();
                }
                if let Some(e) = &st.last_error {
                    ui.colored_label(egui::Color32::ORANGE, "⚠").on_hover_text(e);
                }
            });
            ui.add_space(2.0);
        });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(280.0)
            .show(ctx, |ui| {
                let sats = self.catalog.read().clone();
                if let Some(norad) = show_catalog(ui, &sats, &mut self.filter) {
                    self.selected = Some(norad);
                    // Focus the active pane.
                    self.panes[self.active_pane].focus_norad = Some(norad);
                    if self.panes[self.active_pane].view == ViewKind::WorldMap {
                        self.panes[self.active_pane].view = ViewKind::GroundTrack;
                    }
                }
            });
    }

    fn content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(16, 18, 22)))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                let sats = self.catalog.read().clone();
                let positions = self.prop.all_positions(&sats, chrono::Utc::now());

                for (i, norm_rect) in self.layout.panes().into_iter().enumerate() {
                    let pixels = egui::Rect::from_min_size(
                        area.min + egui::Vec2::new(
                            norm_rect.min.x * area.width(),
                            norm_rect.min.y * area.height(),
                        ),
                        egui::Vec2::new(norm_rect.width() * area.width(), norm_rect.height() * area.height()),
                    );
                    let pane = self.panes[i].clone();
                    let title = match pane.view {
                        ViewKind::GroundTrack => {
                            let name = pane
                                .focus_norad
                                .and_then(|n| self.sat_by_norad(n))
                                .map(|s| s.name)
                                .unwrap_or_else(|| "—".into());
                            format!("{} — {}", pane.view.label(), name)
                        }
                        v => v.label().to_string(),
                    };
                    let content =
                        panes::draw_pane_frame(ctx, pixels, &title, i == self.active_pane);

                    // Click to activate pane.
                    if ui
                        .interact(pixels, egui::Id::new(("pane", i)), egui::Sense::click())
                        .clicked()
                    {
                        self.active_pane = i;
                    }

                    let mut child = panes::pane_ui_at(ui, content);
                    match pane.view {
                        ViewKind::WorldMap => {
                            let painter = child.painter().clone();
                            views::show_world_map(
                                &painter,
                                child.max_rect(),
                                &sats,
                                &positions,
                                &self.groups_enabled,
                            );
                        }
                        ViewKind::GroundTrack => {
                            if let Some(norad) = pane.focus_norad {
                                if let Some(sat) = self.sat_by_norad(norad) {
                                    let painter = child.painter().clone();
                                    views::show_ground_track(&painter, child.max_rect(), &sat, &self.prop);
                                }
                            } else {
                                child.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label("Select a satellite in the sidebar");
                                });
                            }
                        }
                        ViewKind::Detail => {
                            if let Some(norad) = pane.focus_norad {
                                if let Some(sat) = self.sat_by_norad(norad) {
                                    views::show_detail(&mut child, &sat, &self.prop);
                                }
                            } else {
                                child.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label("Select a satellite in the sidebar");
                                });
                            }
                        }
                        ViewKind::Catalog => {
                            let mut filter = self.filter.clone();
                            let clicked = show_catalog(&mut child, &sats, &mut filter);
                            self.filter = filter;
                            if let Some(n) = clicked {
                                self.selected = Some(n);
                                self.panes[i].focus_norad = Some(n);
                            }
                        }
                    }

                    // View picker: click the "⇄" hotspot in the pane's title bar to cycle views.
                    if picker_clicked(ctx, pixels) {
                        let all = ViewKind::ALL;
                        let idx = all.iter().position(|v| *v == pane.view).unwrap_or(0);
                        self.panes[i].view = all[(idx + 1) % all.len()];
                    }
                }
            });
    }
}

/// Invisible click hotspot in the pane's top-right corner to cycle views.
fn picker_clicked(ctx: &egui::Context, pane_rect: egui::Rect) -> bool {
    let rect = egui::Rect::from_min_size(
        egui::Pos2::new(pane_rect.max.x - 30.0, pane_rect.min.y + 2.0),
        egui::Vec2::new(26.0, 14.0),
    );
    let id = egui::Id::new("picker");
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("picker-layer"),
    ));
    let clicked = ctx.input(|i| i.pointer.any_click())
        && ctx.input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p)));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "⇄",
        egui::FontId::proportional(11.0),
        if ctx.input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p))) {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(140, 140, 150)
        },
    );
    let _ = id;
    clicked
}
