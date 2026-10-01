//! Main application state and eframe glue.

use crate::ui::panes;
use crate::ui::views;

use crate::data::model::Sat;
use crate::orbit::Propagator;
use crate::service::{FetchMsg, FetchStatus};
use panes::{Layout, Pane, ViewKind};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use views::catalog::{truncate, CatalogFilter, RowAction};
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
    /// Catalog satellites that fail to propagate as of `invalid_last_scan`:
    /// unresolvable TLE epoch, no SGP4 constants, or a `propagate` error. The
    /// sidebar paints these red — they have no position in the globe, the map
    /// or their ground track, so the row warns before the user selects one.
    invalid: HashSet<u32>,
    /// When `invalid` was last recomputed. Rescanning 16k satellites every
    /// frame costs a full SGP4 pass per frame; three minutes is far tighter
    /// than the element sets' drift.
    invalid_last_scan: std::time::Instant,
    /// Catalog length `invalid` was computed against — a fetch that lands a new
    /// source forces an immediate rescan instead of waiting out the interval.
    invalid_catalog_len: usize,
    /// The most recent "Added ... to Window N" notice: which pane it belongs
    /// to and the frozen text. Only one is kept — clicking several rows in a
    /// row replaces the notice instead of stacking texts that fade together.
    toast: Option<Toast>,
}

/// A transient notice drawn in the top-left of the pane a satellite was added
/// to. The text is fixed when the notice is raised, so a later selection cannot
/// rewrite a notice that is still fading.
/// Deliberately small: it is a confirmation the eye catches in passing, not a
/// label competing with the pane's own title bar.
const TOAST_FONT_SIZE: f32 = 10.0;

struct Toast {
    text: String,
    /// 0-based index of the pane this notice belongs to — only that pane draws
    /// it.
    pane: usize,
    born: std::time::Instant,
}

impl Toast {
    /// Fade in, hold, fade out — `None` once the whole sequence is over, which
    /// is the caller's cue to drop the notice.
    fn alpha(&self, now: std::time::Instant) -> Option<f32> {
        const FADE: f32 = 0.45; // in and out, each
        const HOLD: f32 = 2.4; // fully opaque
        let total = HOLD + 2.0 * FADE;
        let t = now.duration_since(self.born).as_secs_f32();
        if t >= total {
            None
        } else if t < FADE {
            Some(t / FADE)
        } else if t < FADE + HOLD {
            Some(1.0)
        } else {
            Some(((total - t) / FADE).clamp(0.0, 1.0))
        }
    }
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
            earth: crate::ui::views::globe3d::Earth::load(),
            last_refresh: std::time::Instant::now(),
            sim_time: chrono::Utc::now(),
            speed: 60.0,
            sim_last_frame: std::time::Instant::now(),
            catalog_version: std::cell::Cell::new(0),
            catalog_snapshot: std::cell::RefCell::new(Vec::new()),
            invalid: HashSet::new(),
            invalid_last_scan: std::time::Instant::now()
                - std::time::Duration::from_secs(3600),
            invalid_catalog_len: usize::MAX,
            toast: None,
        }
    }

    /// Frame construction without a window: every `App` field is owned state
    /// that needs neither a GPU nor a real pointer source. The tests build from
    /// here to drive the real `sidebar`/`content` frames headlessly, which
    /// `new` forbids — it insists on an eframe `CreationContext`, and eframe
    /// offers no public way to construct one outside a running event loop.
    #[cfg(test)]
    pub(crate) fn new_detached(
        catalog: Arc<RwLock<Vec<Sat>>>,
        status: Arc<RwLock<FetchStatus>>,
        runtime: Arc<tokio::runtime::Runtime>,
        fetch_config: Arc<RwLock<crate::data::fetch::FetchConfig>>,
    ) -> Self {
        let (_fetch_tx, fetch_rx) = tokio::sync::mpsc::unbounded_channel();
        App {
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
            earth: crate::ui::views::globe3d::Earth::load(),
            last_refresh: std::time::Instant::now(),
            sim_time: chrono::Utc::now(),
            speed: 60.0,
            sim_last_frame: std::time::Instant::now(),
            catalog_version: std::cell::Cell::new(0),
            catalog_snapshot: std::cell::RefCell::new(Vec::new()),
            invalid: HashSet::new(),
            invalid_last_scan: std::time::Instant::now()
                - std::time::Duration::from_secs(3600),
            invalid_catalog_len: usize::MAX,
            toast: None,
        }
    }

    /// Triangle: which pane holds which satellite, and which view each pane is
    /// in. The TASK-025 acceptance evidence is a before/after diff of this.
    #[cfg(test)]
    pub(crate) fn pane_state(&self) -> Vec<(Option<u32>, ViewKind)> {
        self.panes
            .iter()
            .map(|p| (p.focus_norad, p.view))
            .collect()
    }

    /// Test-only setter for the triangle above (schema: panes in index order).
    #[cfg(test)]
    pub(crate) fn set_pane_state(&mut self, state: Vec<(Option<u32>, ViewKind)>) {
        assert_eq!(state.len(), self.panes.len(), "one entry per pane");
        for (pane, (norad, view)) in self.panes.iter_mut().zip(state) {
            pane.focus_norad = norad;
            pane.view = view;
        }
    }

    /// Look up one satellite in a catalog slice the caller already holds.
    ///
    /// Takes the slice rather than reading the shared lock itself: `parking_lot`
    /// `RwLock` is not reentrant, so calling this while `content` already holds
    /// the read guard deadlocks the UI thread the moment a fetcher queues a
    /// write. That was the "not responding" hang.
    fn sat_by_norad(sats: &[Sat], norad: u32) -> Option<Sat> {
        sats.iter().find(|s| s.norad_id == norad).cloned()
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

    /// Point the pane the user acted on at `norad`. Every other pane keeps its
    /// own satellite: panes are independent windows, and syncing them was the
    /// reported bug ("不要同步").
    fn focus_sat_in_pane(&mut self, pane: usize, norad: u32) {
        self.selected = Some(norad);
        self.panes[pane].focus_norad = Some(norad);
    }

    /// Sidebar click: "watch THIS satellite in the pane I am working in".
    ///
    /// The sidebar has no pane of its own, so it cannot name a pane; the pane
    /// the user last clicked does, and that is `active_pane` — which only became
    /// trustworthy in TASK-025, because until then a click on a 3D pane could
    /// not activate it at all (see the widget order in `content`). The old code
    /// read the same `active_pane`, so a sidebar row went to pane 0 in practice.
    ///
    /// The former "bring the active pane back to 3D" companion is deliberately
    /// **gone**: it rewrote `panes[active_pane].view`, throwing away the chosen
    /// view of a pane the user had not touched. A pane's view is the user's
    /// choice from the `3D`/`2D` buttons; picking a satellite must only change
    /// *which* satellite a pane shows.
    fn focus_from_sidebar(&mut self, norad: u32) {
        self.focus_sat_in_pane(self.active_pane, norad);
    }

    /// Apply one catalog-row gesture; returns text to put on the clipboard.
    ///
    /// It returns the text rather than copying it itself: the clipboard needs a
    /// `Context`, and keeping this a pure state transition is what lets the
    /// tests drive every gesture without a window. The copy paths are also the
    /// only ones that must touch no pane state at all.
    fn apply_row_action(
        &mut self,
        action: RowAction,
        now: std::time::Instant,
    ) -> Option<String> {
        match action {
            RowAction::Focus(norad) => {
                self.focus_from_sidebar(norad);
                None
            }
            RowAction::AddToPane { norad, pane } => {
                // `pane < len` is a guard, not the normal path: the menu lists
                // one entry per window that exists, computed in the same frame.
                // A stale index must do nothing rather than panic on
                // `self.panes[pane]`.
                if pane < self.panes.len() {
                    self.focus_sat_in_pane(pane, norad);
                    let name = self
                        .catalog_snapshot
                        .borrow()
                        .iter()
                        .find(|s| s.norad_id == norad)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| format!("#{norad}"));
                    self.toast = Some(Toast {
                        text: format!(
                            "Added {} to Window {}",
                            truncate(&name, 24),
                            pane + 1
                        ),
                        pane,
                        born: now,
                    });
                }
                None
            }
            RowAction::CopyNorad(norad) => Some(norad.to_string()),
            RowAction::CopyTle(norad) => {
                let sats = self.catalog_snapshot.borrow();
                sats.iter()
                    .find(|s| s.norad_id == norad)
                    .map(|s| format!("{}\n{}", s.tle.line1, s.tle.line2))
            }
        }
    }

    /// Paint one pane's notice. Returns `false` once the notice has run its
    /// course, which is the caller's cue to drop it.
    ///
    /// Painted on `pane_layer(index)`, the pane's own layer, and after all of
    /// the pane's content (the caller runs it after the view match) — later
    /// paint wins within a layer, so the notice sits on top of the globe, the
    /// map and the ground track without reaching any other pane.
    fn draw_toast(
        ctx: &egui::Context,
        index: usize,
        pixels: egui::Rect,
        toast: &Toast,
        now: std::time::Instant,
    ) -> bool {
        let Some(alpha) = toast.alpha(now) else {
            return false;
        };
        let painter = ctx.layer_painter(panes::pane_layer(index));
        let galley = painter.layout_no_wrap(
            toast.text.clone(),
            egui::FontId::proportional(TOAST_FONT_SIZE),
            egui::Color32::WHITE.gamma_multiply(alpha),
        );
        // Below the 18 px title bar, which already carries
        // "3D Globe — <name>" and would be overprinted by a notice drawn at
        // `pixels.min`.
        let pos = pixels.min + egui::Vec2::new(8.0, 26.0);
        // A translucent backing plate: white text alone disappears over the
        // bright parts of `earth_day.jpg`. Plate and text fade together.
        let plate = egui::Rect::from_min_size(pos, galley.size() + egui::vec2(8.0, 4.0));
        painter.rect_filled(
            plate,
            3.0,
            egui::Color32::from_black_alpha((150.0 * alpha) as u8),
        );
        painter.galley(pos + egui::vec2(4.0, 2.0), galley, egui::Color32::WHITE);
        true
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        // Light surface: the rows themselves are black text (see `row_color`),
        // which the dark theme this app uses elsewhere would swallow whole.
        egui::SidePanel::left("sidebar")
            .default_width(220.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(egui::Color32::from_rgb(242, 242, 245)),
            )
            .show(ctx, |ui| {
                // Render from a snapshot taken only when the catalog changed,
                // not a 16k-element clone on every frame (startup freeze).
                let version = self.catalog_version.get();
                let current = self.catalog.read().len();
                if version != current {
                    self.catalog_version.set(current);
                    *self.catalog_snapshot.borrow_mut() = self.catalog.read().clone();
                }
                self.rescan_invalid();

                let sats = self.catalog_snapshot.borrow();
                let action = show_catalog(
                    ui,
                    &sats,
                    &mut self.filter,
                    &self.invalid,
                    self.panes.len(),
                );
                drop(sats);
                if let Some(action) = action {
                    if let Some(text) = self.apply_row_action(action, std::time::Instant::now())
                    {
                        ctx.copy_text(text);
                    }
                }
            });
    }

    /// Refresh the set of satellites that cannot be propagated. A satellite is
    /// unusable when its TLE epoch does not parse, its element set yields no
    /// SGP4 constants, or `propagate` rejects the current time — exactly the
    /// cases in which every view would silently draw nothing for it.
    ///
    /// Runs after a catalog change or every `INVALID_RESCAN`, not per frame:
    /// the scan is a full SGP4 pass over the whole catalog.
    fn rescan_invalid(&mut self) {
        const INVALID_RESCAN: std::time::Duration = std::time::Duration::from_secs(180);
        let catalog_changed = self.catalog_version.get() != self.invalid_catalog_len;
        if !catalog_changed && self.invalid_last_scan.elapsed() < INVALID_RESCAN {
            return;
        }
        let now = self.sim_time;
        let sats = self.catalog_snapshot.borrow();
        self.invalid = sats
            .iter()
            .filter(|s| self.prop.subpoint(s, now).is_none())
            .map(|s| s.norad_id)
            .collect();
        self.invalid_catalog_len = self.catalog_version.get();
        self.invalid_last_scan = std::time::Instant::now();
    }

    fn content(&mut self, ctx: &egui::Context) {
        // A row gesture from a pane's own Catalog list. It is applied after the
        // panel closure: `apply_row_action` needs `&mut self`, and the closure
        // holds the catalog read guard, so acting inside it would borrow `self`
        // both ways at once.
        let mut deferred: Option<RowAction> = None;
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
                    let focus_sat = pane.focus_norad.and_then(|n| Self::sat_by_norad(sats, n));
                    // Just the satellite: the view already has a lit 3D/2D
                    // button in this same bar, so naming it again was noise.
                    let title = match &focus_sat {
                        Some(sat) => sat.name.clone(),
                        None => "(select a satellite)".to_string(),
                    };
                    let content =
                        panes::draw_pane_frame(ctx, pixels, &title, i == self.active_pane);

                    // Click to activate pane: it must live in the pane's own Ui,
                    // i.e. on the pane's own layer — hit-testing is per layer, so
                    // an activation widget left on the panel layer would be
                    // shadowed by the pane layer.
                    let mut child = panes::pane_ui_at(ctx, i, content);
                    // Whole-pane interaction surface for the 3D view: drag to
                    // rotate, scroll to zoom. Explicit per-pane Id — without it
                    // the drag interaction can be shared across panes, so
                    // dragging one window rotates all of them. It must come
                    // BEFORE the activation widget (below) yet still receives
                    // its own drags: a `click_and_drag` widget is handed a drag
                    // only once the pointer starts moving, and until it is
                    // *decidedly* dragging egui keeps the click for the next
                    // candidate (regression:
                    // `pane_activation_wins_over_the_drag_widget`).
                    let drag_resp = (pane.view == ViewKind::Globe3D).then(|| {
                        child.interact(
                            child.max_rect(),
                            egui::Id::new(("pane-drag", i)),
                            egui::Sense::click_and_drag(),
                        )
                    });
                    // Activation is registered after the drag surface but before
                    // the view's own widgets (list rows), because egui hands a
                    // press/release to the LAST click-sensing widget on the
                    // layer. TASK-025: with the drag surface coming after this
                    // one, egui gave it every click on a 3D pane, so
                    // `active_pane` could only advance from a non-3D pane and
                    // the sidebar — which reads `active_pane` — kept retargeting
                    // pane 0. Measured then: pane 3 clicked, active_pane still 0.
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
                            // The surface registered above (before the activation
                            // widget) drives the camera. `hover_pos` is `Some`
                            // only while nothing is clicked or dragged, which is
                            // what keeps a drag from also zooming.
                            if let Some(resp) = drag_resp {
                                if resp.dragged() {
                                    self.panes[i].globe.drag(resp.drag_delta());
                                }
                                if let Some(hover) = resp.hover_pos() {
                                    let scroll = child.input(|i| i.smooth_scroll_delta.y);
                                    if scroll != 0.0 && child.max_rect().contains(hover) {
                                        self.panes[i].globe.zoom(1.0 + scroll / 600.0);
                                    }
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
                            let action = show_catalog(
                                &mut child,
                                &sats,
                                &mut filter,
                                &self.invalid,
                                self.panes.len(),
                            );
                            self.filter = filter;
                            if let Some(action) = action {
                                match action {
                                    RowAction::Focus(n) => {
                                        // The row belongs to pane `i`, exactly
                                        // the pane the user is pointing at,
                                        // while `active_pane` cannot have
                                        // caught up with this frame's click on
                                        // that pane yet (the activation widget
                                        // is served from the previous pass's
                                        // rects). TASK-025 guard.
                                        self.selected = Some(n);
                                        self.panes[i].focus_norad = Some(n);
                                    }
                                    // The menu names its own target window, so
                                    // it goes through the shared path rather
                                    // than assuming pane `i` — deferred to
                                    // after the panel (see `deferred`).
                                    other => deferred = Some(other),
                                }
                            }
                        }
                    }

                    // "Added ... to Window N" notice. Drawn after the view so
                    // it lands on top of the globe/map/ground track, and on the
                    // pane's own layer so it cannot spill into another pane.
                    let toast_done = match &self.toast {
                        Some(t) if t.pane == i => {
                            !Self::draw_toast(ctx, i, pixels, t, std::time::Instant::now())
                        }
                        _ => false,
                    };
                    if toast_done {
                        self.toast = None;
                    }

                    // 3D/2D switch in the pane's top-right corner. `i` is the
                    // pane whose button was hit, which is the pane to write —
                    // `self.panes[i]` is read here rather than the `pane` clone
                    // so that a change made earlier in this iteration (the
                    // Catalog view's `focus_norad`) can never be masked by a
                    // stale copy.
                    let switched = view_toggle(ctx, pixels, self.panes[i].view);
                    if let Some(new_view) = switched {
                        self.panes[i].view = new_view;
                    }
                }
            });

        if let Some(action) = deferred {
            if let Some(text) = self.apply_row_action(action, std::time::Instant::now()) {
                ctx.copy_text(text);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::model::SatGroup;
    use crate::ui::views::catalog::CategorySel;

    /// Deterministic stand-in catalog; the TLEs are empty on purpose (SGP4 then
    /// fails, so no view does real propagation work in these tests).
    const NORADS: [u32; 8] = [25544, 33591, 43013, 48274, 20580, 25338, 27424, 27651];

    fn catalog() -> Vec<Sat> {
        NORADS
            .iter()
            .map(|&n| Sat {
                norad_id: n,
                name: format!("SAT {n}"),
                group: SatGroup::Station,
                tle: crate::data::model::Tle {
                    line1: String::new(),
                    line2: String::new(),
                },
            })
            .collect()
    }

    fn new_app() -> App {
        App::new_detached(
            Arc::new(RwLock::new(catalog())),
            Arc::new(RwLock::new(FetchStatus::default())),
            Arc::new(
                tokio::runtime::Builder::new_current_thread()
                    .build()
                    .expect("tokio runtime"),
            ),
            Arc::new(RwLock::new(crate::data::fetch::FetchConfig { proxy: None })),
        )
    }

    fn pointer(pos: egui::Pos2, pressed: bool, t: f64) -> egui::RawInput {
        egui::RawInput {
            time: Some(t),
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

    fn pane_rects(area: egui::Rect) -> Vec<egui::Rect> {
        Layout::Four
            .panes()
            .into_iter()
            .map(|r| {
                egui::Rect::from_min_size(
                    area.min + egui::Vec2::new(r.min.x * area.width(), r.min.y * area.height()),
                    egui::Vec2::new(r.width() * area.width(), r.height() * area.height()),
                )
            })
            .collect()
    }

    /// Run one real frame of the app (`App::sidebar` + `App::content`), no
    /// window and no `eframe::Frame` — hence `new_detached`.
    fn frame(app: &mut App, ctx: &egui::Context, input: egui::RawInput) -> egui::FullOutput {
        ctx.run(input, |ctx| {
            app.sidebar(ctx);
            app.content(ctx);
        })
    }

    fn side_panels(screen: egui::Rect) -> egui::Rect {
        // `SidePanel::left("sidebar")` with its default width; only used to place
        // the pointer over pane 3, so the exact split does not matter as long as
        // it is to the right of the panel.
        egui::Rect::from_min_max(egui::pos2(screen.min.x + 300.0, screen.min.y), screen.max)
    }

    /// TASK-025, the acceptance ledger: picking a satellite from the sidebar
    /// must change the target pane's satellite and NOTHING else — no other
    /// pane's satellite, and no pane's view. The old code wrote
    /// `panes[active_pane]` (pane 0 unless another pane had been clicked) and
    /// additionally forced that pane's view to `Globe3D`, so a row click
    /// retargeted a pane the user had not touched and threw away its view.
    #[test]
    fn sidebar_focus_moves_exactly_one_pane_and_no_view() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        app.active_pane = 1;
        app.set_pane_state(vec![
            (Some(NORADS[0]), ViewKind::Globe3D),
            (Some(NORADS[1]), ViewKind::WorldMap),
            (Some(NORADS[2]), ViewKind::Globe3D),
            (Some(NORADS[3]), ViewKind::Globe3D),
        ]);
        let before = app.pane_state();

        app.focus_from_sidebar(NORADS[4]);
        let after = app.pane_state();

        assert_eq!(
            after,
            vec![
                (Some(NORADS[0]), ViewKind::Globe3D),
                (Some(NORADS[4]), ViewKind::WorldMap), // the target pane, view untouched
                (Some(NORADS[2]), ViewKind::Globe3D),
                (Some(NORADS[3]), ViewKind::Globe3D),
            ],
            "sidebar focus changed more than the target pane"
        );
        // Exactly one pane differs from `before` — the target.
        let changed: Vec<usize> = before
            .iter()
            .zip(&after)
            .enumerate()
            .filter(|(_, (b, a))| b != a)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(changed, vec![1], "expected only pane 1 to change");
        assert_eq!(app.selected, Some(NORADS[4]));
    }

    /// The sidebar has no pane of its own, so its target is `active_pane` — the
    /// pane the user last clicked. This drives an end-to-end frame sequence:
    /// the pointer click on a pane, through the real `content()`, must make that
    /// pane active, and the sidebar row must then land on it.
    ///
    /// The sidebar click itself is not simulated (egui's widget registry is not
    /// reachable from a test, so the row's screen rect cannot be computed
    /// without duplicating `views::catalog`'s layout); `focus_from_sidebar` is
    /// the seam. The comment is not incidental: the click is what used to fail
    /// silently in the *other* direction, since a click on a 3D pane could not
    /// make it active at all (see
    /// `pane_activation_wins_over_the_drag_widget`), so a sidebar row always
    /// went to pane 0 no matter which pane the user had clicked.
    #[test]
    fn sidebar_focus_follows_the_last_clicked_pane() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        let screen = ctx.screen_rect();
        let area = side_panels(screen);
        let rects = pane_rects(area);
        let target_pane = 3;
        let click_at = rects[target_pane].center();

        app.panes[target_pane].view = ViewKind::WorldMap;
        app.panes[target_pane].focus_norad = Some(NORADS[0]);
        app.panes[0].focus_norad = Some(NORADS[1]);
        assert_eq!(app.active_pane, 0, "precondition: pane 0 starts active");

        frame(&mut app, &ctx, egui::RawInput::default());
        frame(&mut app, &ctx, pointer(click_at, true, 0.0));
        frame(&mut app, &ctx, pointer(click_at, false, 0.05));
        assert_eq!(
            app.active_pane, target_pane,
            "a click on pane {target_pane} did not make it the active pane, so the \
             sidebar would still target pane 0"
        );

        app.focus_from_sidebar(NORADS[5]);
        assert_eq!(app.pane_state()[0].0, Some(NORADS[1]), "pane 0 was hijacked");
        assert_eq!(
            app.pane_state()[target_pane],
            (Some(NORADS[5]), ViewKind::WorldMap),
            "the clicked pane did not take the satellite, or lost its view"
        );
        assert_eq!(
            app.pane_state()[target_pane].1,
            ViewKind::WorldMap,
            "the sidebar must not rewrite a pane's view"
        );
    }

    /// TASK-025 regression, widget-order half. egui resolves a click per layer
    /// with "last widget wins", so the 3D pane's `click_and_drag` drag widget
    /// must be registered **before** the pane-activation widget. While it came
    /// after (the pre-TASK-025 order), the drag widget took every click on a 3D
    /// pane, `active_pane` could only advance from a non-3D pane, and a sidebar
    /// click therefore landed on `panes[active_pane]` — typically pane 0.
    ///
    /// This mirrors `App::content`'s registration order exactly, so it goes red
    /// on the old code (measured: with the two calls swapped, the release frame
    /// reports no activated pane at all).
    #[test]
    fn pane_activation_wins_over_the_drag_widget() {
        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let rects = pane_rects(area);
        let target = rects[1].center();

        let frame = |input: egui::RawInput| -> Vec<bool> {
            let mut activated = vec![false; rects.len()];
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |_ui| {
                    for (i, r) in rects.iter().enumerate() {
                        let content = r.shrink(18.0);
                        let child = panes::pane_ui_at(ctx, i, content);
                        // 1) drag surface, 2) activation — the fixed order.
                        child.interact(
                            child.max_rect(),
                            egui::Id::new(("pane-drag", i)),
                            egui::Sense::click_and_drag(),
                        );
                        if child
                            .interact(content, egui::Id::new(("pane", i)), egui::Sense::click())
                            .clicked()
                        {
                            activated[i] = true;
                        }
                    }
                });
            });
            activated
        };

        frame(egui::RawInput::default());
        frame(pointer(target, true, 0.0));
        let activated = frame(pointer(target, false, 0.05));
        assert!(activated[1], "click on pane 1 did not activate pane 1");
        assert!(!activated[0], "click on pane 1 also activated pane 0");
        assert!(
            activated.iter().filter(|a| **a).count() == 1,
            "exactly one pane may be activated per click: {activated:?}"
        );
    }

    /// Right-clicking a row and picking "Add ... to Window 3" must retarget
    /// exactly that window and raise a notice naming it — leaving every other
    /// window's satellite and every window's view alone.
    #[test]
    fn add_to_pane_targets_exactly_that_window_and_raises_a_toast() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        app.set_pane_state(vec![
            (Some(NORADS[0]), ViewKind::Globe3D),
            (Some(NORADS[1]), ViewKind::WorldMap),
            (Some(NORADS[2]), ViewKind::Globe3D),
            (Some(NORADS[3]), ViewKind::Detail),
        ]);
        let before = app.pane_state();

        let clipboard = app.apply_row_action(
            RowAction::AddToPane {
                norad: NORADS[4],
                pane: 2,
            },
            std::time::Instant::now(),
        );

        assert_eq!(clipboard, None, "adding must not touch the clipboard");
        let after = app.pane_state();
        assert_eq!(after[2], (Some(NORADS[4]), ViewKind::Globe3D));
        let changed: Vec<usize> = before
            .iter()
            .zip(&after)
            .enumerate()
            .filter(|(_, (b, a))| b != a)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(changed, vec![2], "only window 3 (index 2) may change");

        let toast = app.toast.as_ref().expect("an add raises a notice");
        assert_eq!(toast.pane, 2, "the notice belongs to the target window");
        assert!(toast.text.starts_with("Added "), "{}", toast.text);
        assert!(toast.text.contains("SAT 20580"), "{}", toast.text);
        assert!(toast.text.ends_with("Window 3"), "{}", toast.text);
    }

    /// The chosen behaviour when the target window is already tracking
    /// something: replace it, and say so.
    #[test]
    fn add_to_pane_replaces_the_occupant_instead_of_refusing() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        app.panes[1].focus_norad = Some(NORADS[0]);
        app.apply_row_action(
            RowAction::AddToPane {
                norad: NORADS[5],
                pane: 1,
            },
            std::time::Instant::now(),
        );
        assert_eq!(
            app.panes[1].focus_norad,
            Some(NORADS[5]),
            "the occupant was not replaced"
        );
        assert!(
            app.toast
                .as_ref()
                .is_some_and(|t| t.text.contains("SAT 25338")),
            "the notice must name the satellite that went in"
        );
    }

    /// A pane index past the end of the layout does nothing at all — no panic,
    /// no state change, no notice.
    #[test]
    fn add_to_pane_out_of_range_is_a_no_op() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());
        let before = app.pane_state();

        let clipboard = app.apply_row_action(
            RowAction::AddToPane {
                norad: NORADS[4],
                pane: 9,
            },
            std::time::Instant::now(),
        );

        assert_eq!(clipboard, None);
        assert_eq!(app.pane_state(), before, "an out-of-range pane must not write");
        assert!(app.toast.is_none(), "and must not raise a notice");
    }

    /// "左键单击不在加入的窗口" — the left-click gesture keeps the TASK-025
    /// meaning (focus in the active window) and raises no notice.
    #[test]
    fn left_click_focus_does_not_raise_a_toast() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        app.active_pane = 3;
        let before = app.pane_state();

        let clipboard = app.apply_row_action(RowAction::Focus(NORADS[6]), std::time::Instant::now());

        assert_eq!(clipboard, None);
        assert_eq!(app.pane_state()[3].0, Some(NORADS[6]));
        assert_eq!(
            app.pane_state()
                .iter()
                .zip(&before)
                .filter(|(a, b)| a != b)
                .count(),
            1,
            "focus must move exactly one window"
        );
        assert!(
            app.toast.is_none(),
            "a plain left click is not an 'add to window' and must not notify"
        );
    }

    /// The two copy entries return the text for the clipboard and change no
    /// pane state — the paths a user hits to paste an id into a script.
    #[test]
    fn copy_actions_change_no_pane_state() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());
        let before = app.pane_state();

        assert_eq!(
            app.apply_row_action(RowAction::CopyNorad(NORADS[2]), std::time::Instant::now()),
            Some(NORADS[2].to_string())
        );
        // The stand-in catalog has empty TLE lines; the contract is "both lines,
        // newline separated", so that is what is asserted.
        assert_eq!(
            app.apply_row_action(RowAction::CopyTle(NORADS[2]), std::time::Instant::now()),
            Some("\n".to_string())
        );
        assert_eq!(app.pane_state(), before, "copying must not retarget a window");
        assert!(app.toast.is_none());
    }

    /// The notice fades in, holds opaque, fades out, then reports itself gone.
    #[test]
    fn toast_fades_in_holds_fades_and_ends() {
        let born = std::time::Instant::now();
        let toast = Toast {
            text: "Added SAT to Window 1".into(),
            pane: 0,
            born,
        };
        let at = |secs: f32| born + std::time::Duration::from_secs_f32(secs);

        // Just faded in: near zero.
        let a = toast.alpha(at(0.0)).expect("still alive at t=0");
        assert!(a < 0.05, "expected a fade-in start, got {a}");

        // Mid fade-in: strictly between.
        let a = toast.alpha(at(0.22)).expect("alive during fade-in");
        assert!(a > 0.3 && a < 0.8, "expected a partial fade-in, got {a}");

        // Hold: [FADE, FADE + HOLD) = [0.45, 2.85).
        assert_eq!(toast.alpha(at(0.45)), Some(1.0), "the hold starts at FADE");
        assert_eq!(toast.alpha(at(0.6)), Some(1.0));
        assert_eq!(toast.alpha(at(2.8)), Some(1.0), "still holding just before 2.85");
        assert!(
            toast.alpha(at(2.9)).is_some_and(|a| a < 1.0),
            "2.9s is already 0.05s into the fade-out"
        );

        // Fade-out then gone.
        let a = toast.alpha(at(3.1)).expect("alive during fade-out");
        assert!(a > 0.0 && a < 1.0, "expected a partial fade-out, got {a}");
        assert_eq!(toast.alpha(at(3.4)), None, "the notice must end");
        assert_eq!(toast.alpha(at(60.0)), None, "and stay ended");
    }

    /// The sidebar used to carry a second, never-read `HashSet<SatGroup>`
    /// (`groups_enabled`) alongside the one the filter actually used. Dropping
    /// it must not change the default: a freshly built app lists everything.
    #[test]
    fn sidebar_starts_unfiltered() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        assert_eq!(app.filter.category, CategorySel::All);
        assert!(app.filter.text.is_empty());

        let sats = app.catalog.read();
        assert_eq!(sats.len(), NORADS.len());
        assert!(
            sats.iter().all(|s| app.filter.matches(s)),
            "default filter must not hide any satellite"
        );
    }

    /// Selecting a category narrows the sidebar list to that group only.
    #[test]
    fn category_selection_filters_the_sidebar_list() {
        let mut app = new_app();
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, egui::RawInput::default());

        // The stand-in catalog is all `Station`; Weather has nothing to show.
        app.filter.category = CategorySel::One(SatGroup::Weather);
        frame(&mut app, &ctx, egui::RawInput::default());

        let sats = app.catalog.read();
        assert!(
            !sats.iter().any(|s| app.filter.matches(s)),
            "Weather selection must hide every Station satellite"
        );

        app.filter.category = CategorySel::One(SatGroup::Station);
        assert!(
            sats.iter().all(|s| app.filter.matches(s)),
            "Station selection must admit every Station satellite"
        );
    }
}
