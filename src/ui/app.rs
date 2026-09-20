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
    /// Shared connection settings (toolbar-configurable).
    pub fetch_config: Arc<RwLock<crate::data::fetch::FetchConfig>>,
    /// Toolbar settings dialog state (proxy URL being edited).
    settings_open: bool,
    settings_proxy_draft: String,

    pub prop: Propagator,
    pub layout: Layout,
    pub panes: Vec<Pane>,
    pub active_pane: usize,
    pub filter: CatalogFilter,
    pub selected: Option<u32>,
    pub groups_enabled: HashSet<SatGroup>,
    pub earth: crate::ui::views::globe3d::Earth,
    pub last_refresh: std::time::Instant,
    /// Simulation clock (accelerated). Rendering & propagation use this.
    pub sim_time: chrono::DateTime<chrono::Utc>,
    /// Time-lapse multiplier (1 = real time).
    pub speed: f64,
    sim_last_frame: std::time::Instant,
    /// Catalog length at the time `catalog_snapshot` was taken (change marker).
    catalog_version: std::cell::Cell<usize>,
    /// Sidebar renders from this snapshot instead of cloning 16k sats/frame.
    catalog_snapshot: std::cell::RefCell<Vec<Sat>>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        catalog: Arc<RwLock<Vec<Sat>>>,
        status: Arc<RwLock<FetchStatus>>,
        fetch_rx: tokio::sync::mpsc::UnboundedReceiver<FetchMsg>,
        runtime: Arc<tokio::runtime::Runtime>,
        fetch_config: Arc<RwLock<crate::data::fetch::FetchConfig>>,
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
            fetch_config,
            settings_open: false,
            settings_proxy_draft: String::new(),
            prop: Propagator::new(),
            layout: Layout::Four,
            panes: (0..4).map(|_| Pane::default()).collect(),
            active_pane: 0,
            filter: CatalogFilter::default(),
            selected: None,
            groups_enabled: SatGroup::ALL.iter().copied().collect(),
            earth: crate::ui::views::globe3d::Earth::load(),
            last_refresh: std::time::Instant::now(),
            sim_time: chrono::Utc::now(),
            speed: 60.0,
            sim_last_frame: std::time::Instant::now(),
            catalog_version: std::cell::Cell::new(0),
            catalog_snapshot: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn sat_by_norad(&self, norad: u32) -> Option<Sat> {
        self.catalog.read().iter().find(|s| s.norad_id == norad).cloned()
    }

    /// Recompute this pane's orbit ring only when the satellite changed, the
    /// wall-clock cache is stale, or the sim clock drifted too far from the
    /// ring's centre — SGP4 propagation over ±95 min is expensive and was
    /// redone every frame for every pane. The ring is propagated around the
    /// sim time so the satellite marker always rides on it (with the sim
    /// clock accelerated, wall-time caching would leave it stranded).
    fn ensure_orbit(
        pane: &mut Pane,
        prop: &mut Propagator,
        sat: &Sat,
        sim_time: chrono::DateTime<chrono::Utc>,
    ) -> Vec<[f64; 3]> {
        let fresh = pane
            .orbit_cache
            .as_ref()
            .is_some_and(|(norad, t, centre, _)| {
                *norad == sat.norad_id
                    && t.elapsed().as_secs_f64() < 30.0
                    && (sim_time - *centre).num_seconds().abs() < 120
            });
        if !fresh {
            // True inertial orbit: one full revolution centred on the sim
            // time. Period comes from the TLE's mean motion so GEO (~1436
            // min) gets a complete ring too, not just LEO.
            let revs_per_day = sat.tle.mean_motion_revs_per_day().unwrap_or(14.0);
            let period_min = (1440.0 / revs_per_day).clamp(88.0, 1600.0);
            let half = period_min / 2.0 + 2.0;
            // Step scales with the period so a ring always has ~500 points:
            // 0.25 min for LEO, ~3 min for GEO — smooth everywhere.
            let step = (period_min / 500.0).clamp(0.25, 3.0);
            let track = prop.orbit_eci(sat, sim_time, -half, half, step);
            pane.orbit_cache = Some((
                sat.norad_id,
                std::time::Instant::now(),
                sim_time,
                track,
            ));
        }
        pane.orbit_cache.as_ref().unwrap().3.clone()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
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

        // Advance the simulation clock (accelerated time-lapse so Earth
        // rotation and satellite motion are visible at a glance).
        let elapsed = self.sim_last_frame.elapsed().as_secs_f64().min(0.1);
        self.sim_last_frame = frame_start;
        self.sim_time += chrono::Duration::milliseconds((elapsed * self.speed * 1000.0) as i64);
        let now = self.sim_time;

        // Periodic refresh every 2 h.
        if self.last_refresh.elapsed() > std::time::Duration::from_secs(2 * 3600) {
            self.last_refresh = std::time::Instant::now();
            let status = Arc::clone(&self.status);
            let catalog = Arc::clone(&self.catalog);
            let rt = Arc::clone(&self.runtime);
            let cfg = Arc::clone(&self.fetch_config);
            std::thread::spawn(move || {
                // Simple re-fetch: reuse service::spawn on a fresh channel.
                let rx = crate::service::spawn(&rt, status, catalog, cfg);
                std::mem::forget(rx);
            });
        }
        // Continuous repaint: smooth globe drag + live satellite motion.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        self.top_bar(ctx);
        self.settings_dialog(ctx);
        self.sim_speed_bar(ctx);
        self.sidebar(ctx);
        self.content(ctx);
        let dt = frame_start.elapsed();
        if dt.as_millis() > 50 {
            tracing::warn!("slow frame: {} ms", dt.as_millis());
        }
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
                ui.heading("🛰 Hermes Watch");
                ui.separator();
                if ui.button("⚙ Settings").clicked() {
                    self.settings_proxy_draft =
                        self.fetch_config.read().proxy.clone().unwrap_or_default();
                    self.settings_open = true;
                }
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

    /// Connection settings dialog: proxy URL, saved on "Apply & Refresh".
    fn settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("⚙ Settings")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("HTTP proxy (optional)");
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings_proxy_draft)
                        .hint_text("e.g. http://127.0.0.1:7890 — empty = direct")
                        .desired_width(320.0),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("✔ Apply && Refresh").clicked() {
                        let p = self.settings_proxy_draft.trim().to_string();
                        self.fetch_config.write().proxy =
                            if p.is_empty() { None } else { Some(p) };
                        self.refresh(ctx);
                        self.settings_open = false;
                    }
                    if ui.button("✕ Cancel").clicked() {
                        self.settings_open = false;
                    }
                });
            });
        self.settings_open = open;
    }

    /// Re-fetch all sources with the current fetch config.
    fn refresh(&mut self, _ctx: &egui::Context) {
        tracing::info!("refresh catalog (settings applied)");
        let status = Arc::clone(&self.status);
        let catalog = Arc::clone(&self.catalog);
        let cfg = Arc::clone(&self.fetch_config);
        let rt = Arc::clone(&self.runtime);
        std::thread::spawn(move || {
            let rx = crate::service::spawn(&rt, status, catalog, cfg);
            std::mem::forget(rx);
        });
    }

    /// Time-lapse speed control bar under the toolbar (×1 real time … ×1000).
    fn sim_speed_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("sim_speed_bar")
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(20, 22, 28)).inner_margin(egui::Margin::symmetric(8.0, 2.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Time-lapse:");
                    for (label, s) in [("×1", 1.0f64), ("×10", 10.0), ("×60", 60.0), ("×300", 300.0), ("×1000", 1000.0)] {
                        if ui.selectable_label((self.speed - s).abs() < 0.01, label).clicked() {
                            self.speed = s;
                        }
                    }
                    ui.separator();
                    // Recenter simulation clock to real time.
                    if ui.small_button("⏱ Now").clicked() {
                        self.sim_time = chrono::Utc::now();
                    }
                    ui.separator();
                    ui.label(format!(
                        "Sim UTC: {}",
                        self.sim_time.format("%Y-%m-%d %H:%M:%S")
                    ));
                });
            });
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .default_width(280.0)
            .show(ctx, |ui| {
                // Render from a snapshot taken only when the catalog changed,
                // not a 16k-element clone on every frame (startup freeze).
                let version = self.catalog_version.get();
                let current = self.catalog.read().len();
                if version != current {
                    self.catalog_version.set(current);
                    *self.catalog_snapshot.borrow_mut() = self.catalog.read().clone();
                }
                let sats = self.catalog_snapshot.borrow();
                if let Some(norad) = show_catalog(ui, &sats, &mut self.filter) {
                    self.selected = Some(norad);
                    // Focus the active pane.
                    self.panes[self.active_pane].focus_norad = Some(norad);
                    // Default to 3D: clicking a satellite in the sidebar
                    // brings the active pane back to the 3D globe view.
                    if self.panes[self.active_pane].view != ViewKind::Globe3D {
                        self.panes[self.active_pane].view = ViewKind::Globe3D;
                    }
                }
            });
    }

    fn content(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(16, 18, 22)))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                // Borrow the catalog instead of cloning 16k sats every frame.
                let catalog_guard = self.catalog.read();
                let sats: &Vec<Sat> = &catalog_guard;

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

                    // Click to activate pane. Registered before the view's own
                    // widgets so a click on a list row / button inside the pane
                    // still wins (same ordering as before TASK-023); it must
                    // live in the pane's own Ui, i.e. on the pane's own layer —
                    // hit-testing is per layer, so an activation widget left on
                    // the panel layer would be shadowed by the pane layer.
                    let mut child = panes::pane_ui_at(ctx, i, content);
                    if child
                        .interact(content, egui::Id::new(("pane", i)), egui::Sense::click())
                        .clicked()
                    {
                        self.active_pane = i;
                    }
                    // View lock buttons in the pane's title bar (jump globe to a
                    // timezone's longitude). No return value: the frame is
                    // repainted unconditionally (`request_repaint_after` in
                    // `update`), so a jump needs no extra repaint trigger.
                    title_bar_buttons(ctx, pixels, &mut self.panes[i].globe);
                    match pane.view {
                        ViewKind::Globe3D => {
                            // Interaction: drag to rotate, scroll to zoom.
                            // Explicit per-pane Id — without it the drag
                            // interaction can be shared across panes, so
                            // dragging one window rotates all of them.
                            let resp = child
                                .interact(child.max_rect(), egui::Id::new(("pane-drag", i)), egui::Sense::click_and_drag());
                            if resp.dragged() {
                                self.panes[i].globe.drag(resp.drag_delta());
                            }
                            if let Some(hover) = resp.hover_pos() {
                                let scroll = child.input(|i| i.smooth_scroll_delta.y);
                                if scroll != 0.0 && child.max_rect().contains(hover) {
                                    self.panes[i].globe.zoom(1.0 + scroll / 600.0);
                                }
                            }

                            // Only the focused satellite + its orbit ring
                            // (propagation cached per pane — see focus_orbit_cached).
                            let now = self.sim_time;
                            let focus_orbit = match &focus_sat {
                                Some(sat) => {
                                    let mut pane = self.panes[i].clone();
                                    let points =
                                        Self::ensure_orbit(&mut pane, &mut self.prop, sat, now);
                                    self.panes[i].orbit_cache = pane.orbit_cache;
                                    points
                                }
                                None => Vec::new(),
                            };
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
                                let pos = self.prop.subpoint(sat, self.sim_time);
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
                                views::show_ground_track(
                                    &painter,
                                    child.max_rect(),
                                    sat,
                                    &self.prop,
                                    self.sim_time,
                                );
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

/// Timezone quick-jump buttons drawn in the pane title bar (left of 3D/2D).
/// Clicking rotates the globe so that region faces the viewer and follows it;
/// clicking the SAME zone again releases the lock and eases back to the
/// default view (the active zone is highlighted). The active zone is read back
/// from `globe.lock_label`, so the button state needs no extra channel.
fn title_bar_buttons(
    ctx: &egui::Context,
    pane_rect: egui::Rect,
    globe: &mut crate::ui::views::globe3d::GlobeState,
) {
    // ASCII labels (egui's default font has no CJK glyphs — CJK shows as tofu).
    const ZONES: &[(&str, f64, f64)] = &[("Beijing", 116.4, 39.9), ("DC", -77.0, 38.9)];
    let y = pane_rect.min.y + 2.0;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("tz-buttons-layer"),
    ));
    let mut x = pane_rect.max.x - 62.0 - 4.0;
    for (label, lon, lat) in ZONES.iter() {
        let w = 44.0;
        x -= w + 4.0;
        let rect = egui::Rect::from_min_size(egui::Pos2::new(x, y), egui::Vec2::new(w, 14.0));
        let mouse_in = ctx
            .input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p)));
        let clicked = mouse_in && ctx.input(|i| i.pointer.any_click());
        // The zone currently followed by the camera: highlighted so the
        // locked state (and which zone it is) is visible in the title bar.
        let active = globe.lock_label == Some(*label);
        painter.rect_filled(
            rect,
            3.0,
            if active {
                egui::Color32::from_rgb(180, 140, 50)
            } else if mouse_in {
                egui::Color32::from_rgb(70, 90, 130)
            } else {
                egui::Color32::from_rgb(55, 58, 66)
            },
        );
        if active {
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 210, 80)),
            );
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(9.0),
            if active {
                egui::Color32::from_rgb(20, 20, 20)
            } else {
                egui::Color32::from_rgb(200, 205, 215)
            },
        );
        if clicked {
            // Same zone again → release the lock and ease back to the default
            // view; a different zone → lock onto it. `toggle_zone` keeps the
            // yaw continuous across the release (see `GlobeState::unlock`).
            globe.toggle_zone(*label, *lon, *lat);
        }
    }
}

/// 3D/2D toggle buttons in the pane's top-right title bar.
fn view_toggle(ctx: &egui::Context, pane_rect: egui::Rect, current: ViewKind) -> Option<ViewKind> {    let y = pane_rect.min.y + 2.0;
    // Returns (rect, active, clicked, label). The label travels through here so
    // the drawing loop below paints each button's own text — the earlier
    // `std::ptr::eq(&rect, &b3.0)` test compared the *copy* yielded by the
    // array `for` loop against the original tuple, which is never equal, so both
    // buttons drew "2D".
    let btn =
        |x: f32, label: &'static str, target: ViewKind| -> (egui::Rect, bool, bool, &'static str) {
            let rect = egui::Rect::from_min_size(egui::Pos2::new(x, y), egui::Vec2::new(28.0, 14.0));
            let mouse_in = ctx
                .input(|i| i.pointer.latest_pos().is_some_and(|p| rect.contains(p)));
            let clicked = mouse_in && ctx.input(|i| i.pointer.any_click());
            let active = current == target;
            (rect, active, clicked, label)
        };

    let b3 = btn(pane_rect.max.x - 62.0, "3D", ViewKind::Globe3D);
    let b2 = btn(pane_rect.max.x - 32.0, "2D", ViewKind::WorldMap);

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("view-toggle-layer"),
    ));
    for (rect, active, _, label) in [b3, b2] {
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
            label,
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
