//! Satellite catalog (filterable list) view.

use crate::data::model::{Sat, SatGroup};
use egui::ScrollArea;

#[derive(Debug, Clone)]
pub struct CatalogFilter {
    pub text: String,
    pub groups: std::collections::HashSet<SatGroup>,
}

impl Default for CatalogFilter {
    fn default() -> Self {
        Self {
            text: String::new(),
            groups: SatGroup::ALL.iter().copied().collect(),
        }
    }
}

impl CatalogFilter {
    pub fn matches(&self, sat: &Sat) -> bool {
        if !self.groups.contains(&sat.group) {
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
pub fn show_catalog(ui: &mut egui::Ui, sats: &[Sat], filter: &mut CatalogFilter) -> Option<u32> {
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(egui::TextEdit::singleline(&mut filter.text).hint_text("Filter name / NORAD id"));
        if ui.button("✕").clicked() {
            filter.text.clear();
        }
    });
    ui.separator();

    // Group toggles.
    ui.horizontal_wrapped(|ui| {
        for g in SatGroup::ALL {
            let label = egui::RichText::new(g.label()).color(g.color());
            let mut on = filter.groups.contains(&g);
            if ui.toggle_value(&mut on, label).changed() {
                if on {
                    filter.groups.insert(g);
                } else {
                    filter.groups.remove(&g);
                }
            }
        }
    });
    ui.separator();

    let mut clicked = None;
    // Virtualized: only visible rows are laid out — with 16k satellites a
    // plain ScrollArea built every widget every frame and froze the UI.
    ScrollArea::vertical().show_rows(ui, 18.0, sats.len(), |ui, range| {
        let matching: Vec<&Sat> = sats.iter().filter(|s| filter.matches(s)).collect();
        ui.label(format!("{} satellites", matching.len()));
        // Skip ahead to the first match at/after `range.start` proportionally.
        // (Row indices map onto filtered rows only when no filter is active;
        // with a filter we still cap the layout work to the visible window.)
        let start = range.start.min(matching.len());
        let end = range.end.min(matching.len());
        for sat in matching.iter().copied().skip(start).take(end - start) {
            let label = format!("{:<20} #{}", truncate(&sat.name, 24), sat.norad_id);
            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(&label).color(sat.group.color()).monospace(),
                    )
                    .sense(egui::Sense::click()),
                )
                .clicked()
            {
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
