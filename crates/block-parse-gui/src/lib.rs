//! An egui block editor for [`block_parse`] languages. [`BlockEditor`] embeds
//! in any `Ui`; the standalone window is behind the `app` feature.

pub mod bubble;
pub mod color;
pub mod dropdown;
pub mod editor;
pub mod interact;
pub mod layout;
pub mod paint;
pub mod shape;
#[cfg(feature = "snapshot")]
pub mod snapshot;
pub mod theme;
pub mod toolbar;
pub mod view;

pub use color::{Swatch, SwatchRecipe};
pub use editor::{BlockEditor, EditorEvent, EditorOptions, EditorOutput};
pub use theme::Theme;
pub use toolbar::RunToolbar;
pub use view::View;
