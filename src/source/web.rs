//! Fetching documents from the Web: a page (or a PDF, a Word document, an
//! EPUB book) at an `http(s)` address, and the pictures of a page, many at
//! once.

use crate::common::cancel::{self, CancelToken};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

/// Give up on a page after this long.
const PAGE_TIMEOUT: Duration = Duration::from_secs(60);
/// Give up on one picture after this long.
const PICTURE_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest document accepted, in bytes.
const MAX_DOCUMENT_BYTES: u64 = 300 * 1024 * 1024;
/// Largest picture accepted, in bytes.
const MAX_PICTURE_BYTES: u64 = 25 * 1024 * 1024;
/// Most pictures fetched for one page.
pub const MAX_PICTURES: usize = 300;
/// Tries at a picture (or a page) that fails for a reason that may pass: a
/// connection lost, a server error, too many requests at once.
const ATTEMPTS: u32 = 3;
/// Wait before the second try; it doubles at each try.
const RETRY_WAIT: Duration = Duration::from_secs(1);
/// Pictures fetched at the same time.
const PICTURE_WORKERS: usize = 8;
/// What is asked for a page: HTML first, any document otherwise.
const PAGE_ACCEPT: &str = "text/html,application/xhtml+xml;q=0.9,application/pdf;q=0.8,*/*;q=0.5";
/// What is asked for a picture: the formats that can be drawn (not AVIF).
const PICTURE_ACCEPT: &str = "image/png,image/jpeg,image/webp,image/gif,image/svg+xml;q=0.9,image/*;q=0.5";

/// A document fetched from the Web.
#[derive(Debug, Clone)]
pub struct Fetched {
    /// The address it came from, after redirections.
    pub url: String,
    /// Its media type (`Content-Type`), if the server gave one.
    pub content_type: Option<String>,
    /// Its bytes.
    pub data: Vec<u8>,
}

impl Fetched {
    /// Whether the server says it is an HTML page.
    pub fn is_html(&self) -> bool {
        self.content_type.as_deref().is_some_and(|kind| {
            let kind = kind.to_ascii_lowercase();
            kind.starts_with("text/html") || kind.starts_with("application/xhtml+xml")
        })
    }

    /// The `charset` its `Content-Type` names.
    pub fn charset(&self) -> Option<&str> {
        let kind = self.content_type.as_deref()?;
        kind.split(';').skip(1).find_map(|param| {
            let (key, value) = param.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("charset")
                .then(|| value.trim().trim_matches('"'))
        })
    }
}

/// Whether `input` is the address of a page of the Web (`http://` or
/// `https://`).
pub fn is_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && url::Url::parse(input.trim()).is_ok()
}

/// The HTTP client: the proxy of the environment, a timeout, and a name
/// that says what it is (as sites such as Wikipedia ask).
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout(timeout)
        .user_agent(concat!(
            "kakemono/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/iamcaominhtien/kakemono)"
        ))
        .build()
}

/// Fetches the document at `url`.
///
/// # Errors
/// When `url` is not an `http(s)` address, the request fails or times
/// out, the server answers with an error, or the document is too large.
pub fn fetch(url: &str) -> Result<Fetched> {
    if !is_url(url) {
        bail!("'{url}' is not an http(s) address");
    }
    get_retrying(&agent(PAGE_TIMEOUT), url.trim(), PAGE_ACCEPT, MAX_DOCUMENT_BYTES)
}

/// [`get`], tried again (up to [`ATTEMPTS`] times) when it fails for a
/// reason that may pass: no answer, a lost connection, a server error
/// (5xx) or too many requests (429). A page that is missing (404) or
/// refused (403) is not asked again.
fn get_retrying(agent: &ureq::Agent, url: &str, accept: &'static str, max_bytes: u64) -> Result<Fetched> {
    let mut wait = RETRY_WAIT;
    let mut attempt = 1;
    loop {
        let result = get(agent, url, accept, max_bytes);
        let passing = match &result {
            Ok(_) => false,
            Err(error) => match error.downcast_ref::<ureq::Error>() {
                Some(ureq::Error::Status(code, _)) => *code == 429 || *code >= 500,
                Some(ureq::Error::Transport(_)) => true,
                None => error.chain().any(|cause| cause.is::<std::io::Error>()),
            },
        };
        if !passing || attempt >= ATTEMPTS {
            return result;
        }
        cancel::sleep(wait);
        wait *= 2;
        attempt += 1;
    }
}

/// One request: on a thread of its own when the work can be cancelled
/// ([`cancel::blocking`]), since waiting for an answer has no stopping point.
fn get(agent: &ureq::Agent, url: &str, accept: &'static str, max_bytes: u64) -> Result<Fetched> {
    let (agent, url) = (agent.clone(), url.to_string());
    cancel::blocking(move |token| get_now(&agent, &url, accept, max_bytes, token.as_ref()))
}

/// [`get`], on this thread.
fn get_now(
    agent: &ureq::Agent,
    url: &str,
    accept: &str,
    max_bytes: u64,
    token: Option<&CancelToken>,
) -> Result<Fetched> {
    let resp = agent
        .get(url)
        .set("Accept", accept)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let final_url = resp.get_url().to_string();
    let content_type = resp.header("Content-Type").map(str::to_string);
    let data = read_body(resp.into_reader(), max_bytes + 1, token).with_context(|| format!("reading {url}"))?;
    if data.len() as u64 > max_bytes {
        bail!("{url} is larger than {max_bytes} bytes");
    }
    Ok(Fetched {
        url: final_url,
        content_type,
        data,
    })
}

/// Bytes read from a download at a time, between two looks at its token.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// The bytes of `reader`, at most `max_bytes` of them, read in chunks of
/// [`READ_CHUNK_BYTES`]; stops with an error once `token` stops, so that a
/// download left by a cancelled conversion does not go on.
///
/// # Errors
/// When reading fails, or `token` stops.
pub(crate) fn read_body(reader: impl Read, max_bytes: u64, token: Option<&CancelToken>) -> std::io::Result<Vec<u8>> {
    let mut reader = reader.take(max_bytes);
    let mut data = Vec::new();
    let mut chunk = vec![0; READ_CHUNK_BYTES];
    loop {
        if let Some(stopped) = token.and_then(CancelToken::state) {
            return Err(std::io::Error::other(stopped));
        }
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(data),
            Ok(read) => data.extend_from_slice(&chunk[..read]),
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

/// Fetches the pictures at `urls` (at most [`MAX_PICTURES`], several at a
/// time): the bytes of each one fetched, by address. A picture that cannot
/// be fetched is left out.
pub fn fetch_pictures(urls: &[String]) -> HashMap<String, Vec<u8>> {
    let agent = agent(PICTURE_TIMEOUT);
    let queue = Mutex::new(urls.iter().take(MAX_PICTURES));
    let found = Mutex::new(HashMap::new());
    let token = cancel::current();
    std::thread::scope(|scope| {
        for _ in 0..PICTURE_WORKERS.min(urls.len()) {
            scope.spawn(|| {
                // The lock is held only to take the next address.
                let next = || queue.lock().ok().and_then(|mut queue| queue.next());
                let work = || {
                    while let Some(url) = next() {
                        if let Ok(fetched) = get_retrying(&agent, url, PICTURE_ACCEPT, MAX_PICTURE_BYTES)
                            && let Ok(mut found) = found.lock()
                        {
                            found.insert(url.clone(), fetched.data);
                        }
                    }
                };
                // A cancelled worker stops here: a scoped thread that
                // unwinds would lose why, and the caller stops below.
                match &token {
                    Some(token) => drop(token.run(work)),
                    None => work(),
                }
            });
        }
    });
    cancel::check();
    found.into_inner().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Fetched, is_url};

    #[test]
    fn addresses_and_types() {
        assert!(is_url("https://en.wikipedia.org/wiki/Rust_(programming_language)"));
        assert!(is_url("HTTP://example.org"));
        assert!(!is_url("example.org/page"));
        assert!(!is_url("1706.03762"));
        let page = Fetched {
            url: String::new(),
            content_type: Some("text/html; charset=ISO-8859-1".into()),
            data: Vec::new(),
        };
        assert!(page.is_html());
        assert_eq!(page.charset(), Some("ISO-8859-1"));
    }
}
