# Releasing

All crates share one version, set by `workspace.package.version` in the root
`Cargo.toml`. Every Debian package in a release carries that version.

## GitHub release

1. Bump `workspace.package.version`, run `cargo check --workspace` to update
   `Cargo.lock`, then commit.
2. Tag the commit and publish a GitHub release for the tag. Tags follow
   `v<date>-<short sha>`, for example `v20260714-a981208`.
3. The `Release` workflow runs on `released` and uploads one `.deb` per tool,
   for `amd64` and `arm64`.

The workflow builds on `ubuntu-22.04` to keep the glibc requirement low. If
that runner image goes away, move to the next oldest one.

A prerelease does not trigger the workflow. Publish it as a full release, or
change the trigger to `published`.

## Firmware data

`webosbrew-toolbox-fw-symbols` ships `common/data` to
`/usr/share/webosbrew/compat-checker/data`. The tools that read it
(`elf-verify`, `ipk-verify`) depend on that package and build with the
`linux-install` feature, which points `Firmware::data_path()` at that
directory.

Without that feature the path is the source tree of the machine that built the
binary. That is fine for `cargo run` and `cargo install --path`, but it is why
the release only ships Debian packages: a binary copied to another machine
would not find the data.
