// swift-tools-version:5.9
import PackageDescription

let vendor = "kit"

let package = Package(
    name: "Sample",
    products: [
        .library(name: "Sample", targets: ["Core", "Money"]),
        .executable(name: "sample-tool", targets: ["Tool"]),
    ],
    dependencies: [
        .package(url: "https://example.com/org/kit.git", from: "1.2.0"),
        .package(path: "../lib"),
        .package(url: "git@example.com:org/\(vendor).git", exact: "2.0.0"),
    ],
    targets: [
        .target(name: "Core"),
        .target(name: "Money", path: "Sources/Currency"),
        .executableTarget(name: "Tool"),
    ]
)
