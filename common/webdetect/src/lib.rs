//! Web-app and Node-service technology detection for webOS packages.
//!
//! Given the unpacked directory of a non-native component, this crate reports
//! which frontend framework a web app ships and what JavaScript syntax level
//! its bundle requires ([`detect_web_app`]), and what Node.js runtime a service
//! declares ([`detect_service_runtime`]). It is pure text/JSON analysis with no
//! ELF, ipk or firmware knowledge so it can be unit-tested in isolation.

use semver::Version;

mod eslevel;
mod js;
mod service;
mod web;

pub use eslevel::{EsFeature, EsLevel};
pub use service::detect_service_runtime;
pub use web::detect_web_app;

/// A notable static runtime API used by the code (e.g. `Object.assign`), with
/// the ES level that introduced it. Advisory only — such APIs can be
/// polyfilled, so their use never gates compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiUse {
    pub name: String,
    pub level: EsLevel,
}

/// What was detected about a web/frontend app.
#[derive(Debug, Clone)]
pub struct WebAppDetection {
    /// The primary UI framework (always `Some`; `PlainHtml` when none matched).
    pub framework: Option<FrameworkInfo>,
    /// Other frameworks/libraries also present (e.g. jQuery alongside Enact).
    pub also_present: Vec<FrameworkInfo>,
    /// webOSTV.js SDK: `None` absent, `Some(None)` present with unknown version,
    /// `Some(Some(v))` present with a detected version.
    pub webostvjs: Option<Option<Version>>,
    /// Minimum ES level the shipped bundle requires of the engine.
    pub es_level: Option<EsLevel>,
    /// The syntax features that evidence `es_level`.
    pub es_features: Vec<EsFeature>,
    /// Notable static runtime APIs used (advisory — may be polyfilled).
    pub es_apis: Vec<ApiUse>,
    /// Bundled polyfill libraries detected (core-js, @babel/runtime, ...). When
    /// non-empty the app self-polyfills, so the API advisory is suppressed.
    pub polyfills: Vec<String>,
    /// Distinct remote resource URLs (`http(s)://` or `//host/...`) referenced
    /// by `index.html`. Informational — does not affect the compat verdict.
    pub remote_resources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FrameworkInfo {
    pub kind: FrameworkKind,
    pub version: Option<Version>,
}

impl FrameworkInfo {
    pub fn new(kind: FrameworkKind, version: Option<Version>) -> Self {
        FrameworkInfo { kind, version }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkKind {
    Enact,
    Enyo,
    React,
    Vue,
    Angular,
    AngularJs,
    Jquery,
    WebOsTvJs,
    PlainHtml,
}

impl FrameworkKind {
    pub fn label(self) -> &'static str {
        match self {
            FrameworkKind::Enact => "Enact",
            FrameworkKind::Enyo => "Enyo",
            FrameworkKind::React => "React",
            FrameworkKind::Vue => "Vue",
            FrameworkKind::Angular => "Angular",
            FrameworkKind::AngularJs => "AngularJS",
            FrameworkKind::Jquery => "jQuery",
            FrameworkKind::WebOsTvJs => "webOSTV.js",
            FrameworkKind::PlainHtml => "Plain HTML/JS",
        }
    }
}

/// What was detected about a Node.js/JS service from its `package.json`.
///
/// `engines.node` is intentionally excluded — webOS services don't declare it
/// reliably, so it is not trusted as a runtime requirement.
#[derive(Debug, Clone, Default)]
pub struct ServiceRuntimeDetection {
    /// The `main` entry point, if declared.
    pub main: Option<String>,
    /// Minimum ES level the service's own code requires — a code-derived,
    /// trustworthy requirement checked against the firmware's Node.js version.
    pub es_level: Option<EsLevel>,
    /// The syntax features that evidence `es_level`.
    pub es_features: Vec<EsFeature>,
    /// Notable static runtime APIs used (advisory — may be polyfilled).
    pub es_apis: Vec<ApiUse>,
    /// Bundled polyfill libraries detected — suppresses the API advisory.
    pub polyfills: Vec<String>,
}
