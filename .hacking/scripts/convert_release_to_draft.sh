#!/bin/bash
set -eo pipefail

VERSION="v$(grep "^version" Cargo.toml | awk -F '"' '{print $2}')"
echo "VERSION: $VERSION"

gh release edit "$VERSION" --draft=true