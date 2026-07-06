use bin_lib::LibraryInfo;
use fw_lib::WebEngine;
use ipk_lib::{AppInfo, Component, Package, ServiceInfo};
use semver::Version;
use webdetect_lib::{EsLevel, ServiceRuntimeDetection, WebAppDetection};

use crate::{bin::BinVerifyResult, Verify, VerifyResult};

pub mod component;

#[derive(Debug)]
pub struct PackageVerifyResult {
    pub app: ComponentVerifyResult,
    pub services: Vec<ComponentVerifyResult>,
}

#[derive(Debug)]
pub struct ComponentVerifyResult {
    pub id: String,
    pub exe: ComponentBinVerifyResult,
    pub libs: Vec<(bool, ComponentBinVerifyResult)>,
    /// Non-native technology detection + per-firmware compatibility. `None` for
    /// native components (which go through the exe/libs path instead).
    pub detection: Option<DetectionResult>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ComponentBinVerifyResult {
    Skipped { name: String },
    Ok { name: String },
    Failed(BinVerifyResult),
}

/// Detected technology for a non-native component, paired with the verdict
/// against one firmware's runtime.
#[derive(Debug, Clone)]
pub enum DetectionResult {
    WebApp {
        detection: WebAppDetection,
        /// The firmware's web engine (for rendering the compat column).
        engine: Option<WebEngine>,
        /// Whether the firmware's engine supports the app's ES **syntax** level
        /// (gating — syntax can't be polyfilled).
        es: CompatVerdict,
        /// Whether the firmware's engine natively provides the runtime APIs the
        /// app uses (advisory — APIs can be polyfilled).
        api: CompatVerdict,
    },
    Service {
        detection: ServiceRuntimeDetection,
        /// The firmware's Node.js version.
        available_node: Option<Version>,
        /// Whether the firmware's Node.js supports the service's ES **syntax**
        /// level (gating — derived from the code, not `engines.node`).
        node: CompatVerdict,
        /// Whether the firmware's Node.js natively provides the runtime APIs the
        /// service uses (advisory).
        api: CompatVerdict,
    },
}

/// Per-firmware compatibility outcome for a detected runtime requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatVerdict {
    Ok,
    Fail { reason: String },
    Unknown,
}

impl DetectionResult {
    /// The gating compatibility verdict for this component on this firmware
    /// (web app: ES syntax vs engine; service: ES syntax vs Node.js).
    pub fn verdict(&self) -> &CompatVerdict {
        match self {
            DetectionResult::WebApp { es, .. } => es,
            DetectionResult::Service { node, .. } => node,
        }
    }

    /// The advisory runtime-API verdict — never gates compatibility.
    pub fn api_advisory(&self) -> &CompatVerdict {
        match self {
            DetectionResult::WebApp { api, .. } => api,
            DetectionResult::Service { api, .. } => api,
        }
    }

    /// Whether this component is definitively incompatible (a gating `Fail`);
    /// `Unknown` and the advisory API verdict never count.
    pub fn is_incompatible(&self) -> bool {
        matches!(self.verdict(), CompatVerdict::Fail { .. })
    }
}

impl Verify<PackageVerifyResult> for Package {
    fn verify<F>(&self, find_library: &F) -> PackageVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        return PackageVerifyResult {
            app: self.app.verify(find_library),
            services: self
                .services
                .iter()
                .map(|svc| svc.verify(find_library))
                .collect(),
        };
    }
}

impl VerifyResult for PackageVerifyResult {
    fn is_good(&self) -> bool {
        return self.app.is_good() && self.services.iter().all(|s| s.is_good());
    }
}

/// Verify a package against one firmware, layering non-native technology
/// compatibility on top of the native checks. Implemented for [`Package`]
/// (which lives in another crate, hence an extension trait).
pub trait VerifyForFirmware {
    fn verify_for_firmware<F>(
        &self,
        find_library: &F,
        node: Option<&Version>,
        engine: Option<&WebEngine>,
    ) -> PackageVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>;
}

impl VerifyForFirmware for Package {
    /// `node` and `engine` are the target firmware's resolved runtimes.
    fn verify_for_firmware<F>(
        &self,
        find_library: &F,
        node: Option<&Version>,
        engine: Option<&WebEngine>,
    ) -> PackageVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        let mut result = self.verify(find_library);
        result.app.detection = web_detection(&self.app, engine);
        for (svc_result, svc) in result.services.iter_mut().zip(self.services.iter()) {
            svc_result.detection = service_detection(svc, node);
        }
        return result;
    }
}

fn web_detection(app: &Component<AppInfo>, engine: Option<&WebEngine>) -> Option<DetectionResult> {
    let detection = app.info.web.clone()?;
    let es = web_verdict(detection.es_level, engine, "app uses");
    // Advisory: highest ES level implied by the runtime APIs used.
    let api_level = detection.es_apis.iter().map(|a| a.level).max();
    let api = web_verdict(api_level, engine, "app calls APIs from").demote_reason(
        "may need polyfills",
    );
    return Some(DetectionResult::WebApp {
        detection,
        engine: engine.cloned(),
        es,
        api,
    });
}

fn service_detection(
    svc: &Component<ServiceInfo>,
    node: Option<&Version>,
) -> Option<DetectionResult> {
    let detection = svc.info.runtime.clone()?;
    let node_verdict = service_verdict(detection.es_level, node, "service uses");
    let api_level = detection.es_apis.iter().map(|a| a.level).max();
    let api = service_verdict(api_level, node, "service calls APIs from")
        .demote_reason("may need polyfills");
    return Some(DetectionResult::Service {
        detection,
        available_node: node.cloned(),
        node: node_verdict,
        api,
    });
}

/// Whether the firmware's web engine supports the given ES level.
fn web_verdict(es_level: Option<EsLevel>, engine: Option<&WebEngine>, verb: &str) -> CompatVerdict {
    let Some(es_level) = es_level else {
        return CompatVerdict::Unknown;
    };
    let Some(engine) = engine else {
        return CompatVerdict::Unknown;
    };
    let fw_max = engine_max_es(engine);
    if es_level <= fw_max {
        CompatVerdict::Ok
    } else {
        CompatVerdict::Fail {
            reason: format!(
                "{verb} {}, but {} supports up to {}",
                es_level.label(),
                engine.label(),
                fw_max.label()
            ),
        }
    }
}

/// Whether the firmware's Node.js supports the given ES level.
fn service_verdict(es_level: Option<EsLevel>, node: Option<&Version>, verb: &str) -> CompatVerdict {
    let Some(es_level) = es_level else {
        return CompatVerdict::Unknown;
    };
    let Some(node) = node else {
        return CompatVerdict::Unknown;
    };
    let (maj, min) = es_level.min_node_version();
    if node.major > maj || (node.major == maj && node.minor >= min) {
        CompatVerdict::Ok
    } else {
        CompatVerdict::Fail {
            reason: format!(
                "{verb} {}, which needs Node.js {maj}.{min}, but firmware ships {node}",
                es_level.label(),
            ),
        }
    }
}

/// The highest ES level a firmware's web engine supports.
pub fn engine_max_es(engine: &WebEngine) -> EsLevel {
    match engine {
        WebEngine::Chromium(v) => EsLevel::from_chromium_major(v.major as u32),
        // The LG WebKit port (537.x) predates reliable ES2015 support.
        WebEngine::WebKit(_) => EsLevel::Es5,
    }
}

impl CompatVerdict {
    /// Rewrite a `Fail` reason's lead-in so the advisory reads as a polyfill
    /// note rather than a hard failure. No-op for `Ok`/`Unknown`.
    fn demote_reason(self, note: &str) -> CompatVerdict {
        match self {
            CompatVerdict::Fail { reason } => CompatVerdict::Fail {
                reason: format!("{reason} ({note})"),
            },
            other => other,
        }
    }
}

