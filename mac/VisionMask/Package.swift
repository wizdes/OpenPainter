// swift-tools-version: 6.0
// M53: Apple Vision's foreground mask behind a plain Objective-C interface.
//
// Why a separate package in Objective-C: when a Swift module imports Vision,
// Vision's Swift overlay is loaded into every module that imports it, and on
// the macOS 27 SDK that changes type inference in unrelated code (for example
// `stride(...).first { ... } ?? 0` becomes `Int?`, which broke
// DocumentLifecycleTests). This header names only Foundation and CoreGraphics
// types, so Swift code that imports VisionMask never loads Vision's overlay.
import PackageDescription

let package = Package(
    name: "VisionMask",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "VisionMask", targets: ["VisionMask"])
    ],
    targets: [
        .target(
            name: "VisionMask",
            linkerSettings: [.linkedFramework("Vision"), .linkedFramework("CoreVideo")]
        )
    ]
)
