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
        /// Whether the firmware's engine supports the app's ES level.
        es: CompatVerdict,
    },
    Service {
        detection: ServiceRuntimeDetection,
        /// The firmware's Node.js version — informational only. There is no
        /// compat verdict for services: `engines.node` isn't trusted and webOS
        /// services carry no other reliable runtime requirement.
        available_node: Option<Version>,
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
    /// The compatibility verdict for this component on this firmware, if it has
    /// one. Services are informational only (no verdict).
    pub fn verdict(&self) -> Option<&CompatVerdict> {
        match self {
            DetectionResult::WebApp { es, .. } => Some(es),
            DetectionResult::Service { .. } => None,
        }
    }

    /// Whether this component is definitively incompatible (a `Fail` verdict);
    /// `Unknown`/no-verdict is not treated as incompatible.
    pub fn is_incompatible(&self) -> bool {
        matches!(self.verdict(), Some(CompatVerdict::Fail { .. }))
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
    let es = web_verdict(detection.es_level, engine);
    return Some(DetectionResult::WebApp {
        detection,
        engine: engine.cloned(),
        es,
    });
}

fn service_detection(
    svc: &Component<ServiceInfo>,
    node: Option<&Version>,
) -> Option<DetectionResult> {
    let detection = svc.info.runtime.clone()?;
    return Some(DetectionResult::Service {
        detection,
        available_node: node.cloned(),
    });
}

/// Does the firmware's web engine support the app's required ES level?
fn web_verdict(es_level: Option<EsLevel>, engine: Option<&WebEngine>) -> CompatVerdict {
    let Some(es_level) = es_level else {
        return CompatVerdict::Unknown;
    };
    let fw_max = match engine {
        Some(WebEngine::Chromium(v)) => EsLevel::from_chromium_major(v.major as u32),
        // The LG WebKit port (537.x) predates reliable ES2015 support.
        Some(WebEngine::WebKit(_)) => EsLevel::Es5,
        None => return CompatVerdict::Unknown,
    };
    if es_level <= fw_max {
        CompatVerdict::Ok
    } else {
        CompatVerdict::Fail {
            reason: format!(
                "app uses {}, but {} supports up to {}",
                es_level.label(),
                engine.map(|e| e.label()).unwrap_or_default(),
                fw_max.label()
            ),
        }
    }
}

