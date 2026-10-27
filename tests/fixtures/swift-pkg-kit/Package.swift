// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Kit",
    products: [
        .library(name: "Kit", targets: ["Kit", "KitMoney"]),
        .executable(name: "kit-tool", targets: ["Hidden"]),
    ],
    targets: [
        .target(name: "Kit"),
        .target(name: "KitMoney"),
        .target(name: "Hidden"),
        .testTarget(name: "KitTests", dependencies: ["Kit"]),
    ]
)
