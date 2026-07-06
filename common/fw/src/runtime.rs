//! Per-firmware runtime versions (Node.js, web engine) read from `packages.json`.
//!
//! webOS ships the JS-service Node.js runtime and the web-app engine as ordinary
//! OS packages, so their versions live in each firmware's `packages.json` rather
//! than in a table we would have to maintain by hand. The package *names* vary by
//! generation, so resolution tries a family of candidates.

use semver::Version;

use crate::Firmware;

/// The web-app rendering engine a firmware ships. webOS has used two families:
/// a modern Chromium-based runtime (WAM) and, on the earliest TVs, an LG WebKit
/// port (`webkit-starfish`, versioned like `537.41`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEngine {
    Chromium(Version),
    WebKit(Version),
}

impl WebEngine {
    /// Human-readable label, e.g. "Chromium 120" or "WebKit 537.41".
    pub fn label(&self) -> String {
        match self {
            WebEngine::Chromium(v) => format!("Chromium {}", v.major),
            WebEngine::WebKit(v) => format!("WebKit {}.{}", v.major, v.minor),
        }
    }
}

impl Firmware {
    /// The Node.js version this firmware ships, if a node package is present.
    pub fn node_version(&self) -> Option<Version> {
        ["lib32-nodejs", "nodejs"]
            .iter()
            .find_map(|name| self.pkg_version(name))
    }

    /// The web-app engine this firmware ships. Resolution order:
    /// 1. Chromium family: `lib32-webruntime` → `webruntime` → highest
    ///    `chromium<NN>` → `chromium-webos` → `chromium`;
    /// 2. WebKit family: `webkit-starfish` → `qt5-qtwebkit` → `libQt5WebKit`.
    ///
    /// `com.webos.app.browser` is deliberately ignored — it is the built-in
    /// browser *app*, not the web-app runtime.
    pub fn web_engine(&self) -> Option<WebEngine> {
        for name in ["lib32-webruntime", "webruntime"] {
            if let Some(v) = self.pkg_version(name) {
                return Some(WebEngine::Chromium(v));
            }
        }
        if let Some(v) = self.highest_numbered_chromium() {
            return Some(WebEngine::Chromium(v));
        }
        for name in ["chromium-webos", "chromium"] {
            if let Some(v) = self.pkg_version(name) {
                return Some(WebEngine::Chromium(v));
            }
        }
        for name in ["webkit-starfish", "qt5-qtwebkit", "libQt5WebKit"] {
            if let Some(v) = self.pkg_version(name) {
                return Some(WebEngine::WebKit(v));
            }
        }
        None
    }

    /// Parse a package's `upstream` version string into a [`Version`].
    fn pkg_version(&self, name: &str) -> Option<Version> {
        self.packages
            .get(name)
            .and_then(|entry| parse_leading_semver(&entry.version.upstream))
    }

    /// Among version-suffixed `chromium<NN>` packages (`chromium38`,
    /// `chromium53`, ...), the one with the highest `NN`.
    fn highest_numbered_chromium(&self) -> Option<Version> {
        let mut best: Option<(u32, &str)> = None;
        for key in self.packages.keys() {
            if let Some(rest) = key.strip_prefix("chromium") {
                if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
                    continue;
                }
                if let Ok(n) = rest.parse::<u32>() {
                    if best.map_or(true, |(b, _)| n > b) {
                        best = Some((n, key));
                    }
                }
            }
        }
        best.and_then(|(_, key)| self.pkg_version(key))
    }
}

/// Take the leading dotted-numeric run of an `upstream` string and parse it as a
/// three-component [`Version`]. Debian `upstream` strings are frequently longer
/// than semver allows (`120.0.6099.270-137.paparoa.1`) or have a `-suffix`
/// (`53.0.2785.34-92...`), so keep only the first three numeric parts.
fn parse_leading_semver(upstream: &str) -> Option<Version> {
    let lead: String = upstream
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = lead.split('.').filter(|s| !s.is_empty());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(Version::new(major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Firmware;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn parses_various_upstream_strings() {
        assert_eq!(parse_leading_semver("16.20.2"), Some(Version::new(16, 20, 2)));
        assert_eq!(parse_leading_semver("0.10.15"), Some(Version::new(0, 10, 15)));
        assert_eq!(
            parse_leading_semver("120.0.6099.270-137.paparoa.1"),
            Some(Version::new(120, 0, 6099))
        );
        assert_eq!(
            parse_leading_semver("53.0.2785.34-92.323.glacier.45"),
            Some(Version::new(53, 0, 2785))
        );
        assert_eq!(
            parse_leading_semver("537.41-420.afro.2"),
            Some(Version::new(537, 41, 0))
        );
        assert_eq!(parse_leading_semver("garbage"), None);
    }

    /// Load every committed firmware data dir and assert the runtime accessors
    /// resolve the packages we validated by hand.
    #[test]
    fn resolves_runtimes_from_real_data() {
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
        if !data.exists() {
            return; // data may be absent in some checkouts
        }
        let firmwares = Firmware::list(&data).expect("list firmwares");
        let by_release: HashMap<String, &Firmware> = firmwares
            .iter()
            .map(|f| (f.info.release.to_string(), f))
            .collect();

        let engine = |rel: &str| by_release.get(rel).and_then(|f| f.web_engine());
        let node = |rel: &str| by_release.get(rel).and_then(|f| f.node_version());

        // Chromium generations, including the version-suffixed names.
        assert_eq!(engine("10.2.0"), Some(WebEngine::Chromium(Version::new(120, 0, 6099))));
        assert_eq!(engine("4.4.2"), Some(WebEngine::Chromium(Version::new(53, 0, 2785))));
        assert_eq!(engine("3.4.0"), Some(WebEngine::Chromium(Version::new(38, 0, 2125))));
        assert_eq!(engine("1.2.0"), Some(WebEngine::Chromium(Version::new(26, 0, 1410))));

        // Pre-Chromium WebKit images.
        assert_eq!(engine("2.2.3"), Some(WebEngine::WebKit(Version::new(537, 41, 0))));
        assert!(matches!(engine("1.4.0"), Some(WebEngine::WebKit(_))));

        // Node.js.
        assert_eq!(node("10.2.0"), Some(Version::new(16, 20, 2)));
        assert_eq!(node("6.4.0"), Some(Version::new(8, 12, 0)));
    }
}
