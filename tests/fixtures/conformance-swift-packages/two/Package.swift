// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "two",
    targets: [
        .executableTarget(name: "App"),
        .testTarget(name: "AppTests", dependencies: ["App"]),
    ]
)
