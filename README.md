webOS Compatibility Checker

This is a rewrite of [compat-checker](https://github.com/webosbrew/compat-checker) in Rust.

## Exit codes

All four tools use the same codes. A script can tell an incompatible package from
a tool that could not run. A tool reports a bad input or a write failure and
exits. It does not panic.

| Code | Meaning                                                                                            |
|------|----------------------------------------------------------------------------------------------------|
| 0    | Everything the tool checked is compatible.                                                          |
| 1    | The tool ran, and found an incompatibility.                                                         |
| 2    | Bad command line. This code comes from the argument parser.                                         |
| 3    | An input file is missing, unreadable, or not in the expected format.                                |
| 4    | No firmware data to check against. Either the data is not installed, or `--fw-releases` matched none. |
| 5    | The tool could not write its output.                                                                |

Only `webosbrew-ipk-verify` and `webosbrew-elf-verify` use codes 1 and 4.

Code 3 wins over code 1. If the tool cannot read one input, it does not answer the
question you asked, so it reports that first.
