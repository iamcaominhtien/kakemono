//! Text patterns shared by the analysis of a PDF, the reading of a Word
//! document and the outputs.

use regex::Regex;
use std::sync::LazyLock;

/// The label at the start of a caption ("Figure 3:", "Table 2.",
/// "Extended Data Fig. 1 |", "Figure 3.18:", "Table 1.12" without
/// punctuation), emphasised in the HTML.
pub static CAP_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Supplementary\s+|Extended\s+Data\s+)?(?:Figure|Fig\.?|FIGURE|FIG\.?|Table|TABLE|Tab\.|Algorithm|ALGORITHM|Listing|Scheme|Chart|Plate)\s*(?:\d+(?:\.\d+)*[a-z]?|S\d+|[A-Z]\.?\d*(?:\.\d+)*|[IVXLC]+)(?:\s*[:.|—–-]|\s|$)").unwrap()
});
