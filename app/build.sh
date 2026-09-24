#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
archive="$root/target/release/libstitchr_core.a"
swift_sources="$root/app/Sources/Stitchr/Generated"
ffi_sources="$root/app/Sources/stitchr_coreFFI"
bundle="$root/target/Stitchr.app"

cargo build --manifest-path "$root/Cargo.toml" --release -p stitchr-core
mkdir -p "$swift_sources" "$ffi_sources"

cargo run --quiet --manifest-path "$root/Cargo.toml" -p stitchr-bindgen --bin uniffi-bindgen-swift -- "$archive" "$swift_sources" --swift-sources
cargo run --quiet --manifest-path "$root/Cargo.toml" -p stitchr-bindgen --bin uniffi-bindgen-swift -- "$archive" "$ffi_sources" --headers
cargo run --quiet --manifest-path "$root/Cargo.toml" -p stitchr-bindgen --bin uniffi-bindgen-swift -- "$archive" "$ffi_sources" --modulemap --module-name stitchr_coreFFI --modulemap-filename module.modulemap

# CLT's macOS 27 SDK needs SwiftUIMacros, which is only shipped with Xcode.
export DEVELOPER_DIR="$(env -u DEVELOPER_DIR -u SDKROOT /usr/bin/xcode-select -p)"
export SDKROOT="$(/usr/bin/xcrun --sdk "${STITCHR_SDK:-macosx26.5}" --show-sdk-path)"
swift build --package-path "$root/app" -c release --scratch-path "$root/target/swift-build"
bin_dir="$(swift build --package-path "$root/app" -c release --scratch-path "$root/target/swift-build" --show-bin-path)"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$bin_dir/Stitchr" "$bundle/Contents/MacOS/Stitchr"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$bundle"
printf 'Built %s\n' "$bundle"
