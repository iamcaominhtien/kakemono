//! The headers and footers of a printed sheet: Excel's codes (`&L`, `&C`
//! and `&R` for the left, centre and right sections; `&P`, `&N`, `&A`,
//! `&F`, `&D`, `&T` for the page number, page count, sheet, file, date
//! and time; `&"font,style"`, `&12`, `&B`, `&I`, `&U`, `&S`, `&K` for the
//! font, size, bold, italic, underline, strike and colour) read into the
//! lines of each section, made of pieces of text in one format.

use crate::chart::Rgb;

/// The fields a header or footer fills in.
pub(super) struct Fields<'a> {
    /// The number of the page, from 1.
    pub(super) page: usize,
    /// The number of pages of the sheet.
    pub(super) pages: usize,
    /// The name of the sheet (`&A`).
    pub(super) sheet: &'a str,
    /// The name of the workbook (`&F`).
    pub(super) file: &'a str,
    /// The date it is printed on (`&D`).
    pub(super) date: &'a str,
    /// The time it is printed at (`&T`).
    pub(super) time: &'a str,
}

/// The format of a piece of a header: what its codes changed from the
/// workbook's default font.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct PieceFmt {
    /// The font family (`&"name,style"`); `None` for the default.
    pub(super) font: Option<String>,
    /// The size in points (`&12`); `None` for the default.
    pub(super) size_pt: Option<f32>,
    /// Bold.
    pub(super) bold: bool,
    /// Italic.
    pub(super) italic: bool,
    /// Underlined.
    pub(super) underline: bool,
    /// Struck through.
    pub(super) strike: bool,
    /// The colour (`&Krrggbb`); `None` for the default.
    pub(super) color: Option<Rgb>,
}

/// A piece of a header's line in one format.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Piece {
    /// Its text.
    pub(super) text: String,
    /// Its format.
    pub(super) fmt: PieceFmt,
}

/// A line of a section: its pieces.
pub(super) type Line = Vec<Piece>;

/// The largest font size a code gives, in points (Excel's).
const MAX_SIZE_PT: f32 = 409.0;

/// What a code (the letter after an `&`) does.
enum Step {
    /// It prints this text.
    Text(String),
    /// It starts the section of this index (left, centre, right).
    Section(usize),
    /// It changed the format, or prints nothing.
    Nothing,
}

/// The lines of the left, centre and right sections of the header or
/// footer `code`, with `fields` filled in; a section without text has no
/// lines. Each section starts in the default format.
pub(super) fn sections(code: &str, fields: &Fields<'_>) -> [Vec<Line>; 3] {
    let mut out: [Vec<Line>; 3] = Default::default();
    let mut section = 1;
    let mut fmt = PieceFmt::default();
    let mut chars = code.chars().peekable();
    while let Some(ch) = chars.next() {
        let lines = &mut out[section];
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        let text = match ch {
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                lines.push(Vec::new());
                continue;
            }
            '&' => match step(&mut chars, &mut fmt, fields) {
                Step::Text(text) => text,
                Step::Section(index) => {
                    section = index;
                    fmt = PieceFmt::default();
                    continue;
                }
                Step::Nothing => continue,
            },
            _ => ch.to_string(),
        };
        let line = lines.last_mut().expect("a line was pushed");
        match line.last_mut() {
            Some(piece) if piece.fmt == fmt => piece.text.push_str(&text),
            _ => line.push(Piece { text, fmt: fmt.clone() }),
        }
    }
    for lines in &mut out {
        if lines
            .iter()
            .all(|line| line.iter().all(|piece| piece.text.trim().is_empty()))
        {
            lines.clear();
        }
    }
    out
}

/// Reads the code after an `&` from `chars`, changing `fmt` when it is a
/// format code.
fn step(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, fmt: &mut PieceFmt, fields: &Fields<'_>) -> Step {
    let Some(code) = chars.next() else {
        return Step::Nothing;
    };
    let text = match code {
        '&' => "&".to_string(),
        'L' => return Step::Section(0),
        'C' => return Step::Section(1),
        'R' => return Step::Section(2),
        'P' => fields.page.to_string(),
        'N' => fields.pages.to_string(),
        'A' => fields.sheet.to_string(),
        'F' => fields.file.to_string(),
        'D' => fields.date.to_string(),
        'T' => fields.time.to_string(),
        'B' => {
            fmt.bold = !fmt.bold;
            return Step::Nothing;
        }
        'I' => {
            fmt.italic = !fmt.italic;
            return Step::Nothing;
        }
        'U' | 'E' => {
            fmt.underline = !fmt.underline;
            return Step::Nothing;
        }
        'S' => {
            fmt.strike = !fmt.strike;
            return Step::Nothing;
        }
        '"' => {
            let name: String = chars.by_ref().take_while(|&ch| ch != '"').collect();
            font(fmt, &name);
            return Step::Nothing;
        }
        'K' => {
            let value: String = chars.by_ref().take(6).collect();
            fmt.color = rgb(&value);
            return Step::Nothing;
        }
        digit if digit.is_ascii_digit() => {
            let mut size = String::from(digit);
            while let Some(&next) = chars.peek().filter(|next| next.is_ascii_digit()) {
                size.push(next);
                chars.next();
            }
            fmt.size_pt = size.parse::<f32>().ok().map(|size| size.clamp(1.0, MAX_SIZE_PT));
            return Step::Nothing;
        }
        // A picture (&G), the path (&Z), super- and subscript (&X, &Y),
        // ...: nothing to print.
        _ => return Step::Nothing,
    };
    Step::Text(text)
}

/// Sets the font of `fmt` from a font code's `name,style`: a name of `-`
/// keeps the default; a style says bold and italic, in English or Japanese.
fn font(fmt: &mut PieceFmt, code: &str) {
    let (name, style) = code.split_once(',').unwrap_or((code, ""));
    let name = name.trim();
    if !name.is_empty() && name != "-" {
        fmt.font = Some(name.to_string());
    }
    let style = style.to_lowercase();
    if !style.is_empty() {
        fmt.bold = style.contains("bold") || style.contains("太字");
        fmt.italic = style.contains("italic") || style.contains("oblique") || style.contains("斜体");
    }
}

/// The colour of a `&K` code's six hexadecimal digits; `None` for a theme
/// colour or a damaged code.
fn rgb(value: &str) -> Option<Rgb> {
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&value[at..at + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

/// Seconds since 1970-01-01 UTC, from the system clock.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// [`unix_seconds`] where there is no system clock: 0.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn unix_seconds() -> u64 {
    0
}

/// Today's date (ISO) and time (hours and minutes), in UTC, as a header
/// prints them. On WebAssembly without a host (`wasm32-unknown-unknown`),
/// where the system clock panics, the date is 1970-01-01 00:00.
pub(super) fn now() -> (String, String) {
    let seconds = unix_seconds();
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let of_day = seconds % 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{:02}:{:02}", of_day / 3600, of_day % 3600 / 60),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields<'static> {
        Fields {
            page: 2,
            pages: 5,
            sheet: "Data",
            file: "book.xlsx",
            date: "2026-01-02",
            time: "09:30",
        }
    }

    /// The text of each line of each section.
    fn texts(code: &str) -> Vec<Vec<String>> {
        sections(code, &fields())
            .iter()
            .map(|lines| {
                lines
                    .iter()
                    .map(|line| line.iter().map(|piece| piece.text.as_str()).collect())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn sections_fill_in_their_fields() {
        assert_eq!(
            texts("&L様式第１－９号&C&P / &N ページ&R&A, &F &D &T"),
            [
                vec!["様式第１－９号".to_string()],
                vec!["2 / 5 ページ".to_string()],
                vec!["Data, book.xlsx 2026-01-02 09:30".to_string()],
            ]
        );
        // Text before any section code is centred; && is an ampersand.
        assert_eq!(texts("Q&&A")[1], ["Q&A"]);
    }

    #[test]
    fn codes_set_the_font_and_lines_break() {
        let [_, centre, _] = sections("&C&\"Aptos,Bold\"&14&KFF0000 Label\r\nnext", &fields());
        assert_eq!(centre.len(), 2);
        let piece = &centre[0][0];
        assert_eq!(piece.text, " Label");
        assert_eq!(piece.fmt.font.as_deref(), Some("Aptos"));
        assert_eq!(piece.fmt.size_pt, Some(14.0));
        assert!(piece.fmt.bold);
        assert_eq!(piece.fmt.color, Some([255, 0, 0]));
        assert_eq!(centre[1][0].text, "next");
    }

    #[test]
    fn the_date_is_iso() {
        let (date, time) = now();
        assert_eq!(date.len(), 10);
        assert_eq!(&date[4..5], "-");
        assert_eq!(time.len(), 5);
    }
}
