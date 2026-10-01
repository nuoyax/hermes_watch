//! Satellite catalog (filterable list) view.

use crate::data::model::{Sat, SatGroup};
use egui::ScrollArea;
use std::collections::HashSet;

/// Ink for an ordinary row. The sidebar paints a light surface (see
/// `App::sidebar`), so rows are black instead of being tinted by group.
const ROW_COLOR: egui::Color32 = egui::Color32::from_rgb(16, 16, 18);
/// A satellite whose element set cannot be propagated has no position to draw
/// in the globe, the map or its ground track, so the row says so up front.
const ROW_INVALID_COLOR: egui::Color32 = egui::Color32::from_rgb(192, 32, 32);
/// Fill behind a row under the pointer. Rows span the whole panel width, so the
/// hover is the only thing that shows the hit area reaches past the text.
const ROW_HOVER_COLOR: egui::Color32 = egui::Color32::from_rgb(219, 219, 226);

/// Height of one catalog row, in points. `show_rows` and the row painter must
/// agree on it, so it lives here rather than as a literal at both call sites.
pub const ROW_HEIGHT: f32 = 18.0;

/// Color of one catalog row — red when the satellite is unusable.
pub fn row_color(norad: u32, invalid: &HashSet<u32>) -> egui::Color32 {
    if invalid.contains(&norad) {
        ROW_INVALID_COLOR
    } else {
        ROW_COLOR
    }
}

/// Which category the catalog list is limited to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CategorySel {
    #[default]
    All,
    One(SatGroup),
}

impl CategorySel {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::One(g) => g.label(),
        }
    }

    /// Color for both the collapsed box and the dropdown entries. Darkened for
    /// the sidebar's light surface — `SatGroup::color` is tuned for the
    /// near-black globe and would be illegible there.
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::All => egui::Color32::from_rgb(32, 32, 40),
            Self::One(g) => g.color_on_light(),
        }
    }

    /// Does this selection admit a satellite of group `g`?
    pub fn accepts(self, g: SatGroup) -> bool {
        match self {
            Self::All => true,
            Self::One(sel) => sel == g,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CatalogFilter {
    pub text: String,
    pub category: CategorySel,
}

impl CatalogFilter {
    pub fn matches(&self, sat: &Sat) -> bool {
        if !self.category.accepts(sat.group) {
            return false;
        }
        if self.text.is_empty() {
            return true;
        }
        let t = self.text.to_lowercase();
        sat.name.to_lowercase().contains(&t)
            || sat.norad_id.to_string().contains(&t)
    }
}

/// Returns the NORAD id of a clicked row (if any).
///
/// `invalid` holds the satellites that cannot be propagated this frame; their
/// rows are drawn red.
pub fn show_catalog(
    ui: &mut egui::Ui,
    sats: &[Sat],
    filter: &mut CatalogFilter,
    invalid: &HashSet<u32>,
) -> Option<u32> {
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(egui::TextEdit::singleline(&mut filter.text).hint_text("Filter name / NORAD id"));
        if ui.button("✕").clicked() {
            filter.text.clear();
        }
    });
    ui.separator();

    // Category dropdown. One group at a time (or All) — the list below is the
    // only thing this filters; the globe and map still draw every satellite.
    let mut category = filter.category;
    // One pass over the catalog to get the per-group totals shown in the menu.
    let mut counts = [0usize; SatGroup::ALL.len()];
    for s in sats {
        if let Some(i) = SatGroup::ALL.iter().position(|g| *g == s.group) {
            counts[i] += 1;
        }
    }

    ui.horizontal(|ui| {
        ui.label("Category:");
        egui::ComboBox::from_id_salt("sidebar_category")
            .selected_text(egui::RichText::new(category.label()).color(category.color()))
            .width(150.0)
            .show_ui(ui, |ui| {
            ui.selectable_value(&mut category, CategorySel::All, format!("All ({})", sats.len()));
            for (i, g) in SatGroup::ALL.iter().enumerate() {
                let label = egui::RichText::new(format!("{} ({})", g.label(), counts[i]))
                    .color(g.color_on_light());
                ui.selectable_value(&mut category, CategorySel::One(*g), label);
            }
            });
    });
    filter.category = category;
    ui.separator();

    let mut clicked = None;
    // The rows span the panel's full width rather than hugging the text: a row
    // that only covers its label leaves a wide strip the user reads as part of
    // the list but cannot click. `set_min_width` on the child UI makes every row
    // a full-width, clickable line; the hover fill shows the real hit area.
    let row_width = ui.available_width();
    // Virtualized: only visible rows are laid out — with 16k satellites a
    // plain ScrollArea built every widget every frame and froze the UI.
    ScrollArea::vertical().show_rows(ui, ROW_HEIGHT, sats.len(), |ui, range| {
        ui.set_min_width(row_width);
        let matching: Vec<&Sat> = sats.iter().filter(|s| filter.matches(s)).collect();
        ui.label(format!("{} satellites", matching.len()));
        // Skip ahead to the first match at/after `range.start` proportionally.
        // (Row indices map onto filtered rows only when no filter is active;
        // with a filter we still cap the layout work to the visible window.)
        let start = range.start.min(matching.len());
        let end = range.end.min(matching.len());
        for sat in matching.iter().copied().skip(start).take(end - start) {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(row_width, ROW_HEIGHT), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 2.0, ROW_HOVER_COLOR);
            }
            let ink = row_color(sat.norad_id, invalid);
            let font = egui::FontId::monospace(12.0);
            let painter = ui.painter();
            // Name at the left, id pinned to the right edge: a row that only
            // reached as far as its text left a dead strip on the right that
            // looked like part of the list but belonged to nothing.
            painter.text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                truncate(&sat.name, 24),
                font.clone(),
                ink,
            );
            painter.text(
                rect.right_center(),
                egui::Align2::RIGHT_CENTER,
                format!("#{}", sat.norad_id),
                font,
                ink,
            );
            if response.clicked() {
                clicked = Some(sat.norad_id);
            }
        }
    });
    clicked
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sat(norad: u32, group: SatGroup) -> Sat {
        Sat {
            norad_id: norad,
            name: format!("SAT {norad}"),
            group,
            tle: crate::data::model::Tle {
                line1: String::new(),
                line2: String::new(),
            },
        }
    }

    /// Default is unfiltered — `All` admits every group in the enum.
    #[test]
    fn category_all_accepts_every_group() {
        let sel = CategorySel::default();
        assert_eq!(sel, CategorySel::All);
        for g in SatGroup::ALL {
            assert!(sel.accepts(g), "All should accept {g:?}");
        }
    }

    /// Picking one category narrows `matches` to exactly that group, and the
    /// text search still applies on top of it.
    #[test]
    fn category_selection_narrows_the_list_to_one_group() {
        let sats = [
            sat(25544, SatGroup::Station),
            sat(33591, SatGroup::Weather),
        ];

        let all = CatalogFilter::default();
        assert!(sats.iter().all(|s| all.matches(s)));

        let station = CatalogFilter {
            text: String::new(),
            category: CategorySel::One(SatGroup::Station),
        };
        assert!(station.matches(&sats[0]));
        assert!(!station.matches(&sats[1]));

        // Text search intersects with the category rather than replacing it.
        let hidden_by_text = CatalogFilter {
            text: "33591".into(),
            category: CategorySel::One(SatGroup::Station),
        };
        assert!(!hidden_by_text.matches(&sats[0]));
        assert!(!hidden_by_text.matches(&sats[1]));
    }

    /// Labels and colors the dropdown relies on.
    #[test]
    fn category_labels_and_colors_follow_the_group() {
        assert_eq!(CategorySel::All.label(), "All");
        let comms = CategorySel::One(SatGroup::Communications);
        assert_eq!(comms.label(), "Comms");
        assert_eq!(comms.color(), SatGroup::Communications.color_on_light());
    }

    /// Rows are black by default; only satellites marked invalid go red.
    #[test]
    fn rows_are_black_unless_the_satellite_is_invalid() {
        let none = HashSet::new();
        assert_eq!(row_color(25544, &none), ROW_COLOR);

        let invalid: HashSet<u32> = [25544].into_iter().collect();
        assert_eq!(row_color(25544, &invalid), ROW_INVALID_COLOR);
        assert_eq!(
            row_color(33591, &invalid),
            ROW_COLOR,
            "only the listed satellite turns red"
        );
        // Red must be distinguishable from the ordinary ink, or the marker is
        // invisible; and it must stay dark enough for the sidebar's light fill.
        assert_ne!(ROW_INVALID_COLOR, ROW_COLOR);
        assert!(ROW_INVALID_COLOR.r() > ROW_COLOR.r() + 80);
    }

    /// The hover fill has to be visible on the sidebar's pale surface without
    /// competing with the red invalid marker.
    #[test]
    fn hover_fill_contrasts_with_the_sidebar_surface() {
        let surface = egui::Color32::from_rgb(242, 242, 245);
        let d = |a: u8, b: u8| (a as i32 - b as i32).abs();
        assert!(d(ROW_HOVER_COLOR.r(), surface.r()) > 10, "hover fill is invisible");
        assert_ne!(ROW_HOVER_COLOR, ROW_INVALID_COLOR);
        assert_ne!(ROW_HOVER_COLOR, ROW_COLOR);
    }

    /// The sidebar is a light surface, so group tints have to be darkened or
    /// they vanish (`SatGroup::color` is tuned for the near-black globe).
    #[test]
    fn group_tints_are_darkened_for_the_light_sidebar() {
        for g in SatGroup::ALL {
            let globe = g.color();
            let sidebar = g.color_on_light();
            assert!(
                sidebar.r() <= globe.r() && sidebar.g() <= globe.g() && sidebar.b() <= globe.b(),
                "{g:?} was not darkened for the light sidebar"
            );
            // Mid-grey average luminance must clear a black-ish threshold for
            // legibility on #F2F2F5.
            let luma =
                (sidebar.r() as u32 * 2 + sidebar.g() as u32 * 5 + sidebar.b() as u32) / 8;
            assert!(luma < 140, "{g:?} is still too light on the light sidebar");
        }
        // `All` is plain ink, not a group tint.
        assert!(CategorySel::All.color().r() < 64);
    }
}
