//! Split-pane layout: 1 / 2 / 4 windows, each an independent viewport.

use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};

pub mod globe3d {
    pub use crate::ui::views::globe3d::GlobeState;
}

/// How many panes and their rects within the content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    One,
    TwoHorizontal,
    TwoVertical,
    Four,
}

impl Layout {
    pub const ALL: [Layout; 4] = [
        Layout::One,
        Layout::TwoHorizontal,
        Layout::TwoVertical,
        Layout::Four,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Layout::One => "1",
            Layout::TwoHorizontal => "2H",
            Layout::TwoVertical => "2V",
            Layout::Four => "4",
        }
    }

    pub fn pane_count(self) -> usize {
        match self {
            Layout::One => 1,
            Layout::TwoHorizontal | Layout::TwoVertical => 2,
            Layout::Four => 4,
        }
    }

    /// Rects in normalized [0,1] x [0,1] space.
    pub fn panes(self) -> Vec<Rect> {
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        match self {
            Layout::One => vec![full],
            Layout::TwoHorizontal => vec![
                Rect::from_min_size(Pos2::ZERO, Vec2::new(0.5, 1.0)),
                Rect::from_min_size(Pos2::new(0.5, 0.0), Vec2::new(0.5, 1.0)),
            ],
            Layout::TwoVertical => vec![
                Rect::from_min_size(Pos2::ZERO, Vec2::new(1.0, 0.5)),
                Rect::from_min_size(Pos2::new(0.0, 0.5), Vec2::new(1.0, 0.5)),
            ],
            Layout::Four => vec![
                Rect::from_min_size(Pos2::ZERO, Vec2::new(0.5, 0.5)),
                Rect::from_min_size(Pos2::new(0.5, 0.0), Vec2::new(0.5, 0.5)),
                Rect::from_min_size(Pos2::new(0.0, 0.5), Vec2::new(0.5, 0.5)),
                Rect::from_min_size(Pos2::new(0.5, 0.5), Vec2::new(0.5, 0.5)),
            ],
        }
    }
}

/// What a pane displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// 3D interactive globe with live satellites and orbit ring.
    Globe3D,
    /// 3D-ish orbit globe (equirectangular map with ground tracks).
    WorldMap,
    /// Ground track of one focused satellite.
    GroundTrack,
    /// Satellite list / table.
    Catalog,
    /// Telemetry detail of the selected satellite.
    Detail,
}

impl ViewKind {
    pub const ALL: [ViewKind; 5] = [
        ViewKind::Globe3D,
        ViewKind::WorldMap,
        ViewKind::GroundTrack,
        ViewKind::Catalog,
        ViewKind::Detail,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewKind::Globe3D => "3D Globe",
            ViewKind::WorldMap => "World Map",
            ViewKind::GroundTrack => "Ground Track",
            ViewKind::Catalog => "Catalog",
            ViewKind::Detail => "Detail",
        }
    }
}

/// Per-pane state.
#[derive(Debug, Clone)]
pub struct Pane {
    pub view: ViewKind,
    /// Selected NORAD id for GroundTrack / Detail views.
    pub focus_norad: Option<u32>,
    /// Camera for the 3D globe view.
    pub globe: globe3d::GlobeState,
    /// Cached orbit ring: (norad, computed_at_wall, sim_time_centre, points).
    /// Re-propagated when the satellite changes, the wall-clock cache is
    /// stale, or the sim clock drifts too far from the ring's centre.
    pub orbit_cache: Option<(
        u32,
        std::time::Instant,
        chrono::DateTime<chrono::Utc>,
        Vec<[f64; 3]>,
    )>,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            view: ViewKind::Globe3D,
            focus_norad: None,
            globe: globe3d::GlobeState::default(),
            orbit_cache: None,
        }
    }
}

/// Draw pane chrome (border + title bar) inside `rect` on the painter.
///
/// TASK-023: this stays on the single shared `pane-frame` layer. The content
/// layer exists but nothing painted on it can reach the title bar (`content`
/// starts 18 px below `rect.min`), so the chrome keeps its place by being
/// drawn on a different layer rather than by z-order inside one — panes do not
/// overlap, so frame order between them is irrelevant.
pub fn draw_pane_frame(
    ctx: &egui::Context,
    rect: Rect,
    title: &str,
    active: bool,
) -> Rect {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("pane-frame"),
    ));
    let border = if active {
        Stroke::new(2.0, Color32::from_rgb(100, 160, 255))
    } else {
        Stroke::new(1.0, Color32::from_rgb(70, 70, 80))
    };
    painter.rect_stroke(rect, 4.0, border);

    // Title bar
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 18.0));
    painter.rect_filled(bar, 4.0, Color32::from_rgb(40, 44, 52));
    painter.text(
        bar.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.0),
        Color32::from_rgb(200, 200, 210),
    );

    // Content area below title bar
    Rect::from_min_max(
        Pos2::new(rect.min.x, rect.min.y + 18.0),
        rect.max,
    )
}

/// The paint layer a pane's CONTENT lives on. Every pane needs its own: the 3D
/// globe keys its per-pane mesh cache by `painter.layer_id()` (see
/// `views::globe3d::show_globe`), so panes sharing one LayerId share one cache
/// slot. With the pre-TASK-023 `Ui::new_child` (which *clones* the parent
/// painter, LayerId included) all four panes collapsed onto a single slot and
/// an idle pane replayed whichever pane drew last — the constant flicker
/// between the panes ("光怎么突然在闪").
pub fn pane_layer(index: usize) -> egui::LayerId {
    egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new(("pane-content", index)),
    )
}

/// A pane's content UI, sized to the absolute-pixel rect `pixels`, painting on
/// the pane's OWN layer (`pane_layer(index)`).
///
/// Built with `Ui::new` rather than `Ui::new_child`: a child inherits the
/// parent's painter, and egui 0.29 offers no public way to move a child's
/// *widget registration* to another layer — `Ui::with_layer_id` redirects only
/// the painting, leaving hit-testing (drag routing) on the parent's layer, and
/// `Ui::painter` is read-only. A top-level `Ui` keeps painting and interaction
/// on the same, correctly-set layer.
///
/// `move_to_top` lifts that layer above the panel background for hit-testing:
/// the pane's layer becomes its own hit-test "area", and interaction from the
/// previous pass is routed to the top-most area under the pointer (all panes
/// keep receiving their own drags because their rects do not overlap).
pub fn pane_ui_at(ctx: &egui::Context, index: usize, pixels: Rect) -> Ui {
    let layer = pane_layer(index);
    ctx.move_to_top(layer);
    Ui::new(
        ctx.clone(),
        layer,
        egui::Id::new(("pane-content-ui", index)),
        egui::UiBuilder::new().max_rect(pixels),
    )
}
