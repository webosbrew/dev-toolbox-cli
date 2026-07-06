//! Frontend framework and JavaScript syntax-level detection for a web app.
//!
//! Heuristics operate on the shipped `index.html` and the bundle `*.js` files.
//! They are intentionally conservative: regex on JS cannot tell code from
//! strings, so the ES-level result is treated as a floor ("the engine must
//! support at least this"), and framework versions are only reported when a
//! reliable signal (license banner, `ng-version` attribute) is present.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
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
    let html = fs::read_to_string(index_html).unwrap_or_default();

    let mut js: Vec<(String, String)> = Vec::new();
    collect_js(dir, 0, &mut js);

    let (framework, also_present, webostvjs) = detect_frameworks(&html, &js);
    // `<script type="module">` is an HTML-level engine signal the JS scan misses.
    let has_module = RE_SCRIPT_MODULE.is_match(&html);
    let (es_level, es_features) = detect_es(&js, has_module);
    let remote_resources = detect_remote_resources(&html);

    WebAppDetection {
        framework,
        also_present,
        webostvjs,
        es_level,
        es_features,
        remote_resources,
    }
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
        if !name.ends_with(".js") || name.ends_with(".min.js.map") || name.ends_with(".map") {
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

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}

// Framework signatures.
re!(RE_NG_VERSION, r#"ng-version="([0-9]+\.[0-9]+\.[0-9]+)""#);
re!(RE_ANGULARJS, r#"(?:angular\.version|"full"\s*:\s*")\s*[:=]?\s*"?(1\.[0-9]+\.[0-9]+)"#);
re!(RE_REACT_BANNER, r"@license React v?([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_REACT_PRESENT, r"React\.createElement|react-dom(?:\.production)?(?:\.min)?\.js|__reactContainer|data-reactroot");
re!(RE_VUE_V2, r"Vue\.js v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_VUE_V3, r"@vue/(?:runtime|shared|reactivity)[^\n]{0,40}?v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_VUE_PRESENT, r"__vue__|createElementVNode|Vue\.createApp|\bVue\b");
re!(RE_JQUERY, r"jQuery(?: JavaScript Library)? v([0-9]+\.[0-9]+\.[0-9]+)");
re!(RE_JQUERY_PRESENT, r"jquery|jQuery");
re!(RE_ENACT, r"@enact/|enactVersion|enact_dev|enactMeta");
re!(RE_WEBOSTV, r"(?i)webOSTV(?:-dev)?\.js");
re!(RE_WEBOSTV_VER, r"webOSTV(?:-dev)?\.js\s*(?:v(?:ersion)?)?\s*([0-9]+\.[0-9]+\.[0-9]+)");

// HTML-level signals.
re!(RE_SCRIPT_MODULE, r#"(?i)<script[^>]*\btype\s*=\s*["']module["']"#);
re!(
    RE_REMOTE_RESOURCE,
    r#"(?i)(?:src|href)\s*=\s*["']((?:https?:)?//[^"']+)["']"#
);

/// Returns (primary framework, other frameworks present, webOSTV.js presence).
///
/// `webostvjs`: `None` = absent, `Some(None)` = present (version unknown),
/// `Some(Some(v))` = present with a detected version.
fn detect_frameworks(
    html: &str,
    js: &[(String, String)],
) -> (Option<FrameworkInfo>, Vec<FrameworkInfo>, Option<Option<Version>>) {
    // Search the HTML plus every JS file/name. `haystacks` is scanned for
    // content signatures; `names` for filename signatures.
    let mut found: Vec<FrameworkInfo> = Vec::new();

    let cap_ver = |re: &Regex, hay: &str| -> Option<Version> {
        re.captures(hay)
            .and_then(|c| c.get(1))
            .and_then(|m| Version::parse(m.as_str()).ok())
    };

    // Angular (2+): the ng-version attribute lives in index.html and is exact.
    if let Some(v) = cap_ver(&RE_NG_VERSION, html) {
        found.push(FrameworkInfo::new(FrameworkKind::Angular, Some(v)));
    }

    // Everything else: scan HTML + JS contents. `scan` returns the detection
    // instead of capturing `found`, so its borrow ends at each call.
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
        // Vue 3's banner is a separate pattern; fold its version in if v2 missed it.
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

    // Enact (LG's React-based framework). Version is not reliably in the bundle.
    let enact = std::iter::once(html)
        .chain(js.iter().map(|(_, c)| c.as_str()))
        .any(|h| RE_ENACT.is_match(h));
    if enact {
        found.push(FrameworkInfo::new(FrameworkKind::Enact, None));
    }

    // webOSTV.js SDK — an app can use it alongside any framework.
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

    // Pick the primary by precedence; the rest become `also_present`.
    let precedence = [
        FrameworkKind::Enact,
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

// ES feature signatures. Deliberately narrow to limit false positives on
// strings/comments in minified code.
re!(RE_LET_CONST, r"\b(?:let|const)\s");
re!(RE_ARROW, r"=>");
re!(RE_TEMPLATE, r"`");
re!(RE_CLASS, r"\bclass\s+[A-Za-z_$]");
re!(RE_SPREAD, r"\.\.\.");
// Require operand context (`a ** b`) so JSDoc `/**` banners don't false-match.
re!(RE_EXPONENT, r"[A-Za-z0-9_)\]]\s*\*\*\s*[A-Za-z0-9_($]");
re!(RE_ASYNC, r"\b(?:async|await)\b");
re!(RE_OPTIONAL_CHAIN, r"\?\.");
re!(RE_NULLISH, r"\?\?");

/// Scan the bundle for JS syntax features (plus the HTML `type="module"` flag)
/// and derive the minimum ES level.
fn detect_es(js: &[(String, String)], html_module: bool) -> (Option<EsLevel>, Vec<EsFeature>) {
    let checks: [(&LazyLock<Regex>, EsFeature); 9] = [
        (&RE_LET_CONST, EsFeature::LetConst),
        (&RE_ARROW, EsFeature::Arrow),
        (&RE_TEMPLATE, EsFeature::TemplateLiteral),
        (&RE_CLASS, EsFeature::Class),
        (&RE_SPREAD, EsFeature::Spread),
        (&RE_EXPONENT, EsFeature::Exponent),
        (&RE_ASYNC, EsFeature::AsyncAwait),
        (&RE_OPTIONAL_CHAIN, EsFeature::OptionalChaining),
        (&RE_NULLISH, EsFeature::NullishCoalescing),
    ];
    let mut features: Vec<EsFeature> = Vec::new();
    for (re, feature) in checks {
        if js.iter().any(|(_, c)| re.is_match(c)) {
            features.push(feature);
        }
    }
    if html_module {
        features.push(EsFeature::EsModule);
    }
    // Nothing to go on: no JS files and no module tag → level unknown.
    if js.is_empty() && !html_module {
        return (None, features);
    }
    let level = features
        .iter()
        .map(|f| f.level())
        .max()
        .unwrap_or(EsLevel::Es5);
    (Some(level), features)
}

/// Collect distinct remote resource URLs (`http(s)://` or protocol-relative
/// `//host/...`) referenced by `src`/`href` in the HTML. Informational only.
fn detect_remote_resources(html: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for cap in RE_REMOTE_RESOURCE.captures_iter(html) {
        let url = cap.get(1).unwrap().as_str().to_string();
        if seen.insert(url.clone()) {
            out.push(url);
            if out.len() >= MAX_REMOTE {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(content: &str) -> Vec<(String, String)> {
        vec![("bundle.js".to_string(), content.to_string())]
    }

    #[test]
    fn detects_angular_version_from_html() {
        let (primary, _others, _tv) =
            detect_frameworks(r#"<html><app-root ng-version="15.2.9"></app-root></html>"#, &[]);
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::Angular);
        assert_eq!(f.version, Some(Version::parse("15.2.9").unwrap()));
    }

    #[test]
    fn detects_react_from_license_banner() {
        let (primary, _o, _t) = detect_frameworks(
            "",
            &js("/** @license React v18.2.0 */ React.createElement('div')"),
        );
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::React);
        assert_eq!(f.version, Some(Version::parse("18.2.0").unwrap()));
    }

    #[test]
    fn detects_jquery_version() {
        let (primary, _o, _t) =
            detect_frameworks("", &js("/*! jQuery JavaScript Library v3.6.0 */ jQuery(function(){})"));
        let f = primary.unwrap();
        assert_eq!(f.kind, FrameworkKind::Jquery);
        assert_eq!(f.version, Some(Version::parse("3.6.0").unwrap()));
    }

    #[test]
    fn enact_takes_precedence_over_react() {
        // Enact bundles are React under the hood; the primary should be Enact.
        let (primary, others, _t) = detect_frameworks(
            "",
            &js("import '@enact/core'; /** @license React v18.2.0 */ React.createElement"),
        );
        assert_eq!(primary.unwrap().kind, FrameworkKind::Enact);
        assert!(others.iter().any(|f| f.kind == FrameworkKind::React));
    }

    #[test]
    fn detects_webostvjs() {
        let files = vec![("webOSTV.js".to_string(), "var webOS = {};".to_string())];
        let (_p, _o, tv) = detect_frameworks("", &files);
        assert!(tv.is_some());
    }

    #[test]
    fn plain_html_when_nothing_matches() {
        let (primary, others, tv) = detect_frameworks("<html><body>hi</body></html>", &[]);
        assert_eq!(primary.unwrap().kind, FrameworkKind::PlainHtml);
        assert!(others.is_empty());
        assert!(tv.is_none());
    }

    #[test]
    fn es_level_is_max_feature() {
        let (level, feats) = detect_es(&js("const x = a?.b; let y = async () => await z;"), false);
        assert_eq!(level, Some(EsLevel::Es2020)); // optional chaining
        assert!(feats.contains(&EsFeature::OptionalChaining));
        assert!(feats.contains(&EsFeature::AsyncAwait));
        assert!(feats.contains(&EsFeature::Arrow));
    }

    #[test]
    fn es5_bundle_reads_as_es5() {
        let (level, feats) = detect_es(&js("var x = 1; function f() { return x; }"), false);
        assert_eq!(level, Some(EsLevel::Es5));
        assert!(feats.is_empty());
    }

    #[test]
    fn jsdoc_banner_is_not_an_exponent() {
        // A `/** ... */` license banner must not be read as the `**` operator.
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
    fn script_module_raises_es_level_over_es5_bundle() {
        // An otherwise-ES5 bundle loaded as a module still needs a modern engine.
        let (level, feats) = detect_es(&js("var x = 1;"), true);
        assert_eq!(level, Some(EsLevel::Es2018));
        assert!(feats.contains(&EsFeature::EsModule));
    }

    #[test]
    fn detects_script_module_tag() {
        assert!(RE_SCRIPT_MODULE.is_match(r#"<script type="module" src="app.js"></script>"#));
        assert!(RE_SCRIPT_MODULE.is_match(r#"<script src="a.js" type='module'></script>"#));
        assert!(!RE_SCRIPT_MODULE.is_match(r#"<script src="app.js"></script>"#));
    }

    #[test]
    fn collects_remote_resources_deduped() {
        let html = r#"<html><head>
            <script src="https://cdn.example.com/lib.js"></script>
            <link href="//fonts.example.com/f.css">
            <script src="bundle.js"></script>
            <img src="https://cdn.example.com/lib.js">
        </head></html>"#;
        let remotes = detect_remote_resources(html);
        assert_eq!(remotes.len(), 2); // local bundle.js excluded, duplicate deduped
        assert!(remotes.iter().any(|u| u.contains("cdn.example.com/lib.js")));
        assert!(remotes.iter().any(|u| u.starts_with("//fonts.example.com")));
    }

    #[test]
    fn no_remote_resources_when_all_local() {
        let remotes = detect_remote_resources(
            r#"<script src="bundle.js"></script><link href="style.css">"#,
        );
        assert!(remotes.is_empty());
    }
}
