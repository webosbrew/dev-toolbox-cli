//! Exit codes and small helpers shared by the command line tools.
//!
//! A script must be able to tell "the package is incompatible" from "the tool
//! could not run". Code 2 is left to clap, which uses it for a usage error.

use std::borrow::Cow;
use std::path::Path;
use std::process::exit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    /// Everything the tool checked is compatible.
    Ok = 0,
    /// The tool ran, and found an incompatibility.
    Incompatible = 1,
    /// An input file is missing, unreadable, or not in the expected format.
    BadInput = 3,
    /// There is no firmware data to check against. Either the data is not
    /// installed, or the `--fw-releases` filter matched nothing.
    NoFirmware = 4,
    /// The tool could not write its output.
    OutputError = 5,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        return self as i32;
    }

    /// End the process with this code.
    pub fn exit(self) -> ! {
        exit(self.code());
    }
}

/// The name to show for a path. A path such as `..` has no file name, so fall
/// back to the whole path.
pub fn file_label(path: &Path) -> Cow<'_, str> {
    return path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
}
