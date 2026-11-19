//! A Word document with all its formatting, for the layout engine
//! (`crate::layout`): what the semantic reading (`ir`, `build`) leaves out
//! because the other outputs choose their own presentation.

mod docx;
pub mod model;
mod props;
mod styles;

pub use model::FDoc;

use super::WordFormat;
use anyhow::{Result, bail};

/// Reads a Word file with its formatting.
///
/// # Errors
/// When the file is damaged, or is a `.doc` (not read with its formatting
/// yet).
pub fn read(data: &[u8], format: WordFormat) -> Result<FDoc> {
    match format {
        WordFormat::Docx => docx::read(data),
        WordFormat::Doc => bail!("the formatting of .doc files is not read yet"),
    }
}
