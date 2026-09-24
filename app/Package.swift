// swift-tools-version: 5.9
import Foundation
import PackageDescription

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let archive = root.appendingPathComponent("target/release/libstitchr_core.a").path

let package = Package(
    name: "Stitchr",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "Stitchr", targets: ["Stitchr"])],
    targets: [
        .systemLibrary(name: "stitchr_coreFFI", path: "Sources/stitchr_coreFFI"),
        .executableTarget(
            name: "Stitchr",
            dependencies: ["stitchr_coreFFI"],
            path: "Sources/Stitchr",
            linkerSettings: [.unsafeFlags([archive])]
        ),
    ]
)
