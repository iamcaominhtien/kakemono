//! The fonts a Word document is laid out with: those embedded in it, those
//! installed on the computer, and open fonts with the same metrics as
//! Word's usual fonts (Carlito for Calibri, Liberation for Times New Roman
//! and Arial...), which give the same line breaks.

use crate::word::fmt::model::{EmbeddedFont, FontKind};
use rustybuzz::ttf_parser;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A loaded font face.
pub struct Face {
    /// The font file.
    pub data: Arc<Vec<u8>>,
    /// Index of the face in the file (collections).
    pub index: u32,
    /// Its metrics.
    pub metrics: Metrics,
    /// The family name it was found under.
    pub family: String,
}

/// The metrics of a face, in multiples of the font size.
#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    /// Units per em.
    pub upem: f32,
    /// Height above the baseline Word reserves (`usWinAscent`).
    pub ascent: f32,
    /// Depth below the baseline Word reserves (`usWinDescent`).
    pub descent: f32,
    /// Extra space Word adds to a line (the external leading).
    pub leading: f32,
    /// Position of the underline below the baseline.
    pub underline_pos: f32,
    /// Thickness of the underline.
    pub underline_thickness: f32,
    /// Height of the strike-through line above the baseline.
    pub strike_pos: f32,
    /// Height of a lowercase x.
    pub x_height: f32,
    /// Height of a capital.
    pub cap_height: f32,
}

/// Identifier of a loaded face.
pub type FaceId = usize;

/// A family name with the bold and italic flags asked for.
type StyleKey = (String, bool, bool);

/// The face chosen for a style, and whether bold and italic must be faked.
type ChosenFace = (FaceId, bool, bool);

/// A face found on the computer or in the document, before it is loaded.
#[derive(Debug, Clone)]
struct Candidate {
    source: Source,
    index: u32,
    bold: bool,
    italic: bool,
}

/// Where a font file comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Source {
    /// A file on disk.
    File(PathBuf),
    /// A font embedded in the document.
    Embedded(usize),
}

/// The fonts available to the layout.
pub struct FontBook {
    faces: Vec<Face>,
    by_family: HashMap<String, Vec<Candidate>>,
    loaded: HashMap<(Source, u32), FaceId>,
    embedded: Vec<Arc<Vec<u8>>>,
    resolved: HashMap<StyleKey, Option<ChosenFace>>,
    fallback_faces: Option<Vec<FaceId>>,
    kinds: HashMap<String, FontKind>,
}

/// Fonts with the same metrics as fonts Word documents often use: the
/// first one found is taken in place of the original, with the same line
/// breaks.
const ALIASES: &[(&str, &[&str])] = &[
    ("calibri", &["carlito"]),
    ("cambria", &["caladea"]),
    ("times new roman", &["liberation serif", "tinos"]),
    ("times", &["liberation serif", "tinos"]),
    ("arial", &["liberation sans", "arimo"]),
    ("helvetica", &["liberation sans", "arimo"]),
    ("arial narrow", &["liberation sans narrow"]),
    ("courier new", &["liberation mono", "cousine"]),
    ("courier", &["liberation mono", "cousine"]),
    ("georgia", &["gelasio"]),
    ("segoe ui", &["selawik"]),
];

/// Fonts that only look like the original (their widths differ): used to
/// draw text measured with the original's metrics (`metrics`), or when
/// nothing better is known.
const LOOKALIKES: &[(&str, &[&str])] = &[
    ("calibri light", &["carlito"]),
    ("verdana", &["dejavu sans"]),
    ("tahoma", &["dejavu sans"]),
    ("segoe ui", &["open sans", "noto sans", "dejavu sans"]),
    ("trebuchet ms", &["dejavu sans"]),
    ("symbol", &["dejavu sans", "freeserif"]),
    ("wingdings", &["dejavu sans", "freeserif"]),
    ("cambria math", &["dejavu serif", "freeserif"]),
    ("times new roman", &["freeserif"]),
    ("arial", &["freesans"]),
    ("courier new", &["freemono"]),
];

/// Families tried, in order, for characters the chosen font lacks.
const FALLBACKS: &[&str] = &[
    "liberation sans",
    "dejavu sans",
    "noto sans",
    "freeserif",
    "noto sans cjk sc",
    "wenquanyi zen hei",
    "noto sans symbols",
    "noto sans symbols2",
    "dejavu serif",
];

/// The family used when nothing else is found.
const LAST_RESORT: &[&str] = &["liberation serif", "dejavu serif", "dejavu sans", "freeserif"];

impl FontBook {
    /// The fonts of the computer and those `embedded` in the document.
    pub fn new(embedded: &[EmbeddedFont], kinds: &HashMap<String, FontKind>) -> Self {
        let mut book = FontBook {
            faces: Vec::new(),
            by_family: HashMap::new(),
            loaded: HashMap::new(),
            embedded: Vec::new(),
            resolved: HashMap::new(),
            fallback_faces: None,
            kinds: kinds.clone(),
        };
        for dir in font_dirs() {
            book.scan(&dir, 0);
        }
        for (at, font) in embedded.iter().enumerate() {
            book.embedded.push(Arc::new(font.data.clone()));
            book.by_family.entry(font.family.to_lowercase()).or_default().insert(
                0,
                Candidate {
                    source: Source::Embedded(at),
                    index: 0,
                    bold: font.bold,
                    italic: font.italic,
                },
            );
        }
        book
    }

    /// Adds the font files under `dir`.
    fn scan(&mut self, dir: &Path, depth: usize) {
        const MAX_DEPTH: usize = 6;
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < MAX_DEPTH {
                    self.scan(&path, depth + 1);
                }
                continue;
            }
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
            for index in 0..count {
                let Ok(face) = ttf_parser::Face::parse(&data, index) else {
                    continue;
                };
                let (families, bold, italic) = describe(&face);
                for family in families {
                    self.by_family.entry(family).or_default().push(Candidate {
                        source: Source::File(path.clone()),
                        index,
                        bold,
                        italic,
                    });
                }
            }
        }
    }

    /// The face for `family` in the given style: the family itself, a font
    /// with the same metrics, or a fallback; and whether bold and italic
    /// must be faked.
    pub fn face(&mut self, family: &str, bold: bool, italic: bool) -> (FaceId, bool, bool) {
        let key = (family.to_lowercase(), bold, italic);
        if let Some(found) = self.resolved.get(&key) {
            return found.unwrap_or_else(|| self.last_resort(bold, italic));
        }
        let mut names = vec![key.0.clone()];
        for table in [ALIASES, LOOKALIKES] {
            if let Some((_, aliases)) = table.iter().find(|(name, _)| *name == key.0) {
                names.extend(aliases.iter().map(|alias| (*alias).to_string()));
            }
        }
        let mut found = None;
        for name in &names {
            if let Some(face) = self.load_family(name, bold, italic) {
                found = Some(face);
                break;
            }
        }
        if found.is_none() {
            // A stand-in of the same kind.
            let lower = key.0.as_str();
            let kind = self.kinds.get(lower).copied().unwrap_or_else(|| guess_kind(lower));
            let stand_ins: &[&str] = match kind {
                FontKind::Serif => &["liberation serif", "dejavu serif", "freeserif"],
                FontKind::Mono => &["liberation mono", "dejavu sans mono", "freemono"],
                FontKind::Sans | FontKind::Symbol => &["liberation sans", "dejavu sans", "freesans"],
            };
            for name in stand_ins {
                if let Some(face) = self.load_family(name, bold, italic) {
                    found = Some(face);
                    break;
                }
            }
        }
        self.resolved.insert(key, found);
        found.unwrap_or_else(|| self.last_resort(bold, italic))
    }

    /// Whether `family` (or a font with its metrics) is available.
    pub fn has_family(&self, family: &str) -> bool {
        let name = family.to_lowercase();
        self.by_family.contains_key(&name)
            || ALIASES
                .iter()
                .find(|(alias, _)| *alias == name)
                .is_some_and(|(_, aliases)| aliases.iter().any(|alias| self.by_family.contains_key(*alias)))
    }

    /// The face used when a family is not found.
    fn last_resort(&mut self, bold: bool, italic: bool) -> (FaceId, bool, bool) {
        for name in LAST_RESORT {
            if let Some(face) = self.load_family(name, bold, italic) {
                return face;
            }
        }
        // Any font at all.
        let any = self.by_family.keys().next().cloned();
        if let Some(name) = any
            && let Some(face) = self.load_family(&name, bold, italic)
        {
            return face;
        }
        panic!("no font is installed: the layout needs at least one TrueType or OpenType font");
    }

    /// Loads the best face of family `name` for the style.
    fn load_family(&mut self, name: &str, bold: bool, italic: bool) -> Option<(FaceId, bool, bool)> {
        let candidates = self.by_family.get(name)?.clone();
        let score = |cand: &Candidate| u8::from(cand.bold != bold) * 2 + u8::from(cand.italic != italic);
        let best = candidates.iter().min_by_key(|cand| score(cand))?;
        let id = self.load(best.source.clone(), best.index, name)?;
        Some((id, bold && !best.bold, italic && !best.italic))
    }

    /// Loads a face once.
    fn load(&mut self, source: Source, index: u32, family: &str) -> Option<FaceId> {
        if let Some(&id) = self.loaded.get(&(source.clone(), index)) {
            return Some(id);
        }
        let data = match &source {
            Source::File(path) => Arc::new(std::fs::read(path).ok()?),
            Source::Embedded(at) => self.embedded.get(*at)?.clone(),
        };
        let face = ttf_parser::Face::parse(&data, index).ok()?;
        let metrics = metrics(&face);
        self.faces.push(Face {
            data: data.clone(),
            index,
            metrics,
            family: family.to_string(),
        });
        let id = self.faces.len() - 1;
        self.loaded.insert((source, index), id);
        Some(id)
    }

    /// The loaded face `id`.
    pub fn get(&self, id: FaceId) -> &Face {
        &self.faces[id]
    }

    /// All loaded faces.
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// The fallback faces, loaded once (regular style).
    pub fn fallbacks(&mut self) -> Vec<FaceId> {
        if let Some(faces) = &self.fallback_faces {
            return faces.clone();
        }
        let mut faces = Vec::new();
        for name in FALLBACKS {
            if let Some((id, _, _)) = self.load_family(name, false, false) {
                faces.push(id);
            }
        }
        self.fallback_faces = Some(faces.clone());
        faces
    }

    /// Whether face `id` has a glyph for `ch`.
    pub fn covers(&self, id: FaceId, ch: char) -> bool {
        let face = &self.faces[id];
        ttf_parser::Face::parse(&face.data, face.index)
            .ok()
            .and_then(|parsed| parsed.glyph_index(ch))
            .is_some()
    }
}

/// The kind of a font from its name, when the document does not say.
fn guess_kind(name: &str) -> FontKind {
    const SERIF_WORDS: &[&str] = &[
        "times",
        "roman",
        "serif",
        "garamond",
        "georgia",
        "book",
        "mincho",
        "cambria",
        "palatino",
        "baskerville",
        "minion",
        "caslon",
        "bodoni",
        "century",
    ];
    const MONO_WORDS: &[&str] = &["mono", "courier", "consolas", "code", "typewriter"];
    if name.contains("sans") {
        FontKind::Sans
    } else if MONO_WORDS.iter().any(|word| name.contains(word)) {
        FontKind::Mono
    } else if SERIF_WORDS.iter().any(|word| name.contains(word)) {
        FontKind::Serif
    } else {
        FontKind::Sans
    }
}

/// The family names (lowercase), boldness and italic of a face.
fn describe(face: &ttf_parser::Face<'_>) -> (Vec<String>, bool, bool) {
    let mut families = Vec::new();
    for name in face.names() {
        let id = name.name_id;
        if id != ttf_parser::name_id::FAMILY && id != ttf_parser::name_id::TYPOGRAPHIC_FAMILY {
            continue;
        }
        if let Some(text) = name.to_string() {
            let text = text.to_lowercase();
            if !families.contains(&text) {
                families.push(text);
            }
        }
    }
    let bold = face.is_bold() || face.weight().to_number() >= 600;
    (families, bold, face.is_italic() || face.is_oblique())
}

/// The metrics of a face, as Word uses them.
fn metrics(face: &ttf_parser::Face<'_>) -> Metrics {
    let upem = f32::from(face.units_per_em());
    let tables = face.tables();
    let (win_ascent, win_descent) = tables
        .os2
        .map(|os2| {
            (
                f32::from(os2.windows_ascender()),
                f32::from(os2.windows_descender()).abs(),
            )
        })
        .filter(|(ascent, descent)| ascent + descent > 0.0)
        .unwrap_or_else(|| (f32::from(face.ascender()), f32::from(face.descender()).abs()));
    let hhea_height = f32::from(face.ascender()) - f32::from(face.descender());
    let leading = (f32::from(face.line_gap()) - ((win_ascent + win_descent) - hhea_height)).max(0.0);
    let underline = face.underline_metrics();
    let strike = face.strikeout_metrics();
    Metrics {
        upem,
        ascent: win_ascent / upem,
        descent: win_descent / upem,
        leading: leading / upem,
        underline_pos: underline.map_or(0.1, |line| -f32::from(line.position) / upem),
        underline_thickness: underline.map_or(0.05, |line| f32::from(line.thickness).max(1.0) / upem),
        strike_pos: strike.map_or(0.25, |line| f32::from(line.position) / upem),
        x_height: face.x_height().map_or(0.45, |height| f32::from(height) / upem),
        cap_height: face.capital_height().map_or(0.65, |height| f32::from(height) / upem),
    }
}

/// The directories with installed fonts, and those named in
/// `KAKEMONO_FONT_DIRS` (separated like `PATH`).
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra) = std::env::var_os("KAKEMONO_FONT_DIRS") {
        dirs.extend(std::env::split_paths(&extra));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "windows") {
        let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        dirs.push(windir.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push("/System/Library/Fonts".into());
        dirs.push("/Library/Fonts".into());
        if let Some(home) = &home {
            dirs.push(home.join("Library/Fonts"));
        }
    } else {
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
        if let Some(home) = &home {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}
