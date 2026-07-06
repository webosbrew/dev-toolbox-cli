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
use ress::prelude::*;
use semver::Version;

use crate::eslevel::{EsFeature, EsLevel};
use crate::{FrameworkInfo, FrameworkKind, WebAppDetection};

/// Skip individual files larger than this (minified vendor blobs aside, real
/// app code is far smaller); keeps a pathological package from blowing memory.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Stop after scanning this many JS files.
const MAX_JS_FILES: usize = 400;
/// Recursion depth cap for the directory walk.
const MAX_DEPTH: usize = 12;
/// Cap on distinct remote resource URLs reported.
const MAX_REMOTE: usize = 20;

/// Detect the framework, webOSTV.js SDK, ES syntax level and remote resources
/// of the web app rooted at `dir`, whose HTML entry point is `index_html`.
pub fn detect_web_app(dir: &Path, index_html: &Path) -> WebAppDetection {
    let html = read_capped(index_html);

    let mut js: Vec<(String, String)> = Vec::new();
    collect_js(dir, 0, &mut js);

    let signals = detect_html_signals(&html);
    let (framework, also_present, webostvjs) =
        detect_frameworks(&html, &js, signals.ng_version.as_deref());
    let (es_level, es_features) = detect_es(&js, signals.has_module);

    WebAppDetection {
        framework,
        also_present,
        webostvjs,
        es_level,
        es_features,
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

/// Recursively gather `*.js` file contents (skipping source maps), bounded by
/// the depth/count/size caps above.
fn collect_js(dir: &Path, depth: usize, out: &mut Vec<(String, String)>) {
    if depth > MAX_DEPTH || out.len() >= MAX_JS_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_JS_FILES {
            return;
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            collect_js(&path, depth + 1, out);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".js") || name.ends_with(".map") {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            continue;
        }
        if let Ok(content) = fs::read(&path) {
            out.push((name, String::from_utf8_lossy(&content).into_owned()));
        }
    }
}

// ---------------------------------------------------------------------------
// HTML signals (parsed DOM, not regex)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct HtmlSignals {
    has_module: bool,
    remote_resources: Vec<String>,
    ng_version: Option<String>,
}

/// Parse `index.html` and extract the `<script type="module">` flag, the
/// Angular `ng-version` attribute, and remote `src`/`href` references.
fn detect_html_signals(html: &str) -> HtmlSignals {
    let mut sig = HtmlSignals::default();
    let Ok(dom) = tl::parse(html, tl::ParserOptions::default()) else {
        return sig;
    };
    let mut seen: HashSet<String> = HashSet::new();
    for node in dom.nodes() {
        let Some(tag) = node.as_tag() else { continue };
        let attrs = tag.attributes();
        let attr = |k: &str| {
            attrs
                .get(k)
                .flatten()
                .map(|b| b.as_utf8_str().into_owned())
        };

        if tag.name().as_utf8_str() == "script"
            && attr("type").as_deref() == Some("module")
        {
            sig.has_module = true;
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
    sig
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

// ---------------------------------------------------------------------------
// ES feature detection (token stream, not regex)
// ---------------------------------------------------------------------------

/// Scan the bundle for JS syntax features (plus the HTML `type="module"` flag)
/// and derive the minimum ES level. Features are read from a `ress` token
/// stream, so occurrences inside strings/comments/regex literals are ignored.
fn detect_es(js: &[(String, String)], html_module: bool) -> (Option<EsLevel>, Vec<EsFeature>) {
    let mut found: HashSet<EsFeature> = HashSet::new();
    for (_, content) in js {
        scan_js_features(content, &mut found);
    }
    if html_module {
        found.insert(EsFeature::EsModule);
    }
    // Nothing to go on: no JS files and no module tag → level unknown.
    if js.is_empty() && !html_module {
        return (None, Vec::new());
    }
    // Emit in a stable, oldest-first order for readable output.
    let order = [
        EsFeature::LetConst,
        EsFeature::Arrow,
        EsFeature::TemplateLiteral,
        EsFeature::Class,
        EsFeature::Spread,
        EsFeature::Exponent,
        EsFeature::AsyncAwait,
        EsFeature::EsModule,
        EsFeature::OptionalChaining,
        EsFeature::NullishCoalescing,
    ];
    let features: Vec<EsFeature> = order.into_iter().filter(|f| found.contains(f)).collect();
    let level = features
        .iter()
        .map(|f| f.level())
        .max()
        .unwrap_or(EsLevel::Es5);
    (Some(level), features)
}

/// Tokenize one JS source and record the ES features it uses.
///
/// `?.` and `??` are not single tokens in this lexer, so they are reconstructed
/// from an adjacent `?` + `.`/`?` pair (adjacency rules out `a ? .5 : b`).
/// `async` is a contextual keyword (an identifier here), so it only counts when
/// immediately followed by `function`, `(`, or an identifier.
fn scan_js_features(content: &str, found: &mut HashSet<EsFeature>) {
    let mut pending_question_end: Option<usize> = None;
    let mut pending_async = false;

    for item in Scanner::new(content) {
        let Ok(item) = item else {
            break; // lex error → keep what we have (conservative)
        };
        let span = item.span;

        if pending_async {
            pending_async = false;
            let is_async_fn = matches!(&item.token, Token::Keyword(Keyword::Function(_)))
                || matches!(&item.token, Token::Ident(_))
                || matches!(&item.token, Token::Punct(Punct::OpenParen));
            if is_async_fn {
                found.insert(EsFeature::AsyncAwait);
            }
        }

        if let Some(q_end) = pending_question_end.take() {
            if span.start == q_end {
                match &item.token {
                    Token::Punct(Punct::Period) => {
                        found.insert(EsFeature::OptionalChaining);
                    }
                    Token::Punct(Punct::QuestionMark) => {
                        found.insert(EsFeature::NullishCoalescing);
                    }
                    _ => {}
                }
            }
        }

        match &item.token {
            Token::Keyword(Keyword::Let(_)) | Token::Keyword(Keyword::Const(_)) => {
                found.insert(EsFeature::LetConst);
            }
            Token::Keyword(Keyword::Class(_)) => {
                found.insert(EsFeature::Class);
            }
            Token::Keyword(Keyword::Await(_)) => {
                found.insert(EsFeature::AsyncAwait);
            }
            Token::Template(_) => {
                found.insert(EsFeature::TemplateLiteral);
            }
            // `async` is an identifier in this lexer; confirm it's a function.
            Token::Ident(id) if id.as_ref() == "async" => {
                pending_async = true;
            }
            Token::Punct(p) => match p {
                Punct::EqualGreaterThan => {
                    found.insert(EsFeature::Arrow);
                }
                Punct::Ellipsis => {
                    found.insert(EsFeature::Spread);
                }
                Punct::DoubleAsterisk | Punct::DoubleAsteriskEqual => {
                    found.insert(EsFeature::Exponent);
                }
                Punct::QuestionMark => {
                    pending_question_end = Some(span.end);
                }
                _ => {}
            },
            _ => {}
        }
    }
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

    // --- ES features (ress token stream) ---

    #[test]
    fn es_level_is_max_feature() {
        let (level, feats) = detect_es(&js("const x = a?.b; let y = async () => await z;"), false);
        assert_eq!(level, Some(EsLevel::Es2020)); // optional chaining
        assert!(feats.contains(&EsFeature::OptionalChaining));
        assert!(feats.contains(&EsFeature::AsyncAwait));
        assert!(feats.contains(&EsFeature::Arrow));
        assert!(feats.contains(&EsFeature::LetConst));
    }

    #[test]
    fn detects_nullish_coalescing() {
        let (level, feats) = detect_es(&js("var x = a ?? b;"), false);
        assert_eq!(level, Some(EsLevel::Es2020));
        assert!(feats.contains(&EsFeature::NullishCoalescing));
    }

    #[test]
    fn es5_bundle_reads_as_es5() {
        let (level, feats) = detect_es(&js("var x = 1; function f() { return x; }"), false);
        assert_eq!(level, Some(EsLevel::Es5));
        assert!(feats.is_empty());
    }

    #[test]
    fn features_inside_strings_and_comments_are_ignored() {
        // The whole point of tokenizing: none of these are real syntax.
        let src = r#"
            // const x = () => {}; a ** b; a?.b; a ?? b; async function q(){}
            var s = "const y = async () => await z ** 2 ?? w ?.p";
            var t = `template-looking ${'but a string'}`;
            var u = 1;
        "#;
        let (level, feats) = detect_es(&js(src), false);
        // The backtick template IS real code here, so ES2015; but NO async,
        // arrow, exponent, optional-chaining or nullish from the string/comment.
        assert!(!feats.contains(&EsFeature::AsyncAwait));
        assert!(!feats.contains(&EsFeature::Arrow));
        assert!(!feats.contains(&EsFeature::Exponent));
        assert!(!feats.contains(&EsFeature::OptionalChaining));
        assert!(!feats.contains(&EsFeature::NullishCoalescing));
        assert_eq!(level, Some(EsLevel::Es2015)); // only the real template literal
        assert!(feats.contains(&EsFeature::TemplateLiteral));
    }

    #[test]
    fn jsdoc_banner_is_not_an_exponent() {
        let (level, feats) = detect_es(&js("/** @license v1 */ var x = 1;"), false);
        assert_eq!(level, Some(EsLevel::Es5));
        assert!(!feats.contains(&EsFeature::Exponent));
    }

    #[test]
    fn real_exponent_is_detected() {
        let (_level, feats) = detect_es(&js("var y = a ** 2;"), false);
        assert!(feats.contains(&EsFeature::Exponent));
    }

    #[test]
    fn async_identifier_variable_is_not_async_function() {
        // `var async = 1;` must not be read as async/await usage.
        let (_level, feats) = detect_es(&js("var async = 1; var b = async + 2;"), false);
        assert!(!feats.contains(&EsFeature::AsyncAwait));
    }

    #[test]
    fn script_module_raises_es_level_over_es5_bundle() {
        let (level, feats) = detect_es(&js("var x = 1;"), true);
        assert_eq!(level, Some(EsLevel::Es2018));
        assert!(feats.contains(&EsFeature::EsModule));
    }
}
