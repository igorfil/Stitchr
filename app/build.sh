#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
    ""|--test) ;;
    *) printf 'Usage: bash app/build.sh [--test]\n' >&2; exit 2 ;;
esac

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

export DEVELOPER_DIR="$(env -u DEVELOPER_DIR -u SDKROOT /usr/bin/xcode-select -p)"
SDKROOT="$(/usr/bin/xcrun --sdk "${STITCHR_SDK:-macosx}" --show-sdk-path)"
if [[ -z "${STITCHR_SDK:-}" ]]; then
    compatible_sdk=""
    for sdk in "$SDKROOT" "$(dirname "$SDKROOT")"/MacOSX*.sdk; do
        # Some CLT SDKs require SwiftUI macro plugins only shipped with Xcode.
        if swiftc -sdk "$sdk" -typecheck - >/dev/null 2>&1 <<'SWIFT'
import SwiftUI
struct SDKCheck: View {
    @State private var value = 0
    var body: some View { Text("Stitchr") }
}
SWIFT
        then
            compatible_sdk="$sdk"
            break
        fi
    done
    if [[ -z "$compatible_sdk" ]]; then
        printf 'No compatible macOS SDK found. Install a compatible Xcode/SDK or set STITCHR_SDK explicitly.\n' >&2
        exit 1
    fi
    SDKROOT="$compatible_sdk"
fi
export SDKROOT
swift build --package-path "$root/app" -c release --scratch-path "$root/target/swift-build"
bin_dir="$(swift build --package-path "$root/app" -c release --scratch-path "$root/target/swift-build" --show-bin-path)"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$bin_dir/Stitchr" "$bundle/Contents/MacOS/Stitchr"
cp "$root/app/Info.plist" "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - "$bundle"
printf 'Built %s\n' "$bundle"

if [[ "${1:-}" == --test ]]; then
    swift test --package-path "$root/app" --scratch-path "$root/target/swift-build" --disable-xctest
fi
