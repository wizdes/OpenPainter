// swift-tools-version: 6.0
// Build the engine first: from engine/, run `cargo build -p op-ffi --release`.
// See README.md for the full build order.
import Foundation
import PackageDescription

let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
// The release staticlib libop_ffi.a. OP_ENGINE_LIB_DIR overrides the default
// engine/target/release folder.
let engineLibDir =
    ProcessInfo.processInfo.environment["OP_ENGINE_LIB_DIR"]
    ?? packageDir.appendingPathComponent("../../engine/target/release").standardizedFileURL.path

let package = Package(
    name: "EngineBridge",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "EngineBridge", targets: ["EngineBridge"])
    ],
    targets: [
        // Module map over engine/op-ffi/include/openpainter.h; links op_ffi.
        .systemLibrary(name: "COpenPainter", path: "Sources/COpenPainter"),
        .target(
            name: "EngineBridge",
            dependencies: ["COpenPainter"],
            linkerSettings: [
                .unsafeFlags(["-L", engineLibDir]),
                // Native libraries the Rust staticlib needs
                // (cargo rustc -p op-ffi -- --print native-static-libs).
                .linkedLibrary("iconv"),
            ]
        ),
        .testTarget(name: "EngineBridgeTests", dependencies: ["EngineBridge"]),
    ]
)
