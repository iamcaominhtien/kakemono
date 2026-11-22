//! Adapter around the hayro PDF library: content extraction, fonts and rendering.
//!
//! This is the only part of the program that knows about hayro: everything
//! else works on the plain data it returns ([`RawPage`], [`FontInfo`]) and
//! draws through [`PageRenderer`].

pub mod extract;
pub mod file;
pub mod fonts;
pub mod mathabx;
pub mod render;
pub mod svg;
pub mod symbols;

pub use extract::{Dir, Glyph, ImageBox, PathBox, RawPage};
pub use file::PdfFile;
pub use fonts::{FontInfo, FontRegistry};
pub use render::{MAX_RASTER_PIXELS, PageRenderer, PngCompression, declared_size, svg_to_png, within_pixel_cap};
pub use svg::{parse_svg, sanitize_svg, svg_png_size, svg_to_pdf};
