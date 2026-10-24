// swift-tools-version:6.0
// M09 presenter spike host. Build the Rust library first:
//   cd ../../engine && cargo build -p op-present-spike --release
import Foundation
import PackageDescription

let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let rustLib = packageDir.appendingPathComponent("../../engine/target/release").standardizedFileURL.path

let package = Package(
    name: "PresenterSpike",
    platforms: [.macOS(.v15)],
    targets: [
        .systemLibrary(name: "COpPresentSpike", path: "Sources/COpPresentSpike"),
        .target(
            name: "PresenterSpikeKit",
            dependencies: ["COpPresentSpike"],
            swiftSettings: [.swiftLanguageMode(.v5)],
            linkerSettings: [
                .unsafeFlags(["-L", rustLib]),
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("IOSurface"),
                .linkedFramework("AppKit"),
                .linkedLibrary("objc"),
                .linkedLibrary("iconv"),
            ]
        ),
        .executableTarget(
            name: "PresenterSpikeHost",
            dependencies: ["PresenterSpikeKit"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .testTarget(
            name: "PresenterSpikeTests",
            dependencies: ["PresenterSpikeKit"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
    ]
)
