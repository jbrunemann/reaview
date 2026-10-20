//! Is there a newer release? Asks a web address and compares versions.
//!
//! The address answers with JSON in one of two shapes:
//!
//! - GitHub's "latest release" (`https://api.github.com/repos/<owner>/<repo>/releases/latest`):
//!   `tag_name` is the version, `html_url` the page to download it from.
//! - A file of its own, on any server: `{"version": "1.5.0", "url": "https://example.org/download"}`.
//!   `url` is optional.
//!
//! Nothing here runs by itself: the app asks when the user chooses
//! "Check for Updates…". The request goes through `NSURLSession`, which works
//! inside the App Sandbox (with the network client entitlement) where
//! spawning `curl` would be one more thing to allow. The ported parts are from
//! macsplorer's `update-check` crate.

use std::cell::Cell;
use std::cmp::Ordering;
use std::fmt;

use block2::RcBlock;
use objc2_foundation::{
    NSData, NSError, NSOperationQueue, NSString, NSURL, NSURLResponse, NSURLSession,
    NSURLSessionConfiguration, NSHTTPURLResponse, NSUserDefaults,
};
use serde_json::Value;

/// Where "Check for Updates…" asks by default: GitHub's latest release.
pub const DEFAULT_SOURCE: &str = "https://api.github.com/repos/jbrunemann/reaview/releases/latest";
/// User defaults key that replaces [`DEFAULT_SOURCE`]
/// (`defaults write dev.reaview.ai UpdateURL <address>`).
const SOURCE_KEY: &str = "UpdateURL";
/// How long a request may wait for an answer, in seconds.
const TIMEOUT_SECS: f64 = 15.0;
/// Longer answers are refused; a release document is a few kilobytes.
const MAX_BYTES: usize = 1 << 20;

/// What the source says about the newest release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The running version is the newest or newer (a development build).
    UpToDate { latest: String },
    Available(Release),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version as published, without a leading `v`.
    pub version: String,
    /// Where to get it: always an `http` or `https` address, so the app can open it without looking.
    pub page: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The source isn't an `http(s)` address.
    Source(String),
    /// The request failed: no network, a server error, a timeout.
    Fetch(String),
    /// The answer isn't a release document, or its version can't be read.
    Reply(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Source(source) => write!(f, "\"{source}\" is not a web address"),
            Error::Fetch(why) | Error::Reply(why) => f.write_str(why),
        }
    }
}

/// The address to ask: the user default `UpdateURL`, else [`DEFAULT_SOURCE`].
pub fn source() -> String {
    let stored = NSUserDefaults::standardUserDefaults().stringForKey(&NSString::from_str(SOURCE_KEY));
    stored.map(|s| s.to_string().trim().to_owned()).filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_SOURCE.to_owned())
}

/// Asks `source` for the newest release and compares it with `current`.
/// `done` is called on the main thread once the answer is in; this returns at once.
pub fn check(source: &str, current: &str, done: impl FnOnce(Result<Status, Error>) + 'static) {
    let source = source.trim();
    let url = is_web_address(source).then(|| NSURL::URLWithString(&NSString::from_str(source))).flatten();
    let Some(url) = url else { return done(Err(Error::Source(source.to_owned()))) };

    let current = current.to_owned();
    let done = Cell::new(Some(done));
    // Runs on the main queue (see the session below), so `done` may touch the UI.
    let handler = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
        let Some(done) = done.take() else { return };
        // SAFETY: NSURLSession passes either null or live objects for the duration of the call.
        let (data, response, error) = unsafe { (data.as_ref(), response.as_ref(), error.as_ref()) };
        done(answer(data, response, error, &current));
    });
    let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    configuration.setTimeoutIntervalForRequest(TIMEOUT_SECS);
    let session = unsafe {
        NSURLSession::sessionWithConfiguration_delegate_delegateQueue(&configuration, None, Some(&NSOperationQueue::mainQueue()))
    };
    let task = unsafe { session.dataTaskWithURL_completionHandler(&url, &handler) };
    task.resume();
    session.finishTasksAndInvalidate();
}

fn answer(data: Option<&NSData>, response: Option<&NSURLResponse>, error: Option<&NSError>, current: &str) -> Result<Status, Error> {
    if let Some(error) = error {
        return Err(Error::Fetch(error.localizedDescription().to_string()));
    }
    let status = response.and_then(|r| r.downcast_ref::<NSHTTPURLResponse>()).map(|r| r.statusCode());
    if let Some(code) = status.filter(|code| !(200..300).contains(code)) {
        return Err(Error::Fetch(format!("The server answered with error {code}")));
    }
    let body = data.map(|d| d.to_vec()).unwrap_or_default();
    if body.len() > MAX_BYTES {
        return Err(Error::Reply("The answer is too long".to_owned()));
    }
    parse(&String::from_utf8_lossy(&body), current)
}

/// Reads the source's answer. Separate from [`check`] so it can be tested without a network.
pub fn parse(body: &str, current: &str) -> Result<Status, Error> {
    let reply = |why: &str| Error::Reply(why.to_owned());
    let json: Value = serde_json::from_str(body).map_err(|_| reply("The answer is not JSON"))?;
    let text = |keys: &[&str]| keys.iter().find_map(|k| json.get(k)?.as_str()).map(str::trim).filter(|t| !t.is_empty());
    let published = text(&["tag_name", "version"]).ok_or_else(|| reply("The answer names no version"))?;
    let latest = Version::parse(published).ok_or_else(|| Error::Reply(format!("\"{published}\" is not a version")))?;
    let running = Version::parse(current).ok_or_else(|| Error::Reply(format!("The running version \"{current}\" is not a version")))?;
    let version = published.trim_start_matches(['v', 'V']).to_owned();
    if latest > running {
        let page = text(&["html_url", "url"]).filter(|p| is_web_address(p)).map(str::to_owned);
        Ok(Status::Available(Release { version, page }))
    } else {
        Ok(Status::UpToDate { latest: version })
    }
}

fn is_web_address(text: &str) -> bool {
    let lower = text.get(..8).unwrap_or(text).to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://")) && !text.contains(char::is_whitespace)
}

/// Version numbers as release tags write them: `1.4.1`, `v2.0`, `1.5.0-rc.1`.
/// Dotted numbers with an optional pre-release (`-rc.1`), ordered as semver
/// orders them, except that a missing number counts as 0 (`1.5` is `1.5.0`).
/// A `+build` part is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    numbers: Vec<u64>,
    pre: Option<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_owned()).filter(|p| !p.is_empty())),
            None => (text, None),
        };
        let mut numbers = core.split('.').map(|n| n.parse().ok()).collect::<Option<Vec<u64>>>()?;
        // Without the trailing zeros `1.5` and `1.5.0` are equal as values too, not only in order.
        while numbers.last() == Some(&0) {
            numbers.pop();
        }
        Some(Version { numbers, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.numbers.len().max(other.numbers.len());
        let number = |v: &Version, i: usize| v.numbers.get(i).copied().unwrap_or(0);
        (0..len)
            .map(|i| number(self, i).cmp(&number(other, i)))
            .find(|o| o.is_ne())
            .unwrap_or_else(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A release comes after its pre-releases.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_pre(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Dot-separated parts: numbers by value and before words, words as text,
/// and the shorter list first when one is the start of the other.
fn compare_pre(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order.is_ne() {
                    return order;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB: &str = r#"{"tag_name": "v1.5.0", "name": "Reaview 1.5.0", "html_url": "https://github.com/o/r/releases/tag/v1.5.0", "assets": []}"#;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn a_newer_github_release_is_available() {
        let release = Release { version: "1.5.0".into(), page: Some("https://github.com/o/r/releases/tag/v1.5.0".into()) };
        assert_eq!(parse(GITHUB, "1.4.1"), Ok(Status::Available(release)));
    }

    #[test]
    fn the_same_or_a_newer_running_version_is_up_to_date() {
        assert_eq!(parse(GITHUB, "1.5.0"), Ok(Status::UpToDate { latest: "1.5.0".into() }));
        assert_eq!(parse(GITHUB, "1.6.0-dev"), Ok(Status::UpToDate { latest: "1.5.0".into() }));
    }

    #[test]
    fn a_plain_file_works_too() {
        let body = r#"{"version": "2.0", "url": "https://example.org/get"}"#;
        let release = Release { version: "2.0".into(), page: Some("https://example.org/get".into()) };
        assert_eq!(parse(body, "1.9.9"), Ok(Status::Available(release)));
        assert_eq!(parse(r#"{"version": "2.0"}"#, "1.0"), Ok(Status::Available(Release { version: "2.0".into(), page: None })));
    }

    #[test]
    fn a_page_that_is_not_a_web_address_is_dropped() {
        let body = r#"{"version": "2.0", "url": "file:///Applications/Evil.app"}"#;
        assert_eq!(parse(body, "1.0"), Ok(Status::Available(Release { version: "2.0".into(), page: None })));
    }

    #[test]
    fn answers_that_are_not_release_documents_are_errors() {
        assert!(matches!(parse("<html>", "1.0"), Err(Error::Reply(_))));
        assert!(matches!(parse("{}", "1.0"), Err(Error::Reply(_))));
        assert!(matches!(parse(r#"{"tag_name": "nightly"}"#, "1.0"), Err(Error::Reply(_))));
        assert!(matches!(parse(GITHUB, "unknown"), Err(Error::Reply(_))));
    }

    #[test]
    fn only_web_addresses_are_asked() {
        for source in ["", "file:///etc/passwd", "ftp://example.org/x", "-o /tmp/x", "https://exa mple.org"] {
            assert!(!is_web_address(source), "{source}");
        }
        assert!(is_web_address("HTTPS://example.org/latest.json"));
    }

    #[test]
    fn numbers_compare_by_value_not_as_text() {
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("v2.0.0") > v("1.99.99"));
        assert_eq!(v("1.5"), v("1.5.0"));
        assert!(v("1.5.1") > v("1.5"));
    }

    #[test]
    fn a_pre_release_comes_before_its_release() {
        assert!(v("1.5.0-rc.1") < v("1.5.0"));
        assert!(v("1.5.0-rc.2") > v("1.5.0-rc.1"));
        assert!(v("1.5.0-rc.10") > v("1.5.0-rc.2"));
        assert!(v("1.5.0-rc") < v("1.5.0-rc.1"));
        assert!(v("1.5.0-beta") < v("1.5.0-rc"));
        assert!(v("1.4.9") < v("1.5.0-rc.1"));
    }

    #[test]
    fn build_metadata_is_ignored() {
        assert_eq!(v("1.5.0+abc"), v("1.5.0"));
    }

    #[test]
    fn things_that_are_not_versions_are_refused() {
        for text in ["", "nightly", "1..2", "1.x", "v", "latest-1.0"] {
            assert_eq!(Version::parse(text), None, "{text}");
        }
    }
}
