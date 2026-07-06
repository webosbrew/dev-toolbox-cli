//! Frontend framework and JavaScript syntax-level detection for a web app.
//!
//! Accuracy comes from parsing rather than regex-ing the source:
//! - JS ES-feature detection runs over a **token stream** (`ress`), so a
//!   keyword or operator inside a string or comment can't false-match (the
//!   classic `/**`-as-`**` problem).
//! - HTML signals (`<script type="module">`, remote `src`/`href`, the Angular
//!   `ng-version` attribute) come from a real **DOM** (`tl`), not attribute
//!   regexes.
//!
//! Framework *identity* is still matched from unambiguous license banners in
//! the JS text — those live in comments and are reliable as plain substrings.
//! The ES-level result stays a conservative floor ("the engine must support at
//! least this").

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use semver::Version;

use crate::js::{self, MAX_FILE_BYTES};
use crate::{FrameworkInfo, FrameworkKind, WebAppDetection};

/// Cap on distinct remote resource URLs reported.
const MAX_REMOTE: usize = 20;

/// Detect the framework, webOSTV.js SDK, ES syntax level, runtime APIs and
/// remote resources of the web app rooted at `dir`, whose HTML entry point is
/// `index_html`.
pub fn detect_web_app(dir: &Path, index_html: &Path) -> WebAppDetection {
    let html = read_capped(index_html);

    let mut sources: Vec<(String, String)> = Vec::new();
    js::collect_js(dir, 0, &mut sources);

    let signals = detect_html_signals(&html);
    // Many webOS apps (Enyo/Enact single-file builds) inline all JS into
    // index.html rather than shipping separate .js files; include those inline
    // scripts so ES-level detection sees them too.
    if !signals.inline_js.is_empty() {
        sources.push(("index.html".to_string(), signals.inline_js.clone()));
    }
    let (framework, also_present, webostvjs) =
        detect_frameworks(&html, &sources, signals.ng_version.as_deref());
    let analysis = js::analyze_js(&sources, signals.has_module);

    WebAppDetection {
        framework,
        also_present,
        webostvjs,
        es_level: analysis.es_level,
        es_features: analysis.es_features,
        es_apis: analysis.es_apis,
        polyfills: js::detect_polyfills(&sources),
        remote_resources: signals.remote_resources,
    }
}

/// Read a file to a string, but skip (return empty) anything larger than
/// [`MAX_FILE_BYTES`] so a pathological entry can't exhaust memory.
fn read_capped(path: &Path) -> String {
    if fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return String::new();
    }
    fs::read_to_string(path).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// HTML signals (parsed DOM, not regex)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HtmlSignals {
    has_module: bool,
    remote_resources: Vec<String>,
    ng_version: Option<String>,
    /// Concatenated bodies of inline `<script>` elements (for ES detection when
    /// the app inlines its JS instead of shipping separate .js files).
    inline_js: String,
}

/// Parse `index.html` and extract the `<script type="module">` flag, the
/// Angular `ng-version` attribute, remote `src`/`href` references, and the
/// bodies of inline `<script>` elements.
fn detect_html_signals(html: &str) -> HtmlSignals {
    let mut sig = HtmlSignals::default();
    let Ok(dom) = tl::parse(html, tl::ParserOptions::default()) else {
        return sig;
    };
    let parser = dom.parser();
    let mut seen: HashSet<String> = HashSet::new();
    let mut inline: Vec<String> = Vec::new();
    for node in dom.nodes() {
        let Some(tag) = node.as_tag() else { continue };
        let attrs = tag.attributes();
        let attr = |k: &str| {
            attrs
                .get(k)
                .flatten()
                .map(|b| b.as_utf8_str().into_owned())
        };

        if tag.name().as_utf8_str() == "script" {
            let ty = attr("type");
            if ty.as_deref() == Some("module") {
                sig.has_module = true;
            }
            // Inline JS (no src, JS-ish type) → collect for tokenizing.
            if attr("src").is_none() && is_js_script_type(ty.as_deref()) {
                let body = tag.inner_text(parser);
                if !body.trim().is_empty() {
                    inline.push(body.into_owned());
                }
            }
        }
        if sig.ng_version.is_none() {
            if let Some(v) = attr("ng-version") {
                sig.ng_version = Some(v);
            }
        }
        for key in ["src", "href"] {
            if let Some(url) = attr(key) {
                if is_remote(&url) && seen.insert(url.clone()) && sig.remote_resources.len() < MAX_REMOTE {
                    sig.remote_resources.push(url);
                }
            }
        }
    }
    // `;` between scripts so tokens from adjacent bodies can't merge.
    sig.inline_js = inline.join("\n;\n");
    sig
}

/// Whether a `<script type>` names executable JavaScript (so its inline body is
/// worth tokenizing). `application/json`, `importmap`, etc. are not.
fn is_js_script_type(ty: Option<&str>) -> bool {
    match ty {
        None => true,
        Some(t) => matches!(
            t.trim().to_ascii_lowercase().as_str(),
            "" | "text/javascript"
                | "application/javascript"
                | "module"
                | "text/ecmascript"
                | "application/ecmascript"
        ),
    }
}

/// A `src`/`href` that leaves the device: absolute `http(s)://` or the
/// protocol-relative `//host/...` form.
fn is_remote(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("//")
}

// ---------------------------------------------------------------------------
// Framework identity (license banners / presence markers in JS text)
// ---------------------------------------------------------------------------

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

re!(RE_ANGULARJS, r#"(?:angular\.version|"full"\s*:\s*")\s*[:=]?\s*"?(1\.[0-9]+\.[0-9]+)"#);
re!(RE_REACT_BANNER, r"@license React v?([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_REACT_PRESENT, r"React\.createElement|react-dom(?:\.production)?(?:\.min)?\.js|__reactContainer|data-reactroot");
re!(RE_VUE_V2, r"Vue\.js v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_VUE_V3, r"@vue/(?:runtime|shared|reactivity)[^\n]{0,40}?v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_VUE_PRESENT, r"__vue__|createElementVNode|Vue\.createApp|\bVue\b");
re!(RE_JQUERY, r"jQuery(?: JavaScript Library)? v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_JQUERY_PRESENT, r"jquery|jQuery");
re!(RE_ENACT, r"@enact/|enactVersion|enact_dev|enactMeta");
// Enyo (the legacy LG/webOS framework Enact descends from): the `enyo.*` API
// and its bundle/file markers.
re!(
    RE_ENYO,
    r"\benyo\.(?:kind|Control|Component|Application|version|ready|singleton)\b|@enyo/|\benyojs\b|enyo(?:\.min)?\.js"
);
re!(RE_ENYO_VER, r#"enyo\.version\s*=\s*\{[^}]*?core["']?\s*:\s*["']([0-9]+\.[0-9]+\.[0-9]+)"#);
re!(RE_WEBOSTV, r"(?i)webOSTV(?:-dev)?\.js");
re!(RE_WEBOSTV_VER, r"webOSTV(?:-dev)?\.js\s*(?:v(?:ersion)?)?\s*([0-9]+\.[0-9]+\.[0-9]+)");

/// Returns (primary framework, other frameworks present, webOSTV.js presence).
///
/// `webostvjs`: `None` = absent, `Some(None)` = present (version unknown),
/// `Some(Some(v))` = present with a detected version.
fn detect_frameworks(
    html: &str,
    js: &[(String, String)],
    ng_version: Option<&str>,
) -> (Option<FrameworkInfo>, Vec<FrameworkInfo>, Option<Option<Version>>) {
    let mut found: Vec<FrameworkInfo> = Vec::new();

    let cap_ver = |re: &Regex, hay: &str| -> Option<Version> {
        re.captures(hay)
            .and_then(|c| c.get(1))
            .and_then(|m| Version::parse(m.as_str()).ok())
    };

    // Angular (2+): from the parsed `ng-version` attribute — exact and reliable.
    if let Some(v) = ng_version {
        found.push(FrameworkInfo::new(
            FrameworkKind::Angular,
            Version::parse(v).ok(),
        ));
    }

    // Everything else: scan HTML + JS contents for a banner/presence marker.
    let scan = |kind: FrameworkKind,
                present: &Regex,
                version: Option<&Regex>|
     -> Option<FrameworkInfo> {
        let mut version_found: Option<Version> = None;
        let mut present_found = false;
        for hay in std::iter::once(html).chain(js.iter().map(|(_, c)| c.as_str())) {
            if let Some(re) = version {
                if version_found.is_none() {
                    version_found = cap_ver(re, hay);
                }
            }
            if !present_found && present.is_match(hay) {
                present_found = true;
            }
            if present_found && (version.is_none() || version_found.is_some()) {
                break;
            }
        }
        if present_found || version_found.is_some() {
            Some(FrameworkInfo::new(kind, version_found))
        } else {
            None
        }
    };

    if let Some(f) = scan(FrameworkKind::React, &RE_REACT_PRESENT, Some(&RE_REACT_BANNER)) {
        found.push(f);
    }
    if let Some(mut vue) = scan(FrameworkKind::Vue, &RE_VUE_PRESENT, Some(&RE_VUE_V2)) {
        if vue.version.is_none() {
            vue.version = js.iter().find_map(|(_, c)| cap_ver(&RE_VUE_V3, c));
        }
        found.push(vue);
    }
    if let Some(f) = scan(FrameworkKind::AngularJs, &RE_ANGULARJS, Some(&RE_ANGULARJS)) {
        found.push(f);
    }
    if let Some(f) = scan(FrameworkKind::Jquery, &RE_JQUERY_PRESENT, Some(&RE_JQUERY)) {
        found.push(f);
    }

    let enact = std::iter::once(html)
        .chain(js.iter().map(|(_, c)| c.as_str()))
        .any(|h| RE_ENACT.is_match(h));
    if enact {
        found.push(FrameworkInfo::new(FrameworkKind::Enact, None));
    }

    // Enyo (best-effort version from `enyo.version = { core: "x.y.z" }`).
    if let Some(f) = scan(FrameworkKind::Enyo, &RE_ENYO, Some(&RE_ENYO_VER)) {
        found.push(f);
    }

    let webostvjs = {
        let by_name = js.iter().any(|(n, _)| RE_WEBOSTV.is_match(n));
        let by_content = std::iter::once(html)
            .chain(js.iter().map(|(_, c)| c.as_str()))
            .any(|h| RE_WEBOSTV.is_match(h));
        if by_name || by_content {
            let ver = std::iter::once(html)
                .chain(js.iter().map(|(_, c)| c.as_str()))
                .find_map(|h| cap_ver(&RE_WEBOSTV_VER, h));
            Some(ver)
        } else {
            None
        }
    };

    let precedence = [
        FrameworkKind::Enact,
        FrameworkKind::Enyo,
        FrameworkKind::Angular,
        FrameworkKind::React,
        FrameworkKind::Vue,
        FrameworkKind::AngularJs,
        FrameworkKind::Jquery,
    ];
    let mut primary: Option<FrameworkInfo> = None;
    for kind in precedence {
        if let Some(pos) = found.iter().position(|f| f.kind == kind) {
            primary = Some(found.remove(pos));
            break;
        }
    }

    let primary = Some(primary.unwrap_or_else(|| FrameworkInfo::new(FrameworkKind::PlainHtml, None)));
    (primary, found, webostvjs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(content: &str) -> Vec<(String, String)> {
        vec![("bundle.js".to_string(), content.to_string())]
    }

    // --- framework identity ---

    #[test]
    fn detects_angular_version_from_attribute() {
        let sig = detect_html_signals(r#"<html><app-root ng-version="15.2.9"></app-root></html>"#);
        let (primary, _o, _t) = detect_frameworks("", &[], sig.ng_version.as_deref());
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::Angular);
        assert_eq!(f.version, Some(Version::parse("15.2.9").unwrap()));
    }

    #[test]
    fn detects_react_from_license_banner() {
        let (primary, _o, _t) = detect_frameworks(
            "",
            &js("/** @license React v18.2.0 */ React.createElement('div')"),
            None,
        );
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::React);
        assert_eq!(f.version, Some(Version::parse("18.2.0").unwrap()));
    }

    #[test]
    fn enact_takes_precedence_over_react() {
        let (primary, others, _t) = detect_frameworks(
            "",
            &js("import '@enact/core'; /** @license React v18.2.0 */ React.createElement"),
            None,
        );
        assert_eq!(primary.unwrap().kind, FrameworkKind::Enact);
        assert!(others.iter().any(|f| f.kind == FrameworkKind::React));
    }

    #[test]
    fn plain_html_when_nothing_matches() {
        let (primary, others, tv) = detect_frameworks("<html><body>hi</body></html>", &[], None);
        assert_eq!(primary.unwrap().kind, FrameworkKind::PlainHtml);
        assert!(others.is_empty());
        assert!(tv.is_none());
    }

    #[test]
    fn detects_enyo_from_api_usage() {
        let (primary, _o, _t) =
            detect_frameworks("", &js("enyo.kind({ name: 'App', kind: enyo.Control });"), None);
        assert_eq!(primary.unwrap().kind, FrameworkKind::Enyo);
    }

    #[test]
    fn detects_enyo_version() {
        let (primary, _o, _t) = detect_frameworks(
            "",
            &js(r#"enyo.version = { core: "2.7.0", canvas: "2.7.0" };"#),
            None,
        );
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::Enyo);
        assert_eq!(f.version, Some(Version::parse("2.7.0").unwrap()));
    }

    #[test]
    fn enact_takes_precedence_over_enyo_markers() {
        // Enact's lineage means some enyo-ish strings can co-occur; Enact wins.
        let (primary, others, _t) =
            detect_frameworks("", &js("import '@enact/core'; enyo.kind({});"), None);
        assert_eq!(primary.unwrap().kind, FrameworkKind::Enact);
        assert!(others.iter().any(|f| f.kind == FrameworkKind::Enyo));
    }

    // --- HTML signals (tl DOM) ---

    #[test]
    fn detects_script_module_tag() {
        assert!(detect_html_signals(r#"<script type="module" src="app.js"></script>"#).has_module);
        assert!(detect_html_signals(r#"<script src="a.js" type='module'></script>"#).has_module);
        assert!(!detect_html_signals(r#"<script src="app.js"></script>"#).has_module);
    }

    #[test]
    fn module_word_in_text_is_not_a_module_script() {
        // A DOM parse won't be fooled by the word "module" in body text/attrs.
        let sig = detect_html_signals(r#"<html><body>type="module" is great</body></html>"#);
        assert!(!sig.has_module);
    }

    #[test]
    fn collects_remote_resources_deduped() {
        let sig = detect_html_signals(
            r#"<html><head>
                <script src="https://cdn.example.com/lib.js"></script>
                <link href="//fonts.example.com/f.css">
                <script src="bundle.js"></script>
                <img src="https://cdn.example.com/lib.js">
            </head></html>"#,
        );
        assert_eq!(sig.remote_resources.len(), 2);
        assert!(sig.remote_resources.iter().any(|u| u.contains("cdn.example.com/lib.js")));
        assert!(sig.remote_resources.iter().any(|u| u.starts_with("//fonts.example.com")));
    }

    #[test]
    fn no_remote_resources_when_all_local() {
        let sig = detect_html_signals(r#"<script src="bundle.js"></script><link href="style.css">"#);
        assert!(sig.remote_resources.is_empty());
    }

    #[test]
    fn collects_inline_script_body() {
        // Apps that inline all JS into index.html (Enyo/Enact single-file builds).
        let sig = detect_html_signals(
            r#"<html><head><script>const x = async () => await 1;</script>
               <script type="application/json">{"not":"js"}</script></head></html>"#,
        );
        assert!(sig.inline_js.contains("async"));
        assert!(!sig.inline_js.contains("not")); // JSON script body excluded
    }
}
