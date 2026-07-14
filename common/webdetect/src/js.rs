//! Shared JavaScript analysis: gather `.js` sources and, from a `ress` token
//! stream, derive the ES syntax level and any notable runtime-API usage.
//!
//! Used by both web-app detection (checked against the firmware's web engine)
//! and service detection (checked against the firmware's Node.js). Tokenizing
//! means keywords/operators/identifiers inside strings, comments and regex
//! literals are ignored.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

use ress::prelude::*;

use crate::eslevel::{EsFeature, EsLevel};
use crate::ApiUse;

/// Skip individual files larger than this.
pub(crate) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Stop after scanning this many JS files.
const MAX_JS_FILES: usize = 400;
/// Recursion depth cap for the directory walk.
const MAX_DEPTH: usize = 12;

/// The result of analyzing a set of JS sources.
pub(crate) struct JsAnalysis {
    pub es_level: Option<EsLevel>,
    pub es_features: Vec<EsFeature>,
    pub es_apis: Vec<ApiUse>,
}

/// Recursively gather `*.js` file contents (skipping source maps), bounded by
/// the depth/count/size caps above.
pub(crate) fn collect_js(dir: &Path, depth: usize, out: &mut Vec<(String, String)>) {
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

/// Module-resolution extensions tried for an extensionless specifier and for a
/// directory's `index` file, in Node's precedence order.
const RESOLVE_EXTS: [&str; 3] = ["js", "cjs", "mjs"];

/// Gather the JS a Node service's entry point actually pulls in: starting from
/// `main` (default `index.js`), follow the **static** module graph via relative
/// `require`/`import`/`export … from`/`import(...)` specifiers, staying inside
/// `dir`.
///
/// Unlike [`collect_js`] (used for a web app's flat bundle), this does *not*
/// slurp every `.js` under the tree. A service commonly ships vendored code —
/// `node_modules`, or a server it spawns on its **own** bundled Node — whose ES
/// level says nothing about what the firmware's Node runs. Only first-party code
/// reachable from `main` runs on the firmware Node, so only that is graded.
/// Bare specifiers (npm packages, built-ins) are deliberately not followed.
pub(crate) fn collect_service_js(dir: &Path, main: Option<&str>, out: &mut Vec<(String, String)>) {
    let Some(entry) = resolve_module(dir, dir, main.unwrap_or("index.js")) else {
        return;
    };
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(entry);
    while let Some(path) = queue.pop_front() {
        if out.len() >= MAX_JS_FILES {
            return;
        }
        if !visited.insert(path.clone()) {
            continue;
        }
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let content = String::from_utf8_lossy(&bytes).into_owned();
        let base = path.parent().unwrap_or(dir);
        for spec in extract_relative_specifiers(&content) {
            if let Some(dep) = resolve_module(dir, base, &spec) {
                if !visited.contains(&dep) {
                    queue.push_back(dep);
                }
            }
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        out.push((name, content));
    }
}

/// Resolve a relative module specifier `spec` (or an entry `main`) against
/// `base`, Node-style: try the path as a JS file, then with a `.js/.cjs/.mjs`
/// extension, then as a directory's `index` file. Returns the resolved path only
/// when it exists and stays lexically within `root`.
fn resolve_module(root: &Path, base: &Path, spec: &str) -> Option<PathBuf> {
    let candidate = base.join(spec);
    // 1. Exact path to an existing JS-family file (an explicit extension).
    if candidate.is_file() {
        return has_js_ext(&candidate).then(|| contain(root, &candidate)).flatten();
    }
    // 2. Bare specifier + extension (`./foo` → `./foo.js`). Appends rather than
    //    replacing, so a dotted basename (`./foo.bar`) isn't mangled.
    for ext in RESOLVE_EXTS {
        let p = PathBuf::from(format!("{}.{ext}", candidate.to_string_lossy()));
        if p.is_file() {
            return contain(root, &p);
        }
    }
    // 3. Directory index (`./dir` → `./dir/index.js`).
    if candidate.is_dir() {
        for ext in RESOLVE_EXTS {
            let p = candidate.join(format!("index.{ext}"));
            if p.is_file() {
                return contain(root, &p);
            }
        }
    }
    None
}

fn has_js_ext(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js") | Some("cjs") | Some("mjs")
    )
}

/// Keep `path` only if it stays within `root` after lexically resolving `.`/`..`
/// (an extracted package has no on-disk symlinks, so this matches the real
/// tree). Guards against a specifier like `../../etc/passwd`.
fn contain(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = lexical_normalize(root);
    let path = lexical_normalize(path);
    path.starts_with(&root).then_some(path)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A token, reduced to just what the module-specifier lookback needs.
enum SpecTok {
    Import,
    OpenParen,
    Ident(String),
    Str(String),
    Other,
}

/// Extract the **relative** module specifiers (`./`, `../`) a source statically
/// references via `require('x')`, `import(...)`, `import 'x'`, and
/// `import`/`export … from 'x'`. Tokenizing means specifiers written as string
/// literals in comments or unrelated strings are not matched.
fn extract_relative_specifiers(content: &str) -> Vec<String> {
    let mut specs = Vec::new();
    // The last two meaningful (non-comment) tokens, for lookback.
    let mut prev: Option<SpecTok> = None;
    let mut prev2: Option<SpecTok> = None;
    for item in Scanner::new(content) {
        let Ok(item) = item else { break };
        let cur = match &item.token {
            Token::Comment(_) => continue,
            Token::String(s) => SpecTok::Str(s.as_ref().to_string()),
            Token::Ident(id) => SpecTok::Ident(id.as_ref().to_string()),
            Token::Keyword(Keyword::Import(_)) => SpecTok::Import,
            Token::Punct(Punct::OpenParen) => SpecTok::OpenParen,
            _ => SpecTok::Other,
        };
        if let SpecTok::Str(s) = &cur {
            let is_specifier = match (&prev, &prev2) {
                // require('x')
                (Some(SpecTok::OpenParen), Some(SpecTok::Ident(n))) if n == "require" => true,
                // import('x')
                (Some(SpecTok::OpenParen), Some(SpecTok::Import)) => true,
                // import … from 'x' / export … from 'x' (`from` is contextual)
                (Some(SpecTok::Ident(n)), _) if n == "from" => true,
                // side-effect import 'x'
                (Some(SpecTok::Import), _) => true,
                _ => false,
            };
            if is_specifier && (s.starts_with("./") || s.starts_with("../")) {
                specs.push(s.clone());
            }
        }
        prev2 = prev.take();
        prev = Some(cur);
    }
    specs
}

/// Analyze JS sources for ES syntax level and static runtime-API usage.
/// `html_module` marks a `<script type="module">` (web only) as an extra
/// engine-level signal. Returns `es_level = None` only when there is nothing to
/// analyze (no JS and no module tag).
pub(crate) fn analyze_js(js: &[(String, String)], html_module: bool) -> JsAnalysis {
    let mut features: HashSet<EsFeature> = HashSet::new();
    let mut apis: BTreeMap<String, EsLevel> = BTreeMap::new();
    for (_, content) in js {
        scan_js_tokens(content, &mut features, &mut apis);
    }
    if html_module {
        features.insert(EsFeature::EsModule);
    }

    let es_apis: Vec<ApiUse> = apis
        .into_iter()
        .map(|(name, level)| ApiUse { name, level })
        .collect();

    if js.is_empty() && !html_module {
        return JsAnalysis {
            es_level: None,
            es_features: Vec::new(),
            es_apis,
        };
    }

    // Emit features in a stable, oldest-first order for readable output.
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
    let es_features: Vec<EsFeature> = order.into_iter().filter(|f| features.contains(f)).collect();
    let es_level = Some(
        es_features
            .iter()
            .map(|f| f.level())
            .max()
            .unwrap_or(EsLevel::Es5),
    );
    JsAnalysis {
        es_level,
        es_features,
        es_apis,
    }
}

/// Tokenize one JS source, recording ES syntax features and static API uses.
///
/// `?.`/`??` are reconstructed from an adjacent `?`+`.`/`?` pair (this lexer
/// predates them). `async` is a contextual keyword (an identifier), counted
/// only when followed by `function`/`(`/an identifier. Static APIs are matched
/// as `Global . member` token sequences (`Object.assign`) or bare globals
/// (`globalThis`), so only real code — not strings/comments — is considered.
fn scan_js_tokens(
    content: &str,
    features: &mut HashSet<EsFeature>,
    apis: &mut BTreeMap<String, EsLevel>,
) {
    let mut pending_question_end: Option<usize> = None;
    let mut pending_async = false;
    // For member-API detection: the identifier right before a `.`.
    let mut ident_before_dot: Option<String> = None;
    let mut expect_member = false;

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
                features.insert(EsFeature::AsyncAwait);
            }
        }

        if let Some(q_end) = pending_question_end.take() {
            if span.start == q_end {
                match &item.token {
                    Token::Punct(Punct::Period) => {
                        features.insert(EsFeature::OptionalChaining);
                    }
                    Token::Punct(Punct::QuestionMark) => {
                        features.insert(EsFeature::NullishCoalescing);
                    }
                    _ => {}
                }
            }
        }

        // `.` keeps the previous identifier as a potential API receiver;
        // every other token clears it (handled per-arm below).
        let is_period = matches!(&item.token, Token::Punct(Punct::Period));

        match &item.token {
            Token::Keyword(Keyword::Let(_)) | Token::Keyword(Keyword::Const(_)) => {
                features.insert(EsFeature::LetConst);
            }
            Token::Keyword(Keyword::Class(_)) => {
                features.insert(EsFeature::Class);
            }
            Token::Keyword(Keyword::Await(_)) => {
                features.insert(EsFeature::AsyncAwait);
            }
            Token::Template(_) => {
                features.insert(EsFeature::TemplateLiteral);
            }
            Token::Ident(id) => {
                let s = id.as_ref();
                // `Global.member` API (receiver captured before the dot).
                if expect_member {
                    if let Some(recv) = &ident_before_dot {
                        if let Some(level) = static_api_level(recv, s) {
                            apis.insert(format!("{recv}.{s}"), level);
                        }
                    }
                }
                // Bare global API (e.g. `globalThis`).
                if let Some(level) = bare_api_level(s) {
                    apis.insert(s.to_string(), level);
                }
                if s == "async" {
                    pending_async = true;
                }
            }
            Token::Punct(p) => match p {
                Punct::EqualGreaterThan => {
                    features.insert(EsFeature::Arrow);
                }
                Punct::Ellipsis => {
                    features.insert(EsFeature::Spread);
                }
                Punct::DoubleAsterisk | Punct::DoubleAsteriskEqual => {
                    features.insert(EsFeature::Exponent);
                }
                Punct::QuestionMark => {
                    pending_question_end = Some(span.end);
                }
                _ => {}
            },
            _ => {}
        }

        // Maintain the receiver/member state machine.
        match &item.token {
            Token::Ident(id) => {
                ident_before_dot = Some(id.as_ref().to_string());
                expect_member = false;
            }
            Token::Punct(Punct::Period) => {
                expect_member = ident_before_dot.is_some();
            }
            _ if !is_period => {
                ident_before_dot = None;
                expect_member = false;
            }
            _ => {}
        }
    }
}

/// Level for a `Global.member` static API, or `None` if unremarkable.
/// Only unambiguous namespaced APIs are listed (prototype methods like
/// `.includes`/`.flat` are excluded — their receiver type is unknowable).
fn static_api_level(recv: &str, method: &str) -> Option<EsLevel> {
    use EsLevel::*;
    Some(match (recv, method) {
        ("Object", "assign" | "getOwnPropertySymbols" | "setPrototypeOf") => Es2015,
        ("Object", "values" | "entries" | "getOwnPropertyDescriptors") => Es2017,
        ("Object", "fromEntries") => Es2019,
        ("Array", "from" | "of") => Es2015,
        ("Promise", "allSettled") => Es2020,
        ("Promise", "any") => Es2021Plus,
        (
            "Number",
            "isInteger" | "isNaN" | "isFinite" | "isSafeInteger" | "parseFloat" | "parseInt",
        ) => Es2015,
        (
            "Math",
            "trunc" | "sign" | "cbrt" | "hypot" | "clz32" | "log2" | "log10" | "fround" | "expm1",
        ) => Es2015,
        ("String", "raw" | "fromCodePoint") => Es2015,
        ("Reflect", _) => Es2015, // any Reflect.* is ES2015
        _ => return None,
    })
}

/// Level for a bare global identifier, or `None`.
fn bare_api_level(name: &str) -> Option<EsLevel> {
    use EsLevel::*;
    Some(match name {
        "globalThis" | "BigInt" => Es2020,
        "WeakRef" | "FinalizationRegistry" => Es2021Plus,
        _ => return None,
    })
}

/// Detect bundled polyfill/shim libraries. When present, the app supplies the
/// runtime APIs itself, so the API advisory is meaningless (the bundle
/// *references* every API to feature-detect/define it) and must be suppressed.
/// Returns canonical library names.
pub(crate) fn detect_polyfills(js: &[(String, String)]) -> Vec<String> {
    let mut found: BTreeMap<&str, ()> = BTreeMap::new();
    for (_, c) in js {
        if c.contains("core-js") || c.contains("__core-js_shared__") {
            found.insert("core-js", ());
        }
        if c.contains("@babel/runtime")
            || c.contains("_interopRequireDefault")
            || c.contains("babelHelpers")
        {
            found.insert("@babel/runtime", ());
        }
        if c.contains("regeneratorRuntime") {
            found.insert("regenerator", ());
        }
        if c.contains("es5-shim") || c.contains("es6-shim") {
            found.insert("es-shims", ());
        }
    }
    found.into_keys().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(content: &str) -> Vec<(String, String)> {
        vec![("bundle.js".to_string(), content.to_string())]
    }

    fn apis(src: &str) -> Vec<String> {
        analyze_js(&js(src), false)
            .es_apis
            .into_iter()
            .map(|a| a.name)
            .collect()
    }

    #[test]
    fn detects_static_namespaced_apis() {
        let a = apis("Object.assign({}, x); Object.entries(y); Array.from(z); Promise.allSettled(p);");
        assert!(a.contains(&"Object.assign".to_string()));
        assert!(a.contains(&"Object.entries".to_string()));
        assert!(a.contains(&"Array.from".to_string()));
        assert!(a.contains(&"Promise.allSettled".to_string()));
    }

    #[test]
    fn api_levels_are_correct() {
        let found = analyze_js(&js("Object.fromEntries(x); Object.assign(y);"), false).es_apis;
        let lvl = |n: &str| found.iter().find(|a| a.name == n).map(|a| a.level);
        assert_eq!(lvl("Object.fromEntries"), Some(EsLevel::Es2019));
        assert_eq!(lvl("Object.assign"), Some(EsLevel::Es2015));
    }

    #[test]
    fn detects_bare_globals() {
        assert!(apis("var g = globalThis; var n = BigInt(1);").contains(&"globalThis".to_string()));
    }

    #[test]
    fn prototype_methods_are_not_reported() {
        // `.includes`/`.flat` on an unknown receiver must NOT be flagged.
        let a = apis("myArr.includes(3); list.flat(); s.padStart(2);");
        assert!(a.is_empty());
    }

    #[test]
    fn apis_in_strings_are_ignored() {
        let a = apis(r#"var s = "Object.assign is nice"; // Object.entries(x)"#);
        assert!(a.is_empty());
    }

    #[test]
    fn detects_bundled_polyfills() {
        assert_eq!(
            detect_polyfills(&js("require('core-js/modules/es.object.assign');")),
            vec!["core-js".to_string()]
        );
        assert!(detect_polyfills(&js("function _interopRequireDefault(o){}")).contains(&"@babel/runtime".to_string()));
        assert!(detect_polyfills(&js("var x = regeneratorRuntime.mark(f);")).contains(&"regenerator".to_string()));
        assert!(detect_polyfills(&js("var plain = 1;")).is_empty());
    }

    #[test]
    fn shadowed_object_still_flags_but_syntax_is_clean() {
        // Tokenizer can't do type inference; `Object.assign` is still flagged.
        // This is acceptable for an advisory (never a hard verdict).
        assert!(apis("Object.assign(a, b);").contains(&"Object.assign".to_string()));
    }

    // --- ES syntax level ---

    fn es(src: &str, module: bool) -> (Option<EsLevel>, Vec<EsFeature>) {
        let a = analyze_js(&js(src), module);
        (a.es_level, a.es_features)
    }

    #[test]
    fn es_level_is_max_feature() {
        let (level, feats) = es("const x = a?.b; let y = async () => await z;", false);
        assert_eq!(level, Some(EsLevel::Es2020)); // optional chaining
        assert!(feats.contains(&EsFeature::OptionalChaining));
        assert!(feats.contains(&EsFeature::AsyncAwait));
        assert!(feats.contains(&EsFeature::Arrow));
        assert!(feats.contains(&EsFeature::LetConst));
    }

    #[test]
    fn detects_nullish_coalescing() {
        let (level, feats) = es("var x = a ?? b;", false);
        assert_eq!(level, Some(EsLevel::Es2020));
        assert!(feats.contains(&EsFeature::NullishCoalescing));
    }

    #[test]
    fn es5_bundle_reads_as_es5() {
        let (level, feats) = es("var x = 1; function f() { return x; }", false);
        assert_eq!(level, Some(EsLevel::Es5));
        assert!(feats.is_empty());
    }

    #[test]
    fn features_inside_strings_and_comments_are_ignored() {
        let src = r#"
            // const x = () => {}; a ** b; a?.b; a ?? b; async function q(){}
            var s = "const y = async () => await z ** 2 ?? w ?.p";
            var t = `template-looking ${'but a string'}`;
            var u = 1;
        "#;
        let (level, feats) = es(src, false);
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
        let (level, feats) = es("/** @license v1 */ var x = 1;", false);
        assert_eq!(level, Some(EsLevel::Es5));
        assert!(!feats.contains(&EsFeature::Exponent));
    }

    #[test]
    fn real_exponent_is_detected() {
        let (_level, feats) = es("var y = a ** 2;", false);
        assert!(feats.contains(&EsFeature::Exponent));
    }

    #[test]
    fn async_identifier_variable_is_not_async_function() {
        let (_level, feats) = es("var async = 1; var b = async + 2;", false);
        assert!(!feats.contains(&EsFeature::AsyncAwait));
    }

    #[test]
    fn script_module_raises_es_level_over_es5_bundle() {
        let (level, feats) = es("var x = 1;", true);
        assert_eq!(level, Some(EsLevel::Es2018));
        assert!(feats.contains(&EsFeature::EsModule));
    }
}
