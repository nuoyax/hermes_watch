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
    pub earth: crate::ui::views::globe3d::Earth,
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
            earth: crate::ui::views::globe3d::Earth::load(),
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
        let mut first_batch_done = false;
        while let Ok(FetchMsg::SourceDone { source, result }) = self.fetch_rx.try_recv() {
            match result {
                Ok(n) => {
                    tracing::info!("{source}: {} sats", n.len());
                    first_batch_done = true;
                }
                Err(e) => tracing::warn!("{source}: {e}"),
            }
        }

        // First data arrived but no pane has a satellite yet: seed each pane
        // with a well-known satellite so every window shows something.
        if first_batch_done && self.panes.iter().all(|p| p.focus_norad.is_none()) {
            self.seed_default_sats();
        }

        // Periodic refresh every 2 h.
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
    /// Give each pane a default satellite: prefer famous ones (ISS first),
    /// then fall back to whatever is in the catalog. Deterministic, not random.
    fn seed_default_sats(&mut self) {
        const PREFERRED: &[&str] = &[
            "ISS (ZARYA)", "CSS (TIANHE)", "HST", "NOAA 19",
        ];
        let catalog = self.catalog.read();
        let mut picks: Vec<u32> = Vec::new();
        for want in PREFERRED {
            if let Some(sat) = catalog.iter().find(|s| s.name.contains(want)) {
                picks.push(sat.norad_id);
            }
        }
        // Fill remaining panes with distinct entries from interesting groups.
        for sat in catalog.iter() {
            if picks.len() >= self.panes.len() {
                break;
            }
            if !picks.contains(&sat.norad_id)
                && matches!(
                    sat.group,
                    crate::data::model::SatGroup::Station
                        | crate::data::model::SatGroup::Navigation
                        | crate::data::model::SatGroup::Weather
                        | crate::data::model::SatGroup::Science
                )
            {
                picks.push(sat.norad_id);
            }
        }
        // Last resort: any entries.
        for sat in catalog.iter() {
            if picks.len() >= self.panes.len() {
                break;
            }
            if !picks.contains(&sat.norad_id) {
                picks.push(sat.norad_id);
            }
        }
        drop(catalog);

        for (pane, norad) in self.panes.iter_mut().zip(picks) {
            pane.focus_norad = Some(norad);
        }
        self.selected = self.panes[0].focus_norad;
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
                    if ui.selectable_label(self.layout == l, l.label()).clicked() {
                        self.layout = l;
                        self.panes = (0..l.pane_count())
                            .map(|i| {
                                self.panes.get(i).cloned().map(|mut p| {
                                    if i == 0 && p.view == ViewKind::WorldMap {
                                        p.view = ViewKind::Globe3D;
                                    }
                                    p
                                })
                                .unwrap_or_default()
                            })
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

                for (i, norm_rect) in self.layout.panes().into_iter().enumerate() {
                    let pixels = egui::Rect::from_min_size(
                        area.min + egui::Vec2::new(
                            norm_rect.min.x * area.width(),
                            norm_rect.min.y * area.height(),
                        ),
                        egui::Vec2::new(norm_rect.width() * area.width(), norm_rect.height() * area.height()),
                    );
                    let pane = self.panes[i].clone();
                    // Each pane tracks exactly one satellite.
                    let focus_sat = pane.focus_norad.and_then(|n| self.sat_by_norad(n));
                    let title = match &focus_sat {
                        Some(sat) => format!("{} — {}", pane.view.label(), sat.name),
                        None => format!("{} — (select a satellite)", pane.view.label()),
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
                    // View lock buttons in the pane's title bar (jump globe to a
                    // timezone's longitude).
                    let tz_jumped = title_bar_buttons(ctx, pixels, &mut self.panes[i].globe);
                    let _ = tz_jumped;
                    match pane.view {
                        ViewKind::Globe3D => {
                            // Interaction: drag to rotate, scroll to zoom.
                            let resp = child.allocate_rect(child.max_rect(), egui::Sense::click_and_drag());
                            if resp.dragged() {
                                self.panes[i].globe.drag(resp.drag_delta());
                            }
                            if let Some(hover) = resp.hover_pos() {
                                let scroll = child.input(|i| i.smooth_scroll_delta.y);
                                if scroll != 0.0 && child.max_rect().contains(hover) {
                                    self.panes[i].globe.zoom(1.0 + scroll / 600.0);
                                }
                            }

                            // Only the focused satellite + its orbit ring.
                            let now = chrono::Utc::now();
                            let focus_orbit = focus_sat.as_ref().map(|sat| {
                                self.prop.ground_track(sat, now, -95.0, 95.0, 3.0)
                            }).unwrap_or_default();
                            let focus_pos = focus_sat.as_ref().and_then(|sat| {
                                self.prop.subpoint(sat, now)
                            });
                            let sun_dir = views::globe3d::sun_direction(now);
                            let earth_rot = views::globe3d::earth_rotation(now);
                            let painter = child.painter().clone();
                            views::globe3d::show_globe(
                                &painter,
                                child.max_rect(),
                                &mut self.panes[i].globe,
                                &mut self.earth,
                                sun_dir,
                                earth_rot,
                                focus_sat.as_ref(),
                                &focus_orbit,
                                focus_pos,
                            );
                        }
                        ViewKind::WorldMap => {
                            let painter = child.painter().clone();
                            if let Some(sat) = &focus_sat {
                                let pos = self.prop.subpoint(sat, chrono::Utc::now());
                                if let Some(p) = &pos {
                                    views::show_world_map_full(&painter, child.max_rect(), sat, p);
                                } else {
                                    views::show_world_map(&painter, child.max_rect(), None);
                                }
                            } else {
                                views::show_world_map(&painter, child.max_rect(), None);
                            }
                        }
                        ViewKind::GroundTrack => {
                            if let Some(sat) = &focus_sat {
                                let painter = child.painter().clone();
                                views::show_ground_track(&painter, child.max_rect(), sat, &self.prop);
                            } else {
                                child.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label("Select a satellite in the sidebar");
                                });
                            }
                        }
                        ViewKind::Detail => {
                            if let Some(sat) = &focus_sat {
                                views::show_detail(&mut child, sat, &self.prop);
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

                    // 3D/2D switch in the pane's top-right corner.
                    let switched = view_toggle(ctx, pixels, pane.view);
                    if let Some(new_view) = switched {
                        self.panes[i].view = new_view;
                    }
                }
            });
    }
}

/// Invisible click hotspot in the pane's top-right corner to cycle views.
/// Timezone quick-jump buttons drawn in the pane title bar (left of 3D/2D).
/// Clicking rotates the globe so that region faces the viewer.
fn title_bar_buttons(ctx: &egui::Context, pane_rect: egui::Rect, globe: &mut crate::ui::views::globe3d::GlobeState) -> bool {
    // ASCII labels (egui's default font has no CJK glyphs — CJK shows as tofu).
    const ZONES: &[(&str, f64)] = &[("Beijing", 116.4), ("DC", -77.0)];
    let mut jumped = false;
    let y = pane_rect.min.y + 2.0;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("tz-buttons-layer"),
    ));
    let mut x = pane_rect.max.x - 62.0 - 4.0;
    for (label, lon) in ZONES.iter().rev() {
        let w = 44.0;
        x -= w + 4.0;
        let rect = egui::Rect::from_min_size(egui::Pos2::new(x, y), egui::Vec2::new(w, 14.0));
        let mouse_in = ctx
            .input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p)));
        let clicked = mouse_in && ctx.input(|i| i.pointer.any_click());
        painter.rect_filled(
            rect,
            3.0,
            if mouse_in {
                egui::Color32::from_rgb(70, 90, 130)
            } else {
                egui::Color32::from_rgb(55, 58, 66)
            },
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(9.0),
            egui::Color32::from_rgb(200, 205, 215),
        );
        if clicked {
            // Face that longitude toward the viewer AND keep following it:
            // set a "locked longitude" the globe tracks while auto-spinning.
            globe.lock_lon = Some(*lon);
            globe.last_interaction = Some(std::time::Instant::now());
            jumped = true;
        }
    }
    jumped
}

/// 3D/2D toggle buttons in the pane's top-right title bar.
fn view_toggle(ctx: &egui::Context, pane_rect: egui::Rect, current: ViewKind) -> Option<ViewKind> {    let y = pane_rect.min.y + 2.0;
    let btn = |x: f32, label: &'static str, target: ViewKind| -> (egui::Rect, bool, bool) {
        let rect = egui::Rect::from_min_size(egui::Pos2::new(x, y), egui::Vec2::new(28.0, 14.0));
        let mouse_in = ctx
            .input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p)));
        let clicked = mouse_in && ctx.input(|i| i.pointer.any_click());
        let active = current == target;
        (rect, active, clicked)
    };

    let b3 = btn(pane_rect.max.x - 62.0, "3D", ViewKind::Globe3D);
    let b2 = btn(pane_rect.max.x - 32.0, "2D", ViewKind::WorldMap);

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("view-toggle-layer"),
    ));
    for (rect, active, _) in [b3, b2] {
        painter.rect_filled(
            rect,
            3.0,
            if active {
                egui::Color32::from_rgb(70, 110, 180)
            } else {
                egui::Color32::from_rgb(55, 58, 66)
            },
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if std::ptr::eq(&rect, &b3.0) { "3D" } else { "2D" },
            egui::FontId::proportional(10.0),
            if active { egui::Color32::WHITE } else { egui::Color32::from_rgb(160, 160, 170) },
        );
    }
    if b3.2 {
        Some(ViewKind::Globe3D)
    } else if b2.2 {
        Some(ViewKind::WorldMap)
    } else {
        None
    }
}
