#!/usr/bin/env bash
# Version aus Cargo.toml ([workspace.package] version) — eine Stelle für alle Pakete.
set -euo pipefail
cd "$(dirname "$0")/.."
sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1
