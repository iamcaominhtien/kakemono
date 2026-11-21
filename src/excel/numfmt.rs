//! Numbers formatted as Excel shows them in a cell: with the cell's
//! number format, built-in (by id) or custom (a format code such as
//! `#,##0.00;(#,##0.00)`).
//!
//! A format code has up to four sections, `positive;negative;zero;text`,
//! made of digit placeholders (`0`, `#`, `?`), the decimal point, `,`
//! (thousands, or a scale of 1000 after the last digit), `%`, an exponent
//! (`E+`), fractions (`?/?`), literal text (quoted, escaped with `\`, or
//! `[$€-407]`) and, for dates and times, `y m d h s AM/PM [h]`. Colours
//! and conditions (`[Red]`, `[>100]`) are left out, and so are the fill
//! (`*x`) and the padding (`_x`) of a column.
//!
//! Dates and times are always written ISO (`2024-01-31`, `14:05`,
//! `2024-01-31 14:05`, with seconds only when the code shows them),
//! whatever order and separators the code uses; `.` is the decimal point
//! and `,` the thousands separator, whatever the locale.

/// The number format of a cell.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum NumFmt {
    /// Excel's `General`: the number as it is, up to 11 significant digits.
    #[default]
    General,
    /// A format code.
    Code(String),
}

/// Significant digits of Excel's General format in a cell.
const GENERAL_DIGITS: i32 = 11;
/// Significant digits of the mantissa when General writes a number in
/// scientific notation (`1.23457E+11`).
const GENERAL_SCIENTIFIC_DIGITS: usize = 6;
/// Below this magnitude, General writes a number in scientific notation.
const GENERAL_SMALLEST: f64 = 1e-9;
/// Significant digits Excel keeps of a number before formatting it.
const EXCEL_DIGITS: usize = 15;
/// The last serial of a date: 9999-12-31 in the 1900 system.
const MAX_SERIAL_1900: f64 = 2_958_465.0;
/// The last serial of a date in the 1904 system (9999-12-31).
const MAX_SERIAL_1904: f64 = MAX_SERIAL_1900 - 1462.0;
/// The serial of 1900-02-29, a day Excel has and the calendar has not.
const LEAP_DAY_1900: i64 = 60;
/// The most digits a fraction's denominator is searched with (`???/???`).
const MAX_FRACTION_DIGITS: usize = 3;
/// Seconds in a day.
const DAY_SECONDS: i64 = 86_400;
/// Days from 1970-01-01 back to 1899-12-30, the day before serial 1 that
/// serials after the leap day of 1900 count from.
const EPOCH_1900_DAYS: i64 = -25_569;
/// Days from 1970-01-01 back to 1904-01-01, serial 0 of the 1904 system.
const EPOCH_1904_DAYS: i64 = -24_107;

/// The number format of a built-in id, as Excel (en-US) defines it, except
/// that dates and times are ISO and the currency formats have no symbol.
/// An id Excel does not define is General.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn builtin(id: u32) -> NumFmt {
    let code = match id {
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 | 6 => "#,##0_);(#,##0)",
        7 | 8 => "#,##0.00_);(#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        // The dates and times of en-US, and those of the East Asian
        // locales (27–36, 50–58), all written ISO.
        14..=17 | 27..=31 | 34..=36 | 50..=58 => "yyyy-mm-dd",
        18 | 20 | 32 => "hh:mm",
        19 | 21 | 33 => "hh:mm:ss",
        22 => "yyyy-mm-dd hh:mm",
        37 | 38 => "#,##0 ;(#,##0)",
        39 | 40 => "#,##0.00;(#,##0.00)",
        41 | 42 => "_(* #,##0_);_(* (#,##0);_(* \"-\"_);_(@_)",
        43 | 44 => "_(* #,##0.00_);_(* (#,##0.00);_(* \"-\"??_);_(@_)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return NumFmt::General,
    };
    NumFmt::Code(code.to_string())
}

/// Whether `fmt` shows a date or a time.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_date(fmt: &NumFmt) -> bool {
    match fmt {
        NumFmt::General => false,
        NumFmt::Code(code) => sections(code).first().is_some_and(|tokens| is_date_section(tokens)),
    }
}

/// `value` as Excel shows it with the format `fmt`; `date1904` when the
/// workbook counts dates from 1904.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn format(value: f64, fmt: &NumFmt, date1904: bool) -> String {
    let NumFmt::Code(code) = fmt else {
        return general(value);
    };
    if !value.is_finite() {
        return general(value);
    }
    let sections = sections(code);
    if sections.first().is_some_and(|tokens| is_date_section(tokens)) {
        return date_time(value, &sections[0], date1904).unwrap_or_else(|| general(value));
    }
    // Which section shows the value, and whether it writes the minus sign
    // itself (a negative section does, by its literal text).
    let (tokens, minus) = match (sections.len(), value) {
        (0, _) => return general(value),
        (1, _) => (&sections[0], value < 0.0),
        (_, negative) if negative < 0.0 => (&sections[1], false),
        (2, _) => (&sections[0], false),
        (_, 0.0) => (&sections[2], false),
        _ => (&sections[0], false),
    };
    if tokens.is_empty() {
        return String::new();
    }
    let text = number(value.abs(), tokens);
    // A value that rounds to zero is written without a minus sign.
    let shows_digit = text.bytes().any(|byte| byte.is_ascii_digit() && byte != b'0');
    let sign = if minus && shows_digit { "-" } else { "" };
    format!("{sign}{}", text.trim())
}

/// One piece of a section of a format code.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// Text written as it is.
    Literal(String),
    /// A digit placeholder: `0` (a digit, zero when there is none), `#`
    /// (a digit, nothing when there is none) or `?` (a digit, a space).
    Digit(char),
    /// The decimal point.
    Point,
    /// A comma: thousands separators, or a scale of 1000 after the digits.
    Comma,
    /// `%`: the number times 100.
    Percent,
    /// The exponent: `E+` (sign always written) or `E-` (only the minus).
    Exponent { plus: bool },
    /// The bar of a fraction.
    Slash,
    /// `General` inside a code.
    General,
    /// A date or time part.
    Date(DatePart),
}

/// A date or time part of a format code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatePart {
    /// `yy` or `yyyy`.
    Year,
    /// `m`: the month, or the minutes after hours or before seconds.
    MonthOrMinute,
    /// `d` or `dd` (`ddd` and `dddd`, the weekday, too).
    Day,
    /// `h` or `hh`.
    Hour,
    /// `s` or `ss`.
    Second,
    /// `AM/PM` or `A/P`.
    AmPm,
    /// `[h]`, `[mm]` or `[ss]`: a duration, whose hours go past 24.
    Elapsed,
}

/// The sections of `code`, each as its tokens.
fn sections(code: &str) -> Vec<Vec<Token>> {
    let chars: Vec<char> = code.chars().collect();
    let mut sections = vec![Vec::new()];
    let mut at = 0;
    while at < chars.len() {
        let ch = chars[at];
        let tokens = sections.last_mut().expect("there is always a section");
        at += 1;
        match ch {
            ';' => sections.push(Vec::new()),
            '"' | '[' => {
                let close = if ch == '"' { '"' } else { ']' };
                let len = chars[at..]
                    .iter()
                    .position(|&next| next == close)
                    .unwrap_or(chars.len() - at);
                let inside: String = chars[at..at + len].iter().collect();
                at += len + 1;
                if ch == '"' {
                    tokens.push(Token::Literal(inside));
                } else {
                    tokens.extend(bracket(&inside));
                }
            }
            '\\' => {
                if let Some(&next) = chars.get(at) {
                    tokens.push(Token::Literal(next.to_string()));
                }
                at += 1;
            }
            // Padding to the width of a character, and filling the cell
            // with one: not text.
            '_' | '*' => at += 1,
            '0' | '#' | '?' => tokens.push(Token::Digit(ch)),
            '.' => tokens.push(Token::Point),
            ',' => tokens.push(Token::Comma),
            '%' => tokens.push(Token::Percent),
            '/' => tokens.push(Token::Slash),
            '@' => {}
            'E' | 'e' if matches!(chars.get(at), Some('+' | '-')) => {
                tokens.push(Token::Exponent { plus: chars[at] == '+' });
                at += 1;
            }
            _ => {
                let (token, len) = word(&chars[at - 1..]);
                tokens.push(token);
                at += len - 1;
            }
        }
    }
    sections
}

/// The token of a bracketed part (without its brackets): the symbol of a
/// currency (`$€-407`), an elapsed time (`h`, `mm`), or nothing (a colour,
/// a condition, a locale).
fn bracket(inside: &str) -> Option<Token> {
    if let Some(currency) = inside.strip_prefix('$') {
        let symbol = currency.split('-').next().unwrap_or_default();
        return (!symbol.is_empty()).then(|| Token::Literal(symbol.to_string()));
    }
    let elapsed = !inside.is_empty()
        && inside
            .chars()
            .all(|letter| matches!(letter.to_ascii_lowercase(), 'h' | 'm' | 's'));
    elapsed.then_some(Token::Date(DatePart::Elapsed))
}

/// The token at the start of `chars` (`General`, `AM/PM`, a date part such
/// as `yyyy`, or one literal character) and how many characters it takes.
fn word(chars: &[char]) -> (Token, usize) {
    for (name, token) in [
        ("general", Token::General),
        ("am/pm", Token::Date(DatePart::AmPm)),
        ("a/p", Token::Date(DatePart::AmPm)),
    ] {
        if starts_with(chars, name) {
            return (token, name.len());
        }
    }
    let ch = chars[0];
    let part = match ch.to_ascii_lowercase() {
        'y' | 'e' => DatePart::Year,
        'm' => DatePart::MonthOrMinute,
        'd' => DatePart::Day,
        'h' => DatePart::Hour,
        's' => DatePart::Second,
        _ => return (Token::Literal(ch.to_string()), 1),
    };
    // A run of the same letter (`yyyy`) is one part.
    let len = chars.iter().take_while(|next| next.eq_ignore_ascii_case(&ch)).count();
    (Token::Date(part), len)
}

/// Whether `chars` starts with `word`, ignoring case.
fn starts_with(chars: &[char], word: &str) -> bool {
    chars.len() >= word.len()
        && word
            .chars()
            .zip(chars)
            .all(|(letter, ch)| ch.eq_ignore_ascii_case(&letter))
}

/// Whether a section shows a date or a time.
fn is_date_section(tokens: &[Token]) -> bool {
    tokens.iter().any(|token| matches!(token, Token::Date(_)))
}

/// The date parts of a section, with each `m` read as the month or the
/// minutes: minutes right after hours or right before seconds.
fn date_parts(tokens: &[Token]) -> Vec<DatePart> {
    let parts: Vec<DatePart> = tokens
        .iter()
        .filter_map(|token| match token {
            Token::Date(part) => Some(*part),
            _ => None,
        })
        .collect();
    parts
        .iter()
        .enumerate()
        .map(|(at, &part)| {
            let after_hours = at > 0 && matches!(parts[at - 1], DatePart::Hour | DatePart::Elapsed);
            let before_seconds = parts.get(at + 1) == Some(&DatePart::Second);
            if part == DatePart::MonthOrMinute && (after_hours || before_seconds) {
                // Minutes are a time part, as hours are.
                DatePart::Hour
            } else {
                part
            }
        })
        .collect()
}

/// A serial as an ISO date, time, or both, as the section shows it; `None`
/// when the serial is not a date (negative, or past 9999-12-31).
fn date_time(value: f64, tokens: &[Token], date1904: bool) -> Option<String> {
    let max = if date1904 { MAX_SERIAL_1904 } else { MAX_SERIAL_1900 };
    if value < 0.0 || value >= max + 1.0 {
        return None;
    }
    let parts = date_parts(tokens);
    let has_date = parts
        .iter()
        .any(|part| matches!(part, DatePart::Year | DatePart::MonthOrMinute | DatePart::Day));
    let has_seconds = parts.contains(&DatePart::Second);
    let has_time = has_seconds
        || parts
            .iter()
            .any(|part| matches!(part, DatePart::Hour | DatePart::AmPm | DatePart::Elapsed));
    let elapsed = parts.contains(&DatePart::Elapsed);
    // The serial in seconds, rounded to what the code shows.
    let unit: i64 = if has_seconds {
        1
    } else if has_time {
        60
    } else {
        1
    };
    #[allow(clippy::cast_possible_truncation)] // below 2958466 days
    let seconds = ((value * DAY_SECONDS as f64 / unit as f64).round() as i64) * unit;
    let (days, time) = (seconds.div_euclid(DAY_SECONDS), seconds.rem_euclid(DAY_SECONDS));
    let mut out = Vec::new();
    if has_date {
        let (year, month, day) = serial_date(days, date1904)?;
        out.push(format!("{year:04}-{month:02}-{day:02}"));
    }
    if has_time {
        let hours = if elapsed { seconds / 3600 } else { time / 3600 };
        let mut clock = format!("{hours:02}:{:02}", time % 3600 / 60);
        if has_seconds {
            clock.push_str(&format!(":{:02}", time % 60));
        }
        out.push(clock);
    }
    Some(out.join(" "))
}

/// The calendar date of a day serial.
fn serial_date(days: i64, date1904: bool) -> Option<(i64, u32, u32)> {
    let unix_days = if date1904 {
        EPOCH_1904_DAYS + days
    } else if days == LEAP_DAY_1900 {
        return Some((1900, 2, 29));
    } else if days < LEAP_DAY_1900 {
        // Before the leap day Excel invented, serials count from one day
        // later.
        EPOCH_1900_DAYS + days + 1
    } else {
        EPOCH_1900_DAYS + days
    };
    let date = civil(unix_days);
    (date.0 <= 9999).then_some(date)
}

/// The date of a count of days from 1970-01-01, in the Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let doe = shifted.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 1..=31, 1..=12
    (year, month as u32, day as u32)
}

/// `value` in Excel's General format: up to 11 significant digits,
/// scientific notation for very large or very small numbers.
fn general(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if !value.is_finite() {
        return value.to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let value = value.abs();
    let exponent = decimal_exponent(value);
    if exponent >= GENERAL_DIGITS || value < GENERAL_SMALLEST {
        let (mantissa, exponent) = scientific(value, GENERAL_SCIENTIFIC_DIGITS - 1, 1);
        let mantissa = strip_zeros(&mantissa);
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{mantissa}E{exp_sign}{:02}", exponent.abs());
    }
    #[allow(clippy::cast_sign_loss)] // clamped
    let decimals = (GENERAL_DIGITS - 1 - exponent).max(0) as usize;
    let (int, frac) = digits(value, decimals);
    let int = if int.is_empty() { "0".to_string() } else { int };
    format!("{sign}{}", strip_zeros(&format!("{int}.{frac}")))
}

/// `text` (`1.2500`) without trailing zeros after the point, nor the point
/// itself when nothing is left after it.
fn strip_zeros(text: &str) -> String {
    if !text.contains('.') {
        return text.to_string();
    }
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The power of ten of the first significant digit of `value` (not zero),
/// after Excel's rounding to 15 digits.
fn decimal_exponent(value: f64) -> i32 {
    let text = format!("{:.*e}", EXCEL_DIGITS - 1, value);
    text.split('e').nth(1).and_then(|exp| exp.parse().ok()).unwrap_or(0)
}

/// `value` (not negative) rounded half up to `decimals` decimals, as its
/// integer digits (empty for zero) and its `decimals` decimal digits.
/// Excel rounds to 15 significant digits first, so `2.675` is `2.68`.
fn digits(value: f64, decimals: usize) -> (String, String) {
    let text = format!("{:.*e}", EXCEL_DIGITS - 1, value);
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    // Where the decimal point falls in `digits`.
    let mut point = exponent + 1;
    if point < 0 {
        #[allow(clippy::cast_sign_loss)]
        let zeros = vec![0; (-point) as usize];
        digits.splice(0..0, zeros);
        point = 0;
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let mut point = point as usize;
    let keep = point + decimals;
    if digits.len() > keep {
        let round_up = digits[keep] >= 5;
        digits.truncate(keep);
        if round_up {
            let mut at = keep;
            loop {
                if at == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                at -= 1;
                if digits[at] == 9 {
                    digits[at] = 0;
                } else {
                    digits[at] += 1;
                    break;
                }
            }
        }
    }
    digits.resize(point + decimals, 0);
    let text: String = digits.iter().map(|digit| char::from(b'0' + digit)).collect();
    let (int, frac) = text.split_at(point);
    (int.trim_start_matches('0').to_string(), frac.to_string())
}

/// `value` (not negative) as a mantissa with `decimals` decimals and its
/// exponent, a multiple of `step` (`##0.0E+0` steps by 3).
fn scientific(value: f64, decimals: usize, step: i32) -> (String, i32) {
    if value == 0.0 {
        let frac = "0".repeat(decimals);
        return (if decimals > 0 { format!("0.{frac}") } else { "0".into() }, 0);
    }
    let mut exponent = decimal_exponent(value).div_euclid(step) * step;
    loop {
        let (int, frac) = digits(value / 10f64.powi(exponent), decimals);
        // Rounding up can give one digit too many (9.999 → 10.00).
        if int.len() > usize::try_from(step).unwrap_or(1) {
            exponent += step;
            continue;
        }
        let int = if int.is_empty() { "0".to_string() } else { int };
        let mantissa = if decimals > 0 { format!("{int}.{frac}") } else { int };
        return (mantissa, exponent);
    }
}

/// Whether `token` is a digit placeholder.
fn is_digit(token: &Token) -> bool {
    matches!(token, Token::Digit(_))
}

/// Where the parts of a number section are, by index of its tokens.
struct Layout {
    /// The exponent (`E+`), when the number is scientific.
    exponent_at: Option<usize>,
    /// The end of the mantissa: the exponent, or the end of the section.
    mantissa_end: usize,
    /// The integer digit placeholders.
    int_places: Vec<usize>,
    /// The decimal digit placeholders.
    frac_places: Vec<usize>,
    /// Commas separate the thousands.
    thousands: bool,
    /// How many times the number is divided by 1000 (commas after the
    /// last digit).
    scale: usize,
    /// How many times the number is multiplied by 100 (`%`).
    percents: usize,
}

impl Layout {
    /// The layout of a number section.
    fn of(tokens: &[Token]) -> Self {
        let exponent_at = tokens.iter().position(|token| matches!(token, Token::Exponent { .. }));
        let mantissa_end = exponent_at.unwrap_or(tokens.len());
        let int_end = tokens[..mantissa_end]
            .iter()
            .position(|token| *token == Token::Point)
            .unwrap_or(mantissa_end);
        let last_digit = tokens[..mantissa_end].iter().rposition(is_digit);
        let first_digit = tokens.iter().position(is_digit);
        // Commas after the last digit divide by 1000; commas between
        // integer digits separate the thousands.
        let mut scale = 0;
        let mut thousands = false;
        for (at, token) in tokens[..mantissa_end].iter().enumerate() {
            if *token == Token::Comma {
                if last_digit.is_none_or(|last| at > last) {
                    scale += 1;
                } else if at < int_end && first_digit.is_some_and(|first| at > first) {
                    thousands = true;
                }
            }
        }
        Self {
            exponent_at,
            mantissa_end,
            int_places: (0..int_end).filter(|&at| is_digit(&tokens[at])).collect(),
            frac_places: (int_end..mantissa_end).filter(|&at| is_digit(&tokens[at])).collect(),
            thousands,
            scale,
            percents: tokens.iter().filter(|token| **token == Token::Percent).count(),
        }
    }
}

/// `value` (not negative) with the tokens of a number section.
fn number(value: f64, tokens: &[Token]) -> String {
    if tokens.contains(&Token::Slash) && tokens.iter().any(is_digit) {
        return fraction(value, tokens);
    }
    let layout = Layout::of(tokens);
    let mut value = value;
    for _ in 0..layout.percents {
        value *= 100.0;
    }
    for _ in 0..layout.scale {
        value /= 1000.0;
    }
    let (int, frac, exponent) = if layout.exponent_at.is_some() {
        let places = layout.int_places.len();
        // `##0.0E+0` keeps the exponent a multiple of 3.
        let engineering = places > 1 && layout.int_places.iter().any(|&at| tokens[at] == Token::Digit('#'));
        let step = if engineering {
            i32::try_from(places).unwrap_or(1)
        } else {
            1
        };
        let (mantissa, exponent) = scientific(value, layout.frac_places.len(), step);
        let (int, frac) = mantissa.split_once('.').unwrap_or((&mantissa, ""));
        let int = if int == "0" { "" } else { int };
        (int.to_string(), frac.to_string(), Some(exponent))
    } else {
        let (int, frac) = digits(value, layout.frac_places.len());
        (int, frac, None)
    };
    let int_out = fill_integer(&int, tokens, &layout);
    let frac_out = fill_decimals(&frac, tokens, &layout.frac_places);
    let mut out = String::new();
    let mut exponent_digits = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        match token {
            Token::Literal(text) => out.push_str(text),
            Token::Percent => out.push('%'),
            Token::Slash => out.push('/'),
            Token::General => out.push_str(&general(value)),
            Token::Point if at < layout.mantissa_end => out.push('.'),
            Token::Digit(_) if layout.exponent_at.is_some_and(|exp_at| at > exp_at) => {
                exponent_digits.push(token.clone());
                // The exponent is written at its last placeholder.
                if !tokens[at + 1..].iter().any(is_digit) {
                    out.push_str(&exponent_text(exponent.unwrap_or(0), &exponent_digits));
                }
            }
            Token::Digit(_) => {
                if let Some(slot) = layout.int_places.iter().position(|&place| place == at) {
                    out.push_str(&int_out[slot]);
                } else if let Some(slot) = layout.frac_places.iter().position(|&place| place == at) {
                    out.push_str(&frac_out[slot]);
                }
            }
            Token::Exponent { plus } => {
                out.push('E');
                match exponent {
                    Some(exp) if exp < 0 => out.push('-'),
                    _ if *plus => out.push('+'),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// What each integer placeholder writes of the digits `int` (empty for
/// zero), filled from the right: a digit, or for none `0`, a space (`?`)
/// or nothing (`#`). The first placeholder takes the digits left over.
fn fill_integer(int: &str, tokens: &[Token], layout: &Layout) -> Vec<String> {
    let mut int_out: Vec<String> = vec![String::new(); layout.int_places.len()];
    let int_digits: Vec<char> = int.chars().collect();
    let mut from_right = 0;
    for (slot, &at) in layout.int_places.iter().enumerate().rev() {
        let Token::Digit(placeholder) = tokens[at] else {
            continue;
        };
        let take = if slot == 0 {
            int_digits.len().saturating_sub(from_right).max(1)
        } else {
            1
        };
        let mut written = String::new();
        for _ in 0..take {
            let digit = int_digits
                .len()
                .checked_sub(from_right + 1)
                .map(|index| int_digits[index]);
            let ch = match (digit, placeholder) {
                (Some(digit), _) => digit,
                (None, '0') => '0',
                (None, '?') => ' ',
                _ => continue,
            };
            if layout.thousands && from_right > 0 && from_right % 3 == 0 && ch != ' ' {
                written.insert(0, ',');
            }
            written.insert(0, ch);
            from_right += 1;
        }
        int_out[slot] = written;
    }
    int_out
}

/// What each decimal placeholder writes of the digits `frac`: trailing
/// zeros are dropped by `#` and made spaces by `?`.
fn fill_decimals(frac: &str, tokens: &[Token], frac_places: &[usize]) -> Vec<String> {
    let frac_digits: Vec<char> = frac.chars().collect();
    let mut frac_out: Vec<String> = frac_digits.iter().map(char::to_string).collect();
    for (slot, &at) in frac_places.iter().enumerate().rev() {
        if frac_digits[slot] != '0' {
            break;
        }
        match tokens[at] {
            Token::Digit('#') => frac_out[slot].clear(),
            Token::Digit('?') => frac_out[slot] = " ".into(),
            _ => break,
        }
    }
    frac_out
}

/// The digits of an exponent, with at least as many as the code has `0`.
fn exponent_text(exponent: i32, placeholders: &[Token]) -> String {
    let min = placeholders.iter().filter(|token| **token == Token::Digit('0')).count();
    format!("{:0min$}", exponent.unsigned_abs())
}

/// `value` (not negative) as a fraction (`# ?/?`, `# ??/??`, `?/8`): a
/// whole part when the code has one, then the closest fraction whose
/// denominator has at most as many digits as the code shows.
fn fraction(value: f64, tokens: &[Token]) -> String {
    let slash = tokens.iter().position(|token| *token == Token::Slash).unwrap_or(0);
    // The denominator: placeholders, or a fixed number (`?/8`).
    let after = &tokens[slash + 1..];
    // `?/100`: the literal digits, and the zeros that follow them.
    let fixed: String = if matches!(after.first(), Some(Token::Literal(text)) if text.chars().all(|ch| ch.is_ascii_digit()))
    {
        after
            .iter()
            .map_while(|token| match token {
                Token::Literal(text) if text.chars().all(|ch| ch.is_ascii_digit()) => Some(text.as_str()),
                Token::Digit('0') => Some("0"),
                _ => None,
            })
            .collect()
    } else {
        String::new()
    };
    let den_places = after.iter().take_while(|token| is_digit(token)).count();
    // The numerator: the placeholders right before the bar.
    let num_start = tokens[..slash]
        .iter()
        .rposition(|token| !is_digit(token))
        .map_or(0, |at| at + 1);
    let has_whole = tokens[..num_start].iter().any(is_digit);
    let (whole, part) = if has_whole {
        (value.trunc(), value.fract())
    } else {
        (0.0, value)
    };
    let (num, den) = match fixed.parse::<u64>() {
        Ok(den) if den > 0 => {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let num = (part * den as f64).round() as u64;
            (num, den)
        }
        _ => closest_fraction(
            part,
            10u64.pow(u32::try_from(den_places.clamp(1, MAX_FRACTION_DIGITS)).unwrap_or(1)) - 1,
        ),
    };
    let (whole, num) = if has_whole && num == den {
        (whole + 1.0, 0)
    } else {
        (whole, num)
    };
    let whole_text = if whole > 0.0 || num == 0 {
        general(whole)
    } else {
        String::new()
    };
    if num == 0 {
        return whole_text;
    }
    format!("{whole_text} {num}/{den}").trim().to_string()
}

/// The fraction closest to `value`, with a denominator up to `max_den`
/// (the smallest denominator among equally close ones).
fn closest_fraction(value: f64, max_den: u64) -> (u64, u64) {
    let mut best = (value.round() as u64, 1, f64::INFINITY);
    for den in 1..=max_den {
        #[allow(clippy::cast_precision_loss)]
        let den_f = den as f64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let num = (value * den_f).round() as u64;
        #[allow(clippy::cast_precision_loss)]
        let error = (value - num as f64 / den_f).abs();
        if error < best.2 - f64::EPSILON {
            best = (num, den, error);
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `value` formatted with the custom code `code`, in the 1900 system.
    fn code(value: f64, code: &str) -> String {
        format(value, &NumFmt::Code(code.to_string()), false)
    }

    #[test]
    fn formats_as_excel_shows_them_with_iso_dates() {
        let cases: &[(f64, &str, &str)] = &[
            (1234.5, "General", "1234.5"),
            (0.1 + 0.2, "General", "0.3"),
            (1e20, "General", "1E+20"),
            (1234.5, "#,##0.00", "1,234.50"),
            (-1234.5, "#,##0.00;(#,##0.00)", "(1,234.50)"),
            (0.0, "0.00;-0.00;\"-\"", "-"),
            (0.125, "0.0%", "12.5%"),
            (12345.678, "0.00E+00", "1.23E+04"),
            (1500.0, "\"¥\"#,##0", "¥1,500"),
            (2.0, "[$€-407] #,##0.00", "€ 2.00"),
            (5.0, "[Red]0", "5"),
            (45322.0, "m/d/yyyy", "2024-01-31"),
            (45322.0, "dd\\.mm\\.yy", "2024-01-31"),
            (45322.5856, "yyyy-mm-dd hh:mm", "2024-01-31 14:03"),
            (0.5856, "h:mm AM/PM", "14:03"),
            (0.585601, "hh:mm:ss", "14:03:16"),
            (60.0, "yyyy-mm-dd", "1900-02-29"), // Excel's own leap day of 1900
        ];
        for (value, code, want) in cases {
            assert_eq!(format(*value, &NumFmt::Code(code.to_string()), false), *want, "{code}");
        }
    }

    #[test]
    fn builtin_14_is_iso_and_builtin_7_has_no_symbol() {
        assert_eq!(format(45322.0, &builtin(14), false), "2024-01-31");
        assert_eq!(format(1234.5, &builtin(7), false), "1,234.50");
        assert_eq!(format(-1234.5, &builtin(7), false), "(1,234.50)");
        assert!(is_date(&builtin(14)));
        assert!(!is_date(&builtin(7)));
    }

    #[test]
    fn the_1904_system_shifts_dates() {
        assert_eq!(format(0.0, &builtin(14), true), "1904-01-01");
        assert_eq!(format(60.0, &builtin(14), true), "1904-03-01");
    }

    #[test]
    fn impossible_dates_fall_back_to_numbers() {
        assert_eq!(code(-1.0, "yyyy-mm-dd"), "-1");
        assert_eq!(code(3e6, "yyyy-mm-dd"), "3000000");
    }

    #[test]
    fn days_around_the_leap_day_of_1900() {
        assert_eq!(code(1.0, "yyyy-mm-dd"), "1900-01-01");
        assert_eq!(code(59.0, "yyyy-mm-dd"), "1900-02-28");
        assert_eq!(code(61.0, "yyyy-mm-dd"), "1900-03-01");
        assert_eq!(code(2_958_465.0, "yyyy-mm-dd"), "9999-12-31");
    }

    #[test]
    fn elapsed_hours_go_past_a_day() {
        assert_eq!(code(27.5 / 24.0, "[h]:mm"), "27:30");
        assert_eq!(code(1.5, "h:mm"), "12:00");
        assert_eq!(code(0.999_999, "hh:mm"), "00:00");
        assert_eq!(code(45_322.999_999, "yyyy-mm-dd hh:mm"), "2024-02-01 00:00");
    }

    #[test]
    fn minutes_after_hours_are_not_months() {
        assert_eq!(code(0.5856, "h \"h\" m \"min\""), "14:03");
        assert_eq!(code(0.585601, "mm:ss"), "14:03:16");
        assert!(is_date(&NumFmt::Code("mmm-yy".into())));
        assert!(!is_date(&NumFmt::Code("0.00E+00".into())));
        assert!(!is_date(&NumFmt::Code("\"day\" 0".into())));
        assert!(!is_date(&NumFmt::General));
    }

    #[test]
    fn general_keeps_eleven_digits() {
        let general = |value: f64| format(value, &NumFmt::General, false);
        assert_eq!(general(0.0), "0");
        assert_eq!(general(-42.0), "-42");
        assert_eq!(general(1.0 / 3.0), "0.33333333333");
        assert_eq!(general(2.0 / 3.0), "0.66666666667");
        assert_eq!(general(12_345_678_901.0), "12345678901");
        assert_eq!(general(123_456_789_012.0), "1.23457E+11");
        assert_eq!(general(1.5e-10), "1.5E-10");
        assert_eq!(general(0.000_012_5), "0.0000125");
    }

    #[test]
    fn sections_and_signs() {
        assert_eq!(code(-5.0, "0"), "-5");
        assert_eq!(code(-5.0, "0;0"), "5");
        assert_eq!(code(0.0, "0;-0;"), "");
        assert_eq!(code(-1234.0, "#,##0;[Red]-#,##0"), "-1,234");
        assert_eq!(code(-0.001, "0.00"), "0.00");
        assert_eq!(code(3.0, "General\" units\""), "3 units");
    }

    #[test]
    fn placeholders_round_half_up_and_pad() {
        assert_eq!(code(2.5, "0"), "3");
        assert_eq!(code(2.675, "0.00"), "2.68");
        assert_eq!(code(0.5, "#"), "1");
        assert_eq!(code(0.0, "#"), "");
        assert_eq!(code(7.0, "000"), "007");
        assert_eq!(code(1.5, "#.##"), "1.5");
        assert_eq!(code(1.0, "0.0#"), "1.0");
        assert_eq!(code(1_234_567.0, "#,##0"), "1,234,567");
        assert_eq!(code(999.5, "#,##0"), "1,000");
        assert_eq!(code(1_234_567.0, "#,##0.0,,\"M\""), "1.2M");
        assert_eq!(code(5_551_234.0, "000-0000"), "555-1234");
        assert_eq!(code(0.25, "0%"), "25%");
    }

    #[test]
    fn accounting_formats() {
        assert_eq!(format(1234.0, &builtin(41), false), "1,234");
        assert_eq!(format(-1234.0, &builtin(41), false), "(1,234)");
        assert_eq!(format(0.0, &builtin(43), false), "-");
        assert_eq!(format(1234.5, &builtin(44), false), "1,234.50");
        assert_eq!(format(1234.0, &builtin(37), false), "1,234");
    }

    #[test]
    fn scientific_and_fractions() {
        assert_eq!(code(0.000_123, "0.00E+00"), "1.23E-04");
        assert_eq!(code(9.999, "0.00E+00"), "1.00E+01");
        assert_eq!(code(12345.0, "##0.0E+0"), "12.3E+3");
        assert_eq!(code(0.0, "0.00E+00"), "0.00E+00");
        assert_eq!(format(1.5, &builtin(12), false), "1 1/2");
        assert_eq!(format(0.75, &builtin(12), false), "3/4");
        assert_eq!(format(3.0, &builtin(12), false), "3");
        assert_eq!(format(0.333, &builtin(13), false), "1/3");
        assert_eq!(code(2.25, "# ?/100"), "2 25/100");
        assert_eq!(code(0.5, "?/8"), "4/8");
    }

    #[test]
    fn builtin_ids() {
        assert_eq!(builtin(0), NumFmt::General);
        assert_eq!(builtin(4), NumFmt::Code("#,##0.00".into()));
        assert_eq!(builtin(10), NumFmt::Code("0.00%".into()));
        assert_eq!(builtin(25), NumFmt::General);
        assert_eq!(builtin(163), NumFmt::General);
        assert!((14..=22).chain(45..=47).all(|id| is_date(&builtin(id))));
        assert_eq!(format(0.5, &builtin(18), false), "12:00");
        assert_eq!(format(45322.25, &builtin(22), false), "2024-01-31 06:00");
        assert_eq!(format(1.0 + 1.0 / 24.0, &builtin(46), false), "25:00:00");
        assert_eq!(format(1.0 / 1440.0, &builtin(45), false), "00:01:00");
    }
}
