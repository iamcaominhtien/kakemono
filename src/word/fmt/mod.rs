//! A Word document with all its formatting, for the layout engine
//! (`crate::layout`): what the semantic reading (`ir`, `build`) leaves out
//! because the other outputs choose their own presentation.

mod contrast;
mod doc;
mod docx;
mod drawing;
mod frames;
pub mod model;
mod props;
mod sprm;
mod styles;

pub use model::FDoc;

use super::WordFormat;
use anyhow::Result;

/// Reads a Word file with its formatting.
///
/// # Errors
/// When the file is damaged.
pub fn read(data: &[u8], format: WordFormat) -> Result<FDoc> {
    match format {
        WordFormat::Docx => docx::read(data),
        WordFormat::Doc => doc::read(data),
    }
    .map(|mut doc| {
        frames::group(&mut doc);
        contrast::apply(&mut doc);
        doc
    })
}
