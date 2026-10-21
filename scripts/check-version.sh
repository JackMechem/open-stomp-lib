#!/usr/bin/env bash
# Cargo.toml ([workspace.package]) is the single source of truth for the version of the crate and
# the Python package. Check that the README and CHANGELOG
# mention it, and (with an argument, e.g. from a release tag) that it matches.
#
#   scripts/check-version.sh            consistency check
#   scripts/check-version.sh 0.1.0      ... and the version must be 0.1.0
set -euo pipefail
cd "$(dirname "$0")/.."

v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ok=1
fail() { echo "error: $*" >&2; ok=0; }

[[ -n "$v" ]] || fail "no version in Cargo.toml"
[[ $# -eq 0 || "$1" == "$v" ]] || fail "expected version $1, Cargo.toml has $v"
grep -q "Version \*\*$v\*\*" README.md || fail "README.md does not say 'Version **$v**'"
grep -q "^## \[$v\]" CHANGELOG.md || fail "CHANGELOG.md has no '## [$v]' section"
grep -q "tag = \"v$v\"" README.md || fail "README.md install snippet does not pin tag v$v"
awk '/name = "open-stomp"/{getline; print}' Cargo.lock | grep -q "\"$v\"" || fail "Cargo.lock is stale (run cargo build)"
awk '/name = "open-stomp-python"/{getline; print}' Cargo.lock | grep -q "\"$v\"" || fail "Cargo.lock is stale for the Python bindings"

((ok)) && echo "version $v is consistent"
((ok))
