// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "one",
    targets: [
        .executableTarget(name: "App"),
        .testTarget(name: "AppTests", dependencies: ["App"]),
    ]
)
