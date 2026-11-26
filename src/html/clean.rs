//! The article of a web page: what is not part of it removed (menus,
//! banners, sharing buttons, advertisements, cookie notices, forms,
//! hidden elements), the element that holds it found, its title and
//! authors read, its pictures given the address of their best version
//! and its links made absolute.
//!
//! The article is found as Readability (Firefox's reader view) finds it,
//! more simply: after the clutter is removed, the element that holds the
//! text is the one where the text stops being in one child. Text in links
//! does not count, so that a menu or a list of links weighs nothing.
//!
//! Around and in it, what news sites put there goes too: the breadcrumb
//! and date before the title, the widgets after the body (tags, reactions,
//! a newsletter box, other articles), the teasers for other articles in
//! the body, a repeated author, counts. A page with no article (a video
//! page that a script fills) is read as its description.

use super::dom::{Dom, Id, Kind, Space};
use url::Url;

/// Share of its parent's text a child must hold for the article to be
/// looked for in that child alone.
const DESCEND_SHARE_RATIO: f32 = 0.7;
/// Share of the text the other children may hold, when they have a
/// heading or a long paragraph, for the article to be looked for in one
/// child: more than that is part of the article (an introduction before
/// its one long section).
const LOST_SHARE_MAX_RATIO: f32 = 0.01;
/// Characters of a paragraph that is text of the article, not a label.
const LONG_PARAGRAPH_CHARS: usize = 80;
/// Most characters of text not in links in a teaser for another article
/// (a nested `<article>`: its title, a line of summary).
const TEASER_MAX_CHARS: usize = 300;
/// Most characters of text not in links around a heading that is a link,
/// for its container to be a teaser too (`Xem thêm về:`, `Tags:`).
const TEASER_LABEL_CHARS: usize = 40;
/// Fewest characters of a paragraph that is only a link to another
/// article for it to be a teaser (not a word such as `Next`).
const LINK_PARAGRAPH_MIN_CHARS: usize = 30;
/// Fewest characters of text not in links in an article for its teasers
/// to be removed: in less, the page is a list of articles (a home page).
const TEASERS_MIN_ARTICLE_CHARS: usize = 500;
/// Most share of the text of the element that holds the article that a
/// part after the body may hold and still be a widget (tags, reactions, a
/// newsletter box), when the body holds most of it.
const TRAILING_MAX_SHARE_RATIO: f32 = 0.15;
/// Share of the text of a part after the body in links for it to be a
/// list of other articles, even with their summaries.
const WIDGET_LINK_SHARE_RATIO: f32 = 0.5;
/// Most characters of a part before the title (a breadcrumb, a date) that
/// is removed.
const BEFORE_TITLE_MAX_CHARS: usize = 100;
/// Fewest characters of a block of text that, repeating the one before
/// it, is a copy (not a word said twice).
const REPEATED_BLOCK_MIN_CHARS: usize = 10;
/// Most characters (not spaces) of a box around a button that asks the
/// reader to press it.
const CALL_TO_ACTION_MAX_CHARS: usize = 200;
/// Characters that separate the parts of a byline.
const SEPARATORS: [char; 4] = ['|', '·', '•', '/'];
/// Most characters of a link to the comments of an article.
const COMMENT_LINK_MAX_CHARS: usize = 30;
/// The word of a byline before the name of its author, alone in a block
/// when the name has gone.
const BYLINE_WORDS: [&str; 5] = ["by", "von", "par", "por", "bởi"];
/// Most characters of a block that holds only a number (counts of
/// reactions, `0 0 0`).
const COUNT_MAX_CHARS: usize = 20;
/// Most characters of the text of a page with no article (a video page)
/// for its description to be read instead.
const NO_ARTICLE_MAX_CHARS: usize = 200;
/// Characters of a heading that ends as a sentence ends and is a
/// paragraph set large (a summary), not a heading.
const SENTENCE_HEADING_CHARS: usize = 100;
/// Share of the text of the page an element may hold and still be removed
/// for its class or `id` alone: one that holds more is the article, named
/// oddly.
const CLUTTER_MAX_SHARE_RATIO: f32 = 0.5;
/// Smallest width or height of a picture worth keeping, in CSS pixels:
/// below it, an icon, a logo or a tracking pixel.
pub(crate) const MIN_PICTURE_PX: f32 = 48.0;
/// Width of a picture asked for in a `srcset`, in CSS pixels: the
/// smallest version at least this wide, the widest otherwise.
const PREFERRED_PICTURE_WIDTH_PX: f32 = 1600.0;
/// Widest a picture is shown, in CSS pixels (6.5 inches, the width of
/// the text of a page): a page gives the size of the file, which a
/// browser fits to the column.
const MAX_PICTURE_WIDTH_PX: f32 = 624.0;
/// Longest a `data:` URL can be and still be a placeholder (a blank
/// picture shown until the real one loads).
const PLACEHOLDER_DATA_URL_CHARS: usize = 512;
/// Texts of the links a heading has to itself (Sphinx's `¶`).
const SELF_LINK_MARKS: [&str; 6] = ["", "¶", "#", "§", "🔗", "\u{1f517}"];
/// Headings.
const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
/// Words of a class or `id`, of a picture or of what holds it, that mark
/// an icon or a logo of the site (`info-icon`, `div-logo`).
const ICON_WORDS: [&str; 10] = [
    "icon", "icons", "logo", "logos", "avatar", "avt", "avata", "emoji", "sprite", "favicon",
];
/// How many elements around a picture are looked at for [`ICON_WORDS`]:
/// its parent and the parent of that.
const ICON_ANCESTORS: usize = 2;
/// CSS pixels of one step of the spacing scale of Tailwind (`h-4` is 16
/// pixels), the utility classes that give many sites their sizes.
const TAILWIND_STEP_PX: f32 = 4.0;
/// CSS pixels of a `rem` or an `em`, as browsers set them by default.
const REM_PX: f32 = 16.0;
/// Most authors read from the metadata.
const MAX_AUTHORS: usize = 30;
/// Longest name of an author, in characters.
const MAX_AUTHOR_CHARS: usize = 80;
/// Longest part of a `<title>` that can be the name of the site, in
/// characters (`Gravitational wave - Wikipedia`).
const MAX_SITE_NAME_CHARS: usize = 40;

/// Elements removed with their content: scripts, embedded objects,
/// controls of forms, navigation.
const REMOVED: [&str; 26] = [
    "script", "style", "template", "iframe", "object", "embed", "applet", "video", "audio", "canvas", "button",
    "input", "select", "textarea", "option", "datalist", "dialog", "nav", "menu", "link", "meta", "base", "param",
    "track", "aside", "frameset",
];
/// Roles of elements that are not part of an article.
const CLUTTER_ROLES: [&str; 14] = [
    "navigation",
    "banner",
    "contentinfo",
    "complementary",
    "search",
    "dialog",
    "alertdialog",
    "menu",
    "menubar",
    "toolbar",
    "tablist",
    "button",
    "alert",
    "status",
];
/// Words of a class or `id` that mark clutter, whatever else it says
/// (`sidebar-content` is a sidebar).
const CLUTTER_WORDS: [&str; 40] = [
    "ad",
    "ads",
    "adsbygoogle",
    // The boxes of Wikipedia about the article ("needs more citations").
    "ambox",
    "advert",
    "advertisement",
    "advertising",
    "breadcrumb",
    "breadcrumbs",
    "comment",
    "comments",
    "consent",
    // `affiliate-disclaimer`: links that earn a commission.
    "disclaimer",
    "cookie",
    "cookies",
    "disqus",
    "editsection",
    "gdpr",
    "modal",
    "navbox",
    "newsletter",
    "outbrain",
    "paywall",
    "popup",
    "promo",
    "related",
    "share",
    "sharing",
    "sidebar",
    // The box about the author after an article (`author-bio`).
    "bio",
    "skyscraper",
    "social",
    "sponsor",
    "sponsored",
    "subscribe",
    "taboola",
    // A list of tags (`article-tags`; one tag is too common a word).
    "tags",
    "toc",
    "toolbar",
    "catlinks",
];
/// Words of a class or `id` that mark clutter unless a word of
/// [`CONTENT_WORDS`] is there too (`article-header` is the article's).
const WEAK_CLUTTER_WORDS: [&str; 12] = [
    "banner",
    "footer",
    "header",
    "masthead",
    "menu",
    "nav",
    "navbar",
    "navigation",
    "noprint",
    "pager",
    "pagination",
    "subscription",
];
/// `id`s of clutter, as a whole: WordPress's sidebar (`#secondary`, beside
/// `#primary`). As a word of a class it can name anything (a style of
/// button).
const CLUTTER_IDS: [&str; 1] = ["secondary"];
/// Most characters of the box about the authors after an article.
const AUTHOR_BOX_MAX_CHARS: usize = 1000;
/// Parts of the address of the page of an author.
const AUTHOR_PATHS: [&str; 4] = ["/author/", "/authors/", "/tac-gia/", "/tacgia/"];
/// Words of a class that mark the summary of an article, under its title.
const LEAD_WORDS: [&str; 6] = ["sapo", "lead", "standfirst", "summary", "dek", "excerpt"];
/// Words of a class or `id` that mark the content of a page.
const CONTENT_WORDS: [&str; 7] = ["article", "body", "content", "entry", "main", "post", "story"];
/// Classes that hide an element from sight (left for screen readers).
const HIDDEN_CLASSES: [&str; 10] = [
    "sr-only",
    "visually-hidden",
    "visuallyhidden",
    "screen-reader-text",
    "screenreader-only",
    "a11y-hidden",
    "hidden",
    "d-none",
    "is-hidden",
    "skip-link",
];
/// Elements whose class or `id` is never read as clutter: the text of a
/// heading, a link, the cells of a table.
const NEVER_CLUTTER: [&str; 12] = [
    "html", "body", "main", "article", "a", "th", "td", "tr", "thead", "tbody", "tfoot", "caption",
];
/// Elements in which the article is looked for, inside the one that
/// holds it.
const CONTAINERS: [&str; 13] = [
    "body", "div", "main", "article", "section", "span", "form", "center", "font", "table", "tbody", "tr", "td",
];
/// Attributes of a lazy picture that hold its address until it loads.
const LAZY_SOURCES: [&str; 5] = [
    "data-src",
    "data-lazy-src",
    "data-original",
    "data-url",
    "data-hi-res-src",
];
/// Most other versions of a picture tried when its own cannot be fetched.
const MAX_FALLBACKS: usize = 2;
/// Attribute of a picture that holds its other addresses, separated by
/// spaces, to fetch when its own fails (removed once fetched).
pub(crate) const FALLBACK_ATTR: &str = "data-kakemono-fallback";
/// Attributes that hold a `srcset`.
const SRCSETS: [&str; 3] = ["srcset", "data-srcset", "data-lazy-srcset"];
/// Picture formats the reader cannot draw.
const UNDRAWN_TYPES: [&str; 3] = ["image/avif", "image/jxl", "image/heic"];

/// Where the addresses of a page are relative to.
pub(crate) enum Base<'a> {
    /// A page that came as bytes: its relative addresses lead nowhere.
    None,
    /// A file: its relative addresses are files next to it, made
    /// `file:` URLs against this address of its directory.
    File(&'a Url),
    /// A page of the Web, at this address.
    Url(&'a Url),
}

/// The article of a page.
pub(crate) struct Article {
    /// The element that holds it.
    pub(crate) root: Id,
    /// Its `<style>` elements (detached), for the head of the document.
    pub(crate) styles: Vec<Id>,
    /// Its `<link rel="stylesheet">` elements (detached), for a file.
    pub(crate) sheets: Vec<Id>,
    /// Its title.
    pub(crate) title: Option<String>,
    /// Its authors, from the metadata.
    pub(crate) authors: Vec<String>,
    /// Its language (`lang` of `<html>`).
    pub(crate) lang: Option<String>,
}

/// Finds the article of the page `dom` and cleans it (see the module).
pub(crate) fn article(dom: &mut Dom, base: &Base<'_>) -> Article {
    let (html, head, body) = dom.html_head_body();
    let body = body.or(html).unwrap_or(0);
    let lang = html
        .and_then(|html| dom.attr(html, "lang").or_else(|| dom.attr(html, "xml:lang")))
        .map(str::to_string);
    let meta = Meta::read(dom, head);
    let mut styles = Vec::new();
    let mut sheets = Vec::new();
    for node in dom.descendants(0) {
        if dom.is(node, "style") {
            styles.push(node);
        } else if dom.is(node, "link")
            && dom.attr(node, "rel").is_some_and(|rel| {
                rel.split_whitespace()
                    .any(|word| word.eq_ignore_ascii_case("stylesheet"))
            })
        {
            sheets.push(node);
        }
    }
    for &node in styles.iter().chain(&sheets) {
        dom.detach(node);
    }
    for &sheet in &sheets {
        if let Some(href) = dom.attr(sheet, "href").map(str::to_string) {
            dom.set_attr(sheet, "href", &absolute(&href, base));
        }
    }
    pictures(dom, body, base);
    let lead = lead_picture(dom, body, &meta, base);
    remove_clutter(dom, body);
    let (root, trailing) = find_root(dom, body);
    for node in trailing {
        dom.detach(node);
    }
    let root = without_article(dom, root, &meta);
    if let Some(lead) = lead {
        place_lead_picture(dom, root, lead);
    }
    remove_teasers(dom, root, base, &meta);
    before_title(dom, root);
    repeated_links(dom, root);
    repeated_blocks(dom, root);
    comment_links(dom, root);
    remove_counts(dom, root);
    heading_self_links(dom, root);
    sentence_headings(dom, root);
    split_at_double_breaks(dom, root);
    let title = title(dom, body, root, &meta);
    absolute_links(dom, root, base);
    remove_icons(dom, root);
    Article {
        root,
        styles,
        sheets,
        title,
        authors: meta.authors,
        lang,
    }
}

/// What the `<head>` of a page says about it.
struct Meta {
    /// The title of its `<title>`, without the name of the site.
    page_title: Option<String>,
    /// The title of its metadata (`og:title`, `citation_title`).
    meta_title: Option<String>,
    /// The authors.
    authors: Vec<String>,
    /// The description of the page (`og:description`, `description`).
    description: Option<String>,
    /// Its address, as it says (`og:url`, `<link rel="canonical">`): where a
    /// saved page came from.
    url: Option<String>,
    /// The address of its picture (`og:image`).
    image: Option<String>,
}

impl Meta {
    /// Reads the metadata in `head` (the whole page when there is none).
    fn read(dom: &Dom, head: Option<Id>) -> Meta {
        let scope = head.unwrap_or(0);
        let metas: Vec<(String, String)> = dom
            .find_all(scope, "meta")
            .into_iter()
            .filter_map(|meta| {
                let key = dom.attr(meta, "name").or_else(|| dom.attr(meta, "property"))?;
                let content = dom.attr(meta, "content")?.trim();
                (!content.is_empty()).then(|| (key.to_ascii_lowercase(), content.to_string()))
            })
            .collect();
        let first = |keys: &[&str]| {
            keys.iter().find_map(|key| {
                metas
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value.clone())
            })
        };
        let meta_title = first(&["citation_title", "og:title", "twitter:title", "dc.title"]);
        let page_title = dom
            .find(scope, "title")
            .map(|title| dom.clean_text(title))
            .filter(|title| !title.is_empty())
            .map(|title| without_site_name(&title));
        let mut authors: Vec<String> = metas
            .iter()
            .filter(|(name, _)| name == "citation_author" || name == "dc.creator")
            .map(|(_, value)| value.clone())
            .collect();
        if authors.is_empty()
            && let Some(names) = first(&["author", "article:author", "parsely-author", "sailthru.author"])
        {
            authors = names
                .split([',', ';', '|'])
                .flat_map(|part| part.split(" and "))
                .map(|name| name.trim().to_string())
                .collect();
        }
        let site = site_names(dom, scope, &first);
        authors.retain(|name| {
            let lower = name.to_lowercase();
            !name.is_empty()
                && name.chars().count() <= MAX_AUTHOR_CHARS
                && !name.contains("://")
                && !name.contains('@')
                // A domain (`thanhnien.vn`), not a name.
                && !(name.contains('.') && !name.contains(char::is_whitespace))
                && !site
                    .iter()
                    .any(|site| site.trim() == lower || format!(" {site} ").contains(&format!(" {lower} ")))
        });
        authors.dedup();
        authors.truncate(MAX_AUTHORS);
        let description = first(&["og:description", "description", "twitter:description"]);
        let canonical = dom.find_all(scope, "link").into_iter().find_map(|link| {
            let rel = dom.attr(link, "rel")?;
            rel.eq_ignore_ascii_case("canonical")
                .then(|| dom.attr(link, "href"))
                .flatten()
                .map(str::to_string)
        });
        let url = first(&["og:url"]).or(canonical);
        let image = first(&["og:image", "twitter:image"]);
        Meta {
            page_title,
            meta_title,
            authors,
            description,
            url,
            image,
        }
    }
}

/// The names of the site of a page, lower case, which are not authors
/// (Dân trí's `author`): the name its metadata gives (`first` reads it),
/// the end of its title after a separator, its domain without its suffix
/// (`VnExpress` of `vnexpress.net`).
fn site_names(dom: &Dom, scope: Id, first: &dyn Fn(&[&str]) -> Option<String>) -> Vec<String> {
    let mut site: Vec<String> = first(&["og:site_name", "application-name"])
        .into_iter()
        .chain(
            dom.find(scope, "title")
                .and_then(|title| site_name(&dom.clean_text(title)).map(str::to_string)),
        )
        .map(|name| name.to_lowercase())
        .collect();
    let domain = first(&["og:url"]).or_else(|| first(&["og:site_name"])).and_then(|url| {
        let host = url
            .split("://")
            .last()?
            .split('/')
            .next()?
            .trim_start_matches("www.")
            .to_lowercase();
        host.split('.').next().map(|label| format!(" {label}"))
    });
    site.extend(domain);
    site
}

/// The separators of the title of a page and the name of its site.
const SITE_SEPARATORS: [&str; 7] = [" | ", " - ", " – ", " — ", " \\ ", " · ", " :: "];

/// `title` without the name of the site after a separator
/// (`Gravitational wave - Wikipedia`).
fn without_site_name(title: &str) -> String {
    for separator in SITE_SEPARATORS {
        if let Some((page, site)) = title.rsplit_once(separator)
            && site.chars().count() <= MAX_SITE_NAME_CHARS
            && !page.trim().is_empty()
        {
            return page.trim().to_string();
        }
    }
    title.to_string()
}

/// The name of the site at the end of `title`, after a separator.
fn site_name(title: &str) -> Option<&str> {
    SITE_SEPARATORS.iter().find_map(|separator| {
        let (page, site) = title.rsplit_once(separator)?;
        (site.chars().count() <= MAX_SITE_NAME_CHARS && !page.trim().is_empty()).then(|| site.trim())
    })
}

/// The title of the article: its first `<h1>`; else the first `<h1>` of
/// the page (then put at the start of the article) when it is in the same
/// `<article>` or `<main>` as the article or is the title of the page
/// (not the name of the site in a menu bar); else the metadata's.
fn title(dom: &mut Dom, body: Id, root: Id, meta: &Meta) -> Option<String> {
    // The title is marked so (class `title`): the reader reads the authors
    // after it as authors, not as text after a heading.
    if let Some(h1) = dom.find(root, "h1") {
        let text = dom.clean_text(h1);
        if !text.is_empty() {
            dom.add_class(h1, "title");
            return Some(text);
        }
    }
    let known = [&meta.meta_title, &meta.page_title];
    let outer = dom
        .find(body, "h1")
        .map(|h1| (h1, dom.clean_text(h1)))
        .filter(|(h1, text)| {
            let same_article = dom
                .ancestor(*h1, |id| dom.is_any(id, &["article", "main"]))
                .is_some_and(|ancestor| dom.contains(ancestor, root));
            let is_title = known
                .iter()
                .any(|title| title.as_deref().is_some_and(|title| same_text(title, text)));
            !text.is_empty() && (same_article || is_title)
        });
    if let Some((_, text)) = outer {
        let heading = dom.element("h1", &[("class", "title")]);
        let words = dom.text_node(&text);
        dom.append(heading, words);
        dom.insert(root, 0, heading);
        return Some(text);
    }
    meta.meta_title.clone().or_else(|| meta.page_title.clone())
}

/// Whether two titles are the same, but for case and white space.
fn same_text(one: &str, other: &str) -> bool {
    let key = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    key(one) == key(other)
}

/// The words of a class or `id`: lower case, cut at what is not a letter
/// or a digit (`article__header` → `article`, `header`).
fn words(value: &str) -> impl Iterator<Item = String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
}

/// Whether the element `node` is clutter by its class or `id`.
fn is_clutter_by_name(dom: &Dom, node: Id) -> bool {
    let mut names = String::new();
    names.push_str(dom.attr(node, "class").unwrap_or_default());
    names.push(' ');
    names.push_str(dom.attr(node, "id").unwrap_or_default());
    names.push(' ');
    names.push_str(dom.attr(node, "data-role").unwrap_or_default());
    if dom
        .classes(node)
        .any(|class| HIDDEN_CLASSES.contains(&class.to_ascii_lowercase().as_str()))
    {
        return true;
    }
    if dom
        .attr(node, "id")
        .is_some_and(|id| CLUTTER_IDS.contains(&id.to_ascii_lowercase().as_str()))
    {
        return true;
    }
    let words: Vec<String> = words(&names).collect();
    let has = |list: &[&str]| words.iter().any(|word| list.contains(&word.as_str()));
    has(&CLUTTER_WORDS) || (has(&WEAK_CLUTTER_WORDS) && !has(&CONTENT_WORDS))
}

/// Whether the element is hidden by its attributes (`hidden`,
/// `display: none`).
fn is_hidden(dom: &Dom, node: Id) -> bool {
    // Not `aria-hidden`: it hides from screen readers what is on the
    // screen (pandoc's captions, which repeat the description).
    if dom.attr(node, "hidden").is_some() {
        return true;
    }
    dom.attr(node, "style").is_some_and(|style| {
        let style: String = style
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        // Not `visibility: hidden`: pages that fade their parts in as
        // they scroll hide them so until a script shows them.
        style.contains("display:none")
    })
}

/// Characters of text in each node that are not in a link (indexed by
/// node; 0 for the nodes outside `root`).
fn text_weights(dom: &Dom, root: Id) -> Vec<usize> {
    let mut weight = vec![0; dom.nodes.len()];
    for node in dom.descendants(root).into_iter().rev() {
        weight[node] = match &dom.nodes[node].kind {
            Kind::Text(text) => text.chars().filter(|ch| !ch.is_whitespace()).count(),
            Kind::Element { .. } if dom.is(node, "a") => 0,
            _ => dom.nodes[node].children.iter().map(|&child| weight[child]).sum(),
        };
    }
    weight
}

/// Removes the clutter of the page in `body` (see the module).
fn remove_clutter(dom: &mut Dom, body: Id) {
    let calls = calls_to_action(dom, body);
    let weight = text_weights(dom, body);
    let total = weight[body].max(1);
    let main_h1 = dom.find(body, "h1");
    for node in dom.descendants(body) {
        if node == body || !dom.attached(node) {
            continue;
        }
        let Kind::Element { name, space, .. } = &dom.nodes[node].kind else {
            continue;
        };
        if *space != Space::Html {
            // An SVG drawing hidden from screen readers is decoration.
            let decoration = dom.attr(node, "aria-hidden") == Some("true");
            if *space == Space::Svg && (is_hidden(dom, node) || decoration) {
                dom.detach(node);
            }
            continue;
        }
        let name = name.clone();
        if name == "noscript" {
            noscript(dom, node);
            continue;
        }
        if name == "form" {
            dom.unwrap(node);
            continue;
        }
        let holds_title = main_h1.is_some_and(|h1| dom.contains(node, h1));
        let in_heading = dom
            .ancestor(node, |id| dom.is_any(id, &["h1", "h2", "h3", "h4", "h5", "h6"]))
            .is_some();
        let role_clutter = dom.attr(node, "role").is_some_and(|role| {
            role.split_whitespace()
                .any(|role| CLUTTER_ROLES.contains(&role.to_ascii_lowercase().as_str()))
        });
        let share = weight[node] as f32 / total as f32;
        let named_clutter = !NEVER_CLUTTER.contains(&name.as_str())
            && !in_heading
            && share <= CLUTTER_MAX_SHARE_RATIO
            && is_clutter_by_name(dom, node);
        let landmark = matches!(name.as_str(), "header" | "footer")
            && dom.ancestor(node, |id| dom.is_any(id, &["article", "main"])).is_none();
        // A page that shows itself with a script hides its article until
        // then: what holds the title or most of the text is not hidden.
        let hidden = is_hidden(dom, node) && !holds_title && share <= CLUTTER_MAX_SHARE_RATIO;
        // The notes of the page (pandoc's `<aside role="doc-endnotes">`).
        let notes = dom.attr(node, "role").is_some_and(|role| role.starts_with("doc-"));
        let remove = (REMOVED.contains(&name.as_str()) && !notes)
            || hidden
            || ((role_clutter || named_clutter || landmark) && !holds_title)
            || (name == "footer" && !holds_title);
        if remove {
            dom.detach(node);
        }
    }
    for node in calls {
        dom.detach(node);
    }
}

/// The boxes in `body` that ask the reader to press a button (`Follow
/// topics and authors from this story`, `Follow`, `Subscribe`): the block
/// around a button (`<button>`, `role="button"`) that holds little text
/// besides it and no picture, title or code.
fn calls_to_action(dom: &Dom, body: Id) -> Vec<Id> {
    let mut boxes: Vec<Id> = Vec::new();
    for node in dom.descendants(body) {
        let button = dom.is(node, "button")
            || dom
                .attr(node, "role")
                .is_some_and(|role| role.trim().eq_ignore_ascii_case("button"));
        if !button {
            continue;
        }
        let Some(block) = dom.ancestor(node, |id| id != node && dom.is_any(id, &["div", "section", "aside"])) else {
            continue;
        };
        if block == body || boxes.contains(&block) {
            continue;
        }
        // What a pop-up holds is not shown (the author's photo and bio in
        // The Verge's `Follow` box).
        let shown = |id: Id| {
            dom.ancestor(id, |up| {
                dom.contains(block, up) && (dom.is(up, "aside") || dom.attr(up, "aria-hidden") == Some("true"))
            })
            .is_none()
        };
        let text: usize = dom
            .descendants(block)
            .into_iter()
            .filter(|&id| shown(id))
            .map(|id| match &dom.nodes[id].kind {
                Kind::Text(text) => text.chars().filter(|ch| !ch.is_whitespace()).count(),
                _ => 0,
            })
            .sum();
        let content = dom.descendants(block).into_iter().filter(|&id| shown(id)).any(|id| {
            dom.is_any(
                id,
                &[
                    "img", "picture", "figure", "video", "table", "pre", "code", "h1", "h2", "h3", "math",
                ],
            ) || (dom.is(id, "p")
                && dom.ancestor(id, |up| up == node).is_none()
                && dom.clean_text(id).chars().count() >= CALL_TO_ACTION_MAX_CHARS)
        });
        if text <= CALL_TO_ACTION_MAX_CHARS && !content {
            boxes.push(block);
        }
    }
    boxes
}

/// A `<noscript>`: the picture it holds goes to the lazy picture before it
/// (one with no address yet), or takes the place of the element when there
/// is none; anything else in it goes.
fn noscript(dom: &mut Dom, node: Id) {
    let Some(img) = dom.find(node, "img") else {
        dom.detach(node);
        return;
    };
    let lazy = dom.nodes[node].parent.and_then(|parent| {
        dom.find_all(parent, "img")
            .into_iter()
            .find(|&other| !dom.contains(node, other))
    });
    match lazy {
        Some(lazy) => {
            let has_source = dom.attr(lazy, "src").is_some_and(|src| !is_placeholder(src));
            if !has_source && let Some(src) = dom.attr(img, "src").map(str::to_string) {
                dom.set_attr(lazy, "src", &src);
            }
            dom.detach(node);
        }
        None => {
            dom.insert_after(node, img);
            dom.detach(node);
        }
    }
}

/// Whether `src` is a placeholder: empty, or a short `data:` URL.
fn is_placeholder(src: &str) -> bool {
    let src = src.trim();
    src.is_empty() || (src.starts_with("data:") && src.len() <= PLACEHOLDER_DATA_URL_CHARS)
}

/// The element that holds the article, in `body` (see the module), and
/// the widgets after its body in it: when the body holds most of its text
/// and the rest has a heading (a box of tags, a newsletter), the parts
/// after the body that hold little text, no long paragraph and no picture.
fn find_root(dom: &Dom, body: Id) -> (Id, Vec<Id>) {
    let weight = text_weights(dom, body);
    let mut at = body;
    let mut trailing = Vec::new();
    loop {
        let total = weight[at];
        let children = dom.element_children(at);
        let Some(&best) = children.iter().max_by_key(|&&child| weight[child]) else {
            break;
        };
        if !dom.is_any(best, &CONTAINERS) || (weight[best] as f32) < DESCEND_SHARE_RATIO * total as f32 {
            break;
        }
        let lost = total - weight[best];
        let lost_matters = dom.nodes[at].children.iter().any(|&child| {
            child != best
                && match &dom.nodes[child].kind {
                    Kind::Text(text) => text.trim().chars().count() >= LONG_PARAGRAPH_CHARS,
                    _ => has_article_text(dom, child),
                }
        });
        if lost_matters && lost as f32 >= LOST_SHARE_MAX_RATIO * total as f32 {
            // In links too: a page whose content is a list of links (a page
            // of a government that leads to others) keeps it.
            let chars = |node: Id| dom.clean_text(node).chars().filter(|ch| !ch.is_whitespace()).count();
            let all = chars(at);
            let children = &dom.nodes[at].children;
            let after = children.iter().skip_while(|&&child| child != best).skip(1);
            trailing = after
                .copied()
                .filter(|&child| {
                    (weight[child] as f32) < TRAILING_MAX_SHARE_RATIO * total as f32
                        && (chars(child) as f32) < TRAILING_MAX_SHARE_RATIO * all as f32
                        && (is_widget(dom, child) || is_author_box(dom, child))
                })
                .collect();
            break;
        }
        at = best;
    }
    (at, trailing)
}

/// Whether `node`, after the body of an article, is a widget: no long
/// paragraph, picture (but icons, and the pictures of links: thumbnails of
/// other articles, banners), table, numbered list or notes.
fn is_widget(dom: &Dom, node: Id) -> bool {
    if !matches!(dom.nodes[node].kind, Kind::Element { .. }) {
        return false;
    }
    // Most of its text in links: a list of other articles, whatever their
    // summaries say.
    let all = dom.clean_text(node).chars().filter(|ch| !ch.is_whitespace()).count();
    let links = dom
        .find_all(node, "a")
        .into_iter()
        .filter(|&a| dom.ancestor(a, |up| dom.is(up, "a")).is_none());
    let linked: usize = links
        .map(|a| dom.clean_text(a).chars().filter(|ch| !ch.is_whitespace()).count())
        .sum();
    let list_of_links = linked as f32 >= WIDGET_LINK_SHARE_RATIO * all as f32;
    !dom.descendants(node).into_iter().any(|id| match &dom.nodes[id].kind {
        Kind::Text(text) => !list_of_links && text.trim().chars().count() >= LONG_PARAGRAPH_CHARS,
        Kind::Element { .. } => {
            let picture = dom.is_any(id, &["img", "picture", "figure"])
                && !is_icon(dom, id)
                && dom.ancestor(id, |up| dom.is(up, "a")).is_none()
                && !(dom.is(id, "figure") && dom.find(id, "a").is_some() && dom.find(id, "figcaption").is_none());
            picture
                || dom.is_any(id, &["table", "ol", "pre", "h1", "math"])
                || (!list_of_links && dom.is(id, "p") && dom.clean_text(id).chars().count() >= LONG_PARAGRAPH_CHARS)
                || dom.attr(id, "role").is_some_and(|role| role.starts_with("doc-"))
        }
        _ => false,
    })
}

/// Whether `node`, after the body of an article, is the box about its
/// authors: a link to the page of an author, and little text.
fn is_author_box(dom: &Dom, node: Id) -> bool {
    if !matches!(dom.nodes[node].kind, Kind::Element { .. })
        || dom.clean_text(node).chars().count() > AUTHOR_BOX_MAX_CHARS
    {
        return false;
    }
    let author_link = dom.find_all(node, "a").into_iter().any(|a| {
        dom.attr(a, "rel")
            .is_some_and(|rel| rel.split_whitespace().any(|rel| rel.eq_ignore_ascii_case("author")))
            || dom.attr(a, "href").is_some_and(|href| {
                let href = href.to_ascii_lowercase();
                AUTHOR_PATHS.iter().any(|path| href.contains(path))
            })
    });
    author_link
        && !dom
            .descendants(node)
            .into_iter()
            .any(|id| dom.is_any(id, &["table", "ol", "pre", "h1", "figure"]))
}

/// The picture of the page (its `og:image`) in `body`, to show at the
/// start of an article without one: the photo above the title of a news
/// article, outside the element that holds its text (TechCrunch, The
/// Verge). Its `<figure>` when it is alone in one.
fn lead_picture(dom: &Dom, body: Id, meta: &Meta, base: &Base<'_>) -> Option<Id> {
    let image = absolute(meta.image.as_deref()?, base);
    let file = |src: &str| {
        let path = src.split(['?', '#']).next().unwrap_or(src);
        path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase()
    };
    let wanted = file(&image);
    if wanted.is_empty() {
        return None;
    }
    let img = dom
        .find_all(body, "img")
        .into_iter()
        .find(|&img| dom.attr(img, "src").is_some_and(|src| file(src) == wanted))?;
    let figure = dom
        .ancestor(img, |id| dom.is(id, "figure"))
        .filter(|&figure| dom.find_all(figure, "img").len() == 1);
    Some(figure.unwrap_or(img))
}

/// Puts the picture `lead` at the start of the article in `root` (after
/// its title), when the article has no picture of its own.
fn place_lead_picture(dom: &mut Dom, root: Id, lead: Id) {
    if dom.contains(root, lead) || dom.find(root, "img").is_some() || dom.contains(lead, root) {
        return;
    }
    dom.detach(lead);
    match dom.find(root, "h1") {
        Some(h1) => dom.insert_after(h1, lead),
        None => dom.insert(root, 0, lead),
    }
}

/// The element that holds the article, or, when the page has none (a
/// video page whose script fills it), a new one with the description of
/// the page (its metadata) when it has one.
fn without_article(dom: &mut Dom, root: Id, meta: &Meta) -> Id {
    let Some(description) = &meta.description else {
        return root;
    };
    let text = dom.clean_text(root);
    if has_article_text(dom, root) || text.chars().count() > NO_ARTICLE_MAX_CHARS || description.len() <= text.len() {
        return root;
    }
    let article = dom.element("div", &[]);
    if let Some(h1) = dom.find(root, "h1") {
        dom.append(article, h1);
    }
    let para = dom.element("p", &[]);
    let words = dom.text_node(description);
    dom.append(para, words);
    dom.append(article, para);
    if dom.nodes[root].parent.is_some() {
        dom.insert_after(root, article);
        dom.detach(root);
    }
    article
}

/// Removes the teasers for other articles in the article in `root`: a
/// nested `<article>` with little text and a link, and a heading that is
/// only a link to another page of the site (not a heading that names a
/// product it links to) with the container around it that holds no more
/// than a label.
fn remove_teasers(dom: &mut Dom, root: Id, base: &Base<'_>, meta: &Meta) {
    let weight = text_weights(dom, root);
    if weight[root] < TEASERS_MIN_ARTICLE_CHARS {
        return;
    }
    let own = meta.url.as_deref().and_then(|url| Url::parse(url).ok());
    let address = match base {
        Base::Url(url) => Some(*url),
        _ => own.as_ref(),
    };
    let site = address
        .and_then(Url::host_str)
        .map(|host| host.trim_start_matches("www.").to_ascii_lowercase());
    let other_page = |dom: &Dom, node: Id| {
        dom.find_all(node, "a").into_iter().any(|a| {
            dom.attr(a, "href").is_some_and(|href| {
                let href = href.trim();
                if href.is_empty() || href.contains('#') {
                    return false;
                }
                match Url::parse(href) {
                    // A relative address: a page of the same site.
                    Err(_) => true,
                    Ok(url) => url.host_str().is_some_and(|host| {
                        site.as_deref() == Some(host.trim_start_matches("www.").to_ascii_lowercase().as_str())
                    }),
                }
            })
        })
    };
    for node in dom.descendants(root) {
        if node == root || !dom.attached(node) || dom.find(node, "h1").is_some() {
            continue;
        }
        if dom.is(node, "article") {
            if weight[node] < TEASER_MAX_CHARS && other_page(dom, node) {
                dom.detach(node);
            }
            continue;
        }
        // A box of a link to another article, as a block of its own, and
        // its picture and summary (Tuổi Trẻ's `RelatedOneNews`).
        if dom.is_any(node, &["div", "section"]) && weight[node] < TEASER_MAX_CHARS && is_link_box(dom, node) {
            if other_page(dom, node) {
                dom.detach(node);
            }
            continue;
        }
        // A paragraph that is a link to another article (`Đọc thêm`).
        let link_paragraph = dom.is(node, "p")
            && weight[node] == 0
            && dom.find_all(node, "a").len() == 1
            && dom.find(node, "img").is_none()
            && dom.clean_text(node).chars().count() >= LINK_PARAGRAPH_MIN_CHARS
            && other_page(dom, node);
        if link_paragraph {
            dom.detach(node);
            continue;
        }
        let link_heading = dom.is_any(node, &HEADINGS[1..])
            && weight[node] == 0
            && !dom.clean_text(node).is_empty()
            && other_page(dom, node);
        if !link_heading {
            continue;
        }
        let mut teaser = node;
        while let Some(parent) = dom.nodes[teaser].parent {
            if parent == root || weight[parent] > TEASER_LABEL_CHARS || dom.find(parent, "h1").is_some() {
                break;
            }
            teaser = parent;
        }
        dom.detach(teaser);
    }
}

/// Whether `node` is a box of one link as a block: its children are links
/// (one of which has the text of a title), pictures and one paragraph.
fn is_link_box(dom: &Dom, node: Id) -> bool {
    let mut titles = 0;
    let mut paragraphs = 0;
    for &child in &dom.nodes[node].children {
        match &dom.nodes[child].kind {
            Kind::Text(text) if text.trim().is_empty() => {}
            Kind::Element { .. } if dom.is(child, "a") => {
                if dom.clean_text(child).chars().count() >= LINK_PARAGRAPH_MIN_CHARS {
                    titles += 1;
                }
            }
            Kind::Element { .. } if dom.is_any(child, &["p", "div"]) && dom.find(child, "a").is_none() => {
                paragraphs += 1
            }
            Kind::Element { .. } if dom.is_any(child, &["img", "picture", "figure"]) => {}
            _ => return false,
        }
    }
    titles == 1 && paragraphs <= 1
}

/// Removes what comes before the title in `root` and is short, with no
/// picture: a breadcrumb, a date, a label.
fn before_title(dom: &mut Dom, root: Id) {
    let Some(h1) = dom.find(root, "h1") else {
        return;
    };
    let mut at = h1;
    while at != root {
        let Some(parent) = dom.nodes[at].parent else {
            break;
        };
        let before: Vec<Id> = dom.nodes[parent]
            .children
            .iter()
            .copied()
            .take_while(|&child| child != at)
            .collect();
        for node in before {
            let picture = matches!(dom.nodes[node].kind, Kind::Element { .. })
                && dom
                    .descendants(node)
                    .into_iter()
                    .any(|id| dom.is_any(id, &["img", "picture", "figure", "svg"]));
            if !picture && dom.clean_text(node).chars().count() <= BEFORE_TITLE_MAX_CHARS {
                dom.detach(node);
            }
        }
        at = parent;
    }
}

/// Removes a link in `root` that repeats the link just before it, with no
/// text between them: to the same address (the author of an article, then
/// `See the articles of the author`), or with the same text when it leads
/// nowhere (`javascript:;`).
fn repeated_links(dom: &mut Dom, root: Id) {
    let mut last: Option<String> = None;
    for node in dom.descendants(root) {
        if !dom.attached(node) {
            continue;
        }
        match &dom.nodes[node].kind {
            Kind::Text(text) if !text.trim().is_empty() => {
                if dom.ancestor(node, |id| dom.is(id, "a")).is_none() {
                    last = None;
                }
            }
            Kind::Element { .. } if dom.is(node, "a") => {
                let text = dom.clean_text(node);
                if text.is_empty() {
                    continue;
                }
                let href = dom.attr(node, "href").unwrap_or_default().trim();
                let nowhere =
                    href.is_empty() || href.starts_with('#') || href.to_ascii_lowercase().starts_with("javascript:");
                let link = if nowhere {
                    format!("text:{text}")
                } else {
                    format!("href:{href}")
                };
                if last.as_ref() == Some(&link) {
                    dom.detach(node);
                } else {
                    last = Some(link);
                }
            }
            Kind::Element { .. } if dom.is_any(node, &["img", "figure", "table"]) => last = None,
            _ => {}
        }
    }
}

/// Removes a block of text in `root` that repeats the block before it
/// (a summary or a caption shown twice for wide and narrow screens, or in
/// a lightbox).
fn repeated_blocks(dom: &mut Dom, root: Id) {
    let mut last = String::new();
    for node in dom.descendants(root) {
        if node == root || !dom.attached(node) || !is_leaf_block(dom, node) {
            continue;
        }
        let text = dom.clean_text(node);
        if text.is_empty() {
            continue;
        }
        if text == last && text.chars().count() >= REPEATED_BLOCK_MIN_CHARS {
            dom.detach(node);
        } else {
            last = text;
        }
    }
}

/// Whether `node` is a block of text with no block in it: a paragraph, a
/// heading, a caption, or a `<div>` of inline content.
fn is_leaf_block(dom: &Dom, node: Id) -> bool {
    const BLOCKS: [&str; 10] = [
        "p",
        "figcaption",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "blockquote",
        "dd",
        "dt",
    ];
    const INLINE: [&str; 14] = [
        "a", "span", "em", "strong", "i", "b", "time", "br", "code", "sup", "sub", "small", "abbr", "cite",
    ];
    let is_block = dom.is_any(node, &BLOCKS) || dom.is(node, "div");
    is_block
        && dom.nodes[node]
            .children
            .iter()
            .all(|&child| match &dom.nodes[child].kind {
                Kind::Element { .. } => {
                    dom.is_any(child, &INLINE)
                        && dom
                            .descendants(child)
                            .into_iter()
                            .all(|id| !matches!(dom.nodes[id].kind, Kind::Element { .. }) || dom.is_any(id, &INLINE))
                }
                _ => true,
            })
}

/// Removes the links in `root` to the comments of the article (`43
/// Comments`).
fn comment_links(dom: &mut Dom, root: Id) {
    for link in dom.find_all(root, "a") {
        let to_comments = dom.attr(link, "href").is_some_and(|href| {
            href.split_once('#').is_some_and(|(_, fragment)| {
                fragment.to_ascii_lowercase().starts_with("comment") || fragment == "respond"
            })
        });
        if to_comments && dom.clean_text(link).chars().count() <= COMMENT_LINK_MAX_CHARS {
            // The separator before it goes too (`– Nov 23, 2026 |`), as
            // text or in an element of its own.
            let before = dom.nodes[link].parent.and_then(|parent| {
                let siblings = &dom.nodes[parent].children;
                let at = siblings.iter().position(|&id| id == link)?;
                siblings[..at]
                    .iter()
                    .rev()
                    .find(|&&id| !matches!(&dom.nodes[id].kind, Kind::Text(text) if text.trim().is_empty()))
                    .copied()
            });
            if let Some(before) = before {
                let separator =
                    |text: &str| !text.trim().is_empty() && text.trim().chars().all(|ch| SEPARATORS.contains(&ch));
                if let Kind::Text(text) = &mut dom.nodes[before].kind {
                    let kept = text.trim_end().trim_end_matches(SEPARATORS).trim_end().len();
                    text.truncate(kept);
                } else if separator(&dom.clean_text(before)) {
                    dom.detach(before);
                }
            }
            dom.detach(link);
        }
    }
}

/// Removes the blocks in `root` that hold only numbers (the counts of
/// reactions and comments: `0 0 0`) or only the word of a byline (`by`).
fn remove_counts(dom: &mut Dom, root: Id) {
    for node in dom.descendants(root) {
        if node == root || !dom.attached(node) || !dom.is_any(node, &["p", "div", "li"]) {
            continue;
        }
        let kept = ["table", "pre", "code", "ol", "h1", "h2", "h3", "h4", "h5", "h6"];
        if dom.ancestor(node, |id| dom.is_any(id, &kept)).is_some() {
            continue;
        }
        let text = dom.clean_text(node);
        // A byline whose name went with its pop-up (The Verge).
        if BYLINE_WORDS.contains(&text.to_lowercase().as_str()) {
            dom.detach(node);
            continue;
        }
        let count = text.chars().count() <= COUNT_MAX_CHARS
            && text.chars().any(|ch| ch.is_ascii_digit())
            && text.chars().all(|ch| ch.is_ascii_digit() || ch.is_whitespace());
        let parent_too = dom.nodes[node]
            .parent
            .is_some_and(|parent| parent != root && dom.clean_text(parent) == text);
        if count
            && !parent_too
            && !dom
                .descendants(node)
                .into_iter()
                .any(|id| dom.is_any(id, &["img", "svg"]))
        {
            dom.detach(node);
        }
    }
}

/// Whether `node` holds a heading (below the title) or a long paragraph,
/// not all of whose text is in links: the title of a teaser for another
/// article (a list of related articles, of tags) is not text of this one,
/// nor is a heading that is a label (`Tags:`).
fn has_article_text(dom: &Dom, node: Id) -> bool {
    let weight = text_weights(dom, node);
    dom.descendants(node).into_iter().any(|id| {
        if weight[id] == 0 {
            return false;
        }
        if dom.is_any(id, &HEADINGS[1..]) {
            return !dom.clean_text(id).ends_with(':');
        }
        // A paragraph, or the summary of a news article (in a `<div>`).
        let lead = dom
            .classes(id)
            .any(|class| words(class).any(|word| LEAD_WORDS.contains(&word.as_str())));
        (dom.is(id, "p") || lead) && dom.clean_text(id).chars().count() >= LONG_PARAGRAPH_CHARS
    })
}

/// Makes paragraphs of the headings in `root` (below the title) that are a
/// long sentence: the summary of a news article (Dân trí's `<h2
/// data-slot="sapo">`) set in a heading for its size.
fn sentence_headings(dom: &mut Dom, root: Id) {
    for heading in dom.descendants(root) {
        if !dom.is_any(heading, &HEADINGS[1..]) {
            continue;
        }
        let text = dom.clean_text(heading);
        let sentence = text.ends_with(['.', '!', '?', '…']) && text.chars().count() >= SENTENCE_HEADING_CHARS;
        if sentence {
            dom.rename(heading, "p");
        }
    }
}

/// Whether the picture `img` is the photo of an author: in a link to the
/// page of an author (`rel="author"`, `/author/`, `/tac-gia/`).
fn is_author_photo(dom: &Dom, img: Id) -> bool {
    dom.ancestor(img, |id| dom.is(id, "a")).is_some_and(|link| {
        dom.attr(link, "rel")
            .is_some_and(|rel| rel.split_whitespace().any(|rel| rel.eq_ignore_ascii_case("author")))
            || dom.attr(link, "href").is_some_and(|href| {
                let href = href.to_ascii_lowercase();
                AUTHOR_PATHS.iter().any(|path| href.contains(path))
            })
    })
}

/// Whether the picture `img` at `src` is an advertising banner by the name
/// of its file: one that starts so (`banner_taiappdetail1.jpg`), or one
/// that says so in a link (a sponsor's `railway-banner.png`); a photo
/// named so (`Museumplein-Amsterdam-banner.jpg`) is not.
fn is_banner(dom: &Dom, img: Id, src: &str) -> bool {
    let path = src.split(['?', '#']).next().unwrap_or(src);
    let file = path.rsplit('/').next().unwrap_or(path);
    if file.starts_with("data:") {
        return false;
    }
    let mut names = words(file);
    let first = names.next().is_some_and(|word| word.starts_with("banner"));
    first || (dom.ancestor(img, |id| dom.is(id, "a")).is_some() && names.any(|word| word.starts_with("banner")))
}

/// Gives each picture in `body` the address of its best version (from a
/// `srcset`, a `<picture>`'s sources or the attribute of a lazy picture),
/// made absolute, and a size that fits the page; a picture without an
/// address goes.
fn pictures(dom: &mut Dom, body: Id, base: &Base<'_>) {
    for img in dom.find_all(body, "img") {
        let Some(src) = best_source(dom, img) else {
            dom.detach(img);
            continue;
        };
        let src = absolute(&src, base);
        let others: Vec<String> = other_sources(dom, img, &src)
            .into_iter()
            .map(|other| absolute(&other, base))
            .filter(|other| *other != src)
            .take(MAX_FALLBACKS)
            .collect();
        if !others.is_empty() {
            dom.set_attr(img, FALLBACK_ATTR, &others.join(" "));
        }
        dom.set_attr(img, "src", &src);
        for key in SRCSETS.iter().chain(&LAZY_SOURCES).chain(&["sizes"]) {
            dom.remove_attr(img, key);
        }
        let size = |key: &str| {
            dom.attr(img, key)
                .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
        };
        let (width, height) = (size("width"), size("height"));
        if width.is_some_and(|width| width < MIN_PICTURE_PX)
            || height.is_some_and(|height| height < MIN_PICTURE_PX)
            || is_icon(dom, img)
            || is_author_photo(dom, img)
            || is_banner(dom, img, &src)
        {
            dom.detach(img);
            continue;
        }
        if let Some(width) = width.filter(|&width| width > MAX_PICTURE_WIDTH_PX) {
            dom.set_attr(img, "width", &format!("{MAX_PICTURE_WIDTH_PX}"));
            if let Some(height) = height {
                let fitted = height * MAX_PICTURE_WIDTH_PX / width;
                dom.set_attr(img, "height", &format!("{fitted:.0}"));
            }
        }
        // A size in pixels in the style is the layout of the moment; one
        // in inches or centimetres (as pandoc writes it) is the author's.
        let absolute_size = dom.attr(img, "style").is_some_and(|style| {
            let style = style.to_ascii_lowercase();
            ["in", "cm", "mm", "pt"]
                .iter()
                .any(|unit| style.split(';').any(|decl| decl.trim_end().ends_with(unit)))
        });
        if !absolute_size {
            dom.remove_attr(img, "style");
        }
    }
    for source in dom.find_all(body, "source") {
        dom.detach(source);
    }
    // SVG pictures linking to a file.
    for node in dom.descendants(body) {
        if dom.space(node) == Some(Space::Svg) && dom.name(node) == Some("image") {
            for key in ["xlink:href", "href"] {
                if let Some(href) = dom.attr(node, key).map(str::to_string) {
                    dom.set_attr(node, key, &absolute(&href, base));
                }
            }
        }
    }
}

/// The address of the best version of the picture `img`.
fn best_source(dom: &Dom, img: Id) -> Option<String> {
    let mut sets: Vec<String> = SRCSETS
        .iter()
        .filter_map(|key| dom.attr(img, key).map(str::to_string))
        .collect();
    if let Some(picture) = dom.nodes[img].parent.filter(|&parent| dom.is(parent, "picture")) {
        for source in dom.element_children(picture) {
            let drawn = dom
                .attr(source, "type")
                .is_none_or(|kind| !UNDRAWN_TYPES.contains(&kind.trim().to_ascii_lowercase().as_str()));
            if dom.is(source, "source") && drawn {
                sets.extend(
                    SRCSETS
                        .iter()
                        .filter_map(|key| dom.attr(source, key).map(str::to_string)),
                );
            }
        }
    }
    let from_set = sets.iter().find_map(|set| best_in_srcset(set));
    let src = dom.attr(img, "src").filter(|src| !is_placeholder(src));
    let lazy = LAZY_SOURCES
        .iter()
        .find_map(|key| dom.attr(img, key).filter(|src| !is_placeholder(src)));
    from_set
        .or_else(|| lazy.map(str::to_string))
        .or_else(|| src.map(str::to_string))
        .map(|src| src.trim().to_string())
}

/// The other addresses of the picture `img` than `best`, its best: the
/// other versions of its `srcset`s, widest first, then its lazy and plain
/// addresses (to fetch when the best fails).
fn other_sources(dom: &Dom, img: Id, best: &str) -> Vec<String> {
    let mut versions: Vec<(String, f32)> = SRCSETS
        .iter()
        .filter_map(|key| dom.attr(img, key))
        .flat_map(parse_srcset)
        .collect();
    versions.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out: Vec<String> = Vec::new();
    let plain = LAZY_SOURCES
        .iter()
        .chain(&["src"])
        .filter_map(|key| dom.attr(img, key))
        .filter(|src| !is_placeholder(src))
        .map(|src| src.trim().to_string());
    for url in versions.into_iter().map(|(url, _)| url).chain(plain) {
        if url != best && !url.starts_with("data:") && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The best address of a `srcset`: the smallest version at least
/// [`PREFERRED_PICTURE_WIDTH_PX`] wide (or of the highest density), the
/// widest otherwise.
fn best_in_srcset(set: &str) -> Option<String> {
    let candidates = parse_srcset(set);
    let wide = candidates
        .iter()
        .filter(|(_, width)| *width >= PREFERRED_PICTURE_WIDTH_PX)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    wide.or_else(|| candidates.iter().max_by(|a, b| a.1.total_cmp(&b.1)))
        .map(|(url, _)| url.clone())
}

/// The candidates of a `srcset`: address and width (a density `2x` counts
/// as 2 pixels, enough to order the densities; no descriptor as 1x).
fn parse_srcset(set: &str) -> Vec<(String, f32)> {
    let mut out = Vec::new();
    let mut rest = set;
    loop {
        rest = rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == ',');
        if rest.is_empty() {
            break;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let mut url = &rest[..end];
        rest = &rest[end..];
        let mut descriptor = "";
        if url.ends_with(',') {
            url = url.trim_end_matches(',');
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            descriptor = rest[..end].trim();
            rest = &rest[end..];
        }
        let size = if let Some(width) = descriptor.strip_suffix('w') {
            width.parse().ok()
        } else if let Some(density) = descriptor.strip_suffix('x') {
            density.parse().ok()
        } else {
            Some(1.0)
        };
        if let Some(size) = size
            && !url.is_empty()
        {
            out.push((url.to_string(), size));
        }
    }
    out
}

/// The address `href` of a picture or a style sheet made absolute against
/// the page (unchanged for a page without an address).
fn absolute(href: &str, base: &Base<'_>) -> String {
    let href = href.trim();
    match base {
        Base::Url(base) | Base::File(base) => base.join(href).map_or_else(|_| href.to_string(), String::from),
        Base::None => href.to_string(),
    }
}

/// Makes the links in `root` absolute; a link to a part of the page itself
/// becomes `#part`, and a `javascript:` link is no link.
fn absolute_links(dom: &mut Dom, root: Id, base: &Base<'_>) {
    for link in dom.find_all(root, "a") {
        let Some(href) = dom.attr(link, "href").map(|href| href.trim().to_string()) else {
            continue;
        };
        if href.to_ascii_lowercase().starts_with("javascript:") || href.is_empty() {
            dom.remove_attr(link, "href");
            continue;
        }
        if href.starts_with('#') {
            continue;
        }
        let Base::Url(base) = base else {
            continue;
        };
        let Ok(mut target) = base.join(&href) else {
            continue;
        };
        let fragment = target.fragment().map(str::to_string);
        target.set_fragment(None);
        let mut page = (*base).clone();
        page.set_fragment(None);
        let href = match fragment {
            Some(fragment) if target == page => format!("#{fragment}"),
            _ => {
                if let Some(fragment) = &fragment {
                    target.set_fragment(Some(fragment));
                }
                target.to_string()
            }
        };
        dom.set_attr(link, "href", &href);
    }
}

/// Removes the icons in `root`: inline SVG drawings smaller than
/// [`MIN_PICTURE_PX`] (by their size or their `viewBox`).
fn remove_icons(dom: &mut Dom, root: Id) {
    for node in dom.descendants(root) {
        if dom.space(node) != Some(Space::Svg) || dom.name(node) != Some("svg") || !dom.attached(node) {
            continue;
        }
        let number = |key: &str| {
            dom.attr(node, key)
                .and_then(|value| value.trim().trim_end_matches("px").parse::<f32>().ok())
        };
        let view = dom.attr(node, "viewBox").and_then(|view| {
            let numbers: Vec<f32> = view
                .split([' ', ','])
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            (numbers.len() == 4).then(|| (numbers[2], numbers[3]))
        });
        let width = number("width").or(view.map(|view| view.0));
        let height = number("height").or(view.map(|view| view.1));
        if width.is_some_and(|width| width < MIN_PICTURE_PX)
            || height.is_some_and(|height| height < MIN_PICTURE_PX)
            || is_icon(dom, node)
        {
            dom.detach(node);
        }
    }
}

/// Whether the picture `node` (`<img>` or `<svg>`) is an icon or a logo of
/// the site: smaller than [`MIN_PICTURE_PX`] by its utility classes
/// (`h-4 w-4`: the size a style sheet gives it, whatever the size of its
/// file or drawing), or named so ([`ICON_WORDS`]) by its class or `id` or
/// those of what holds it. A picture with a caption is the article's.
fn is_icon(dom: &Dom, node: Id) -> bool {
    let (width, height) = class_size_px(dom, node);
    if width.is_some_and(|width| width < MIN_PICTURE_PX) || height.is_some_and(|height| height < MIN_PICTURE_PX) {
        return true;
    }
    if dom.ancestor(node, |id| dom.is(id, "figure")).is_some() {
        return false;
    }
    let mut at = Some(node);
    for _ in 0..=ICON_ANCESTORS {
        let Some(id) = at else {
            break;
        };
        if dom.is_any(id, &["body", "main", "article"]) {
            break;
        }
        let mut names = String::new();
        names.push_str(dom.attr(id, "class").unwrap_or_default());
        names.push(' ');
        names.push_str(dom.attr(id, "id").unwrap_or_default());
        if words(&names).any(|word| ICON_WORDS.contains(&word.as_str())) {
            return true;
        }
        at = dom.nodes[id].parent;
    }
    false
}

/// The width and height, in CSS pixels, that the utility classes of `node`
/// give it (`w-5`, `h-[36px]`, `size-4`, `md:h-12`: the largest of each,
/// so that a picture shown large on some screen is not an icon).
fn class_size_px(dom: &Dom, node: Id) -> (Option<f32>, Option<f32>) {
    let mut width: Option<f32> = None;
    let mut height: Option<f32> = None;
    for class in dom.classes(node) {
        // `md:h-12`: the size on some screens.
        let class = class.rsplit(':').next().unwrap_or(class);
        let Some((axis, value)) = class.split_once('-') else {
            continue;
        };
        let px = if let Some(inner) = value.strip_prefix('[').and_then(|value| value.strip_suffix(']')) {
            if let Some(number) = inner.strip_suffix("px") {
                number.parse::<f32>().ok()
            } else if let Some(number) = inner.strip_suffix("rem").or_else(|| inner.strip_suffix("em")) {
                number.parse::<f32>().ok().map(|number| number * REM_PX)
            } else {
                None
            }
        } else {
            value.parse::<f32>().ok().map(|steps| steps * TAILWIND_STEP_PX)
        };
        let Some(px) = px else {
            continue;
        };
        let grow = |old: Option<f32>| Some(old.map_or(px, |old: f32| old.max(px)));
        match axis {
            "w" => width = grow(width),
            "h" => height = grow(height),
            "size" => {
                width = grow(width);
                height = grow(height);
            }
            _ => {}
        }
    }
    (width, height)
}

/// The links of the headings in `root` to themselves: a mark (`¶`) goes,
/// a link around the text of the heading leaves the text.
fn heading_self_links(dom: &mut Dom, root: Id) {
    for link in dom.find_all(root, "a") {
        let Some(heading) = dom.ancestor(link, |id| dom.is_any(id, &HEADINGS)) else {
            continue;
        };
        let Some(fragment) = dom.attr(link, "href").and_then(|href| href.strip_prefix('#')) else {
            continue;
        };
        let text = dom.clean_text(link);
        if SELF_LINK_MARKS.contains(&text.as_str()) {
            dom.detach(link);
            continue;
        }
        // The heading, or the section it opens, is the target.
        let mut at = Some(heading);
        let mut targets_itself = false;
        while let Some(id) = at {
            if dom.attr(id, "id") == Some(fragment) {
                targets_itself = true;
                break;
            }
            at = dom.nodes[id].parent.filter(|&parent| parent != root);
        }
        if targets_itself || dom.attr(link, "id") == Some(fragment) {
            dom.unwrap(link);
        }
    }
}

/// Two line breaks in a row (`<br><br>`, white space between) end a
/// paragraph: pages written so get their paragraphs, as in a browser.
fn split_at_double_breaks(dom: &mut Dom, root: Id) {
    for br in dom.find_all(root, "br") {
        if !dom.attached(br) {
            continue;
        }
        let Some(parent) = dom.nodes[br].parent else {
            continue;
        };
        let siblings = dom.nodes[parent].children.clone();
        let Some(at) = siblings.iter().position(|&child| child == br) else {
            continue;
        };
        let next = siblings[at + 1..]
            .iter()
            .copied()
            .find(|&child| !matches!(&dom.nodes[child].kind, Kind::Text(text) if text.trim().is_empty()));
        if let Some(next) = next.filter(|&next| dom.is(next, "br")) {
            // An empty block between the two parts: the reader ends the
            // paragraph there.
            let block = dom.element("div", &[]);
            dom.insert_after(br, block);
            dom.detach(br);
            dom.detach(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Dom, best_in_srcset, is_author_photo, is_banner, is_icon, parse_srcset, without_site_name};

    #[test]
    fn banners_and_author_photos() {
        let dom = Dom::parse(
            r#"<p><img id=a src="https://x.vn/images/banner_taiapp.jpg">
<img id=b src="https://x.vn/Museumplein-Amsterdam-banner.jpg">
<a href="https://sponsor.example"><img id=c src="/img/railway-banner.png"></a>
<a href="/tac-gia/chi-tue.htm"><img id=d src="/avatar/chi-tue.jpg"></a></p>"#,
        );
        let find = |dom: &Dom, id: &str| {
            dom.find_all(0, "img")
                .into_iter()
                .find(|&img| dom.attr(img, "id") == Some(id))
                .unwrap()
        };
        let banner = |dom: &Dom, id: &str| {
            let img = find(dom, id);
            let src = dom.attr(img, "src").unwrap().to_string();
            is_banner(dom, img, &src)
        };
        assert!(banner(&dom, "a"));
        assert!(!banner(&dom, "b"));
        assert!(banner(&dom, "c"));
        assert!(is_author_photo(&dom, find(&dom, "d")));
        assert!(!is_author_photo(&dom, find(&dom, "c")));
    }

    #[test]
    fn icons_are_known_by_their_classes() {
        let dom = Dom::parse(
            r#"<body><a class="view-comments"><svg class="h-4 w-4" viewBox="0 0 80 80"></svg></a>
            <svg class="h-[36px] md:h-[65px]" viewBox="0 0 436 144"></svg>
            <div class="div-logo"><a><img src="logo.png"></a></div>
            <img class="info-icon" src="sun.png">
            <figure><div class="logo"><img src="brand.png"></div><figcaption>The brand.</figcaption></figure>
            <img class="w-full" src="photo.jpg"></body>"#,
        );
        let icons: Vec<bool> = dom
            .descendants(0)
            .into_iter()
            .filter(|&id| dom.is(id, "img") || dom.name(id) == Some("svg"))
            .map(|id| is_icon(&dom, id))
            .collect();
        assert_eq!(icons, [true, false, true, true, false, false]);
    }

    #[test]
    fn srcsets_and_titles() {
        let set = "a.jpg?c=16x9&q=h_27,w_640 640w, b.jpg 1920w,c.jpg 3840w";
        assert_eq!(parse_srcset(set).len(), 3);
        assert_eq!(best_in_srcset(set).as_deref(), Some("b.jpg"));
        assert_eq!(best_in_srcset("a.png, b.png 2x").as_deref(), Some("b.png"));
        assert_eq!(best_in_srcset("a.png 300w, b.png 800w").as_deref(), Some("b.png"));
        assert_eq!(
            without_site_name("Gravitational wave - Wikipedia"),
            "Gravitational wave"
        );
        assert_eq!(
            without_site_name("Writing effective tools for AI agents \\ Anthropic"),
            "Writing effective tools for AI agents"
        );
        assert_eq!(without_site_name("A - B"), "A");
        assert_eq!(without_site_name("Plain"), "Plain");
    }
}
