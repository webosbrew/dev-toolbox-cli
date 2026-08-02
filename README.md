webOS Compatibility Checker

This is a rewrite of [compat-checker](https://github.com/webosbrew/compat-checker) in Rust.

## Exit codes

`webosbrew-ipk-verify` and `webosbrew-elf-verify` use the same codes. A script can
tell an incompatible package from a tool that could not run.

| Code | Meaning                                                                                            |
|------|----------------------------------------------------------------------------------------------------|
| 0    | Everything the tool checked is compatible.                                                          |
| 1    | The tool ran, and found an incompatibility.                                                         |
| 2    | Bad command line. This code comes from the argument parser.                                         |
| 3    | An input file is missing, unreadable, or not in the expected format.                                |
| 4    | No firmware data to check against. Either the data is not installed, or `--fw-releases` matched none. |
| 5    | The tool could not write the report.                                                                |

Code 3 wins over code 1. If the tool cannot read one input, it does not answer the
question you asked, so it reports that first.
