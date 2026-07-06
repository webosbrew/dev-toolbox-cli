//! Path-containment guard for untrusted package metadata.
//!
//! A `.ipk` is attacker-controlled input. Its metadata carries path fragments
//! that get joined onto the extraction directory and then opened — the app id
//! and service ids from `packageinfo.json`, and the `main`/`executable` entry
//! from `appinfo.json`/`services.json`. Without a check, a value like
//! `../../../../dev/zero` or `/etc/passwd` would make the verifier open a file
//! outside the package (a device read is an easy DoS). `tar`'s `unpack_in`
//! already blocks traversal when *writing* extracted files; this guards the
//! *reads* we do afterwards.

use std::io::{Error, ErrorKind};
use std::path::{Component, Path, PathBuf};

/// Lexically resolve `.` / `..` components without touching the filesystem.
///
/// An extracted package has no on-disk symlinks (they are recorded in memory,
/// never written), so lexical resolution matches canonicalization for our tree
/// while never opening the candidate — a traversal path is rejected before it
/// is ever read.
pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
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

/// Ensure `candidate` stays within `root` after resolving `.`/`..`, returning
/// the normalized path. Rejects path traversal via untrusted package metadata.
pub(crate) fn ensure_within(root: &Path, candidate: &Path) -> Result<PathBuf, Error> {
    let root = lexical_normalize(root);
    let candidate = lexical_normalize(candidate);
    if !candidate.starts_with(&root) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("unsafe path escapes package directory: {}", candidate.display()),
        ));
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_paths_inside_root() {
        let root = Path::new("/tmp/pkg");
        assert!(ensure_within(root, Path::new("/tmp/pkg/usr/palm/app/index.html")).is_ok());
        // `..` that stays within root is fine.
        assert_eq!(
            ensure_within(root, Path::new("/tmp/pkg/usr/../usr/main")).unwrap(),
            PathBuf::from("/tmp/pkg/usr/main")
        );
    }

    #[test]
    fn rejects_traversal_escaping_root() {
        let root = Path::new("/tmp/pkg");
        assert!(ensure_within(root, Path::new("/tmp/pkg/../../../../etc/passwd")).is_err());
        assert!(ensure_within(root, Path::new("/tmp/pkg/a/../../../dev/zero")).is_err());
    }

    #[test]
    fn rejects_absolute_paths_outside_root() {
        let root = Path::new("/tmp/pkg");
        // A `main` of "/etc/passwd" makes join() reset to the absolute path.
        assert!(ensure_within(root, Path::new("/etc/passwd")).is_err());
    }
}
