//! Views rendered inside panes.

pub mod catalog;
pub mod detail;
pub mod globe3d;
pub mod ground_track;
pub mod world_map;

pub use catalog::show_catalog;
pub use detail::show_detail;
pub use ground_track::show_ground_track;
pub use world_map::show_world_map;
