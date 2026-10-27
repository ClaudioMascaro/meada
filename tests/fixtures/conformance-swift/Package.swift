// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Shop",
    products: [
        .library(name: "Store", targets: ["Store"]),
    ],
    targets: [
        .target(name: "Store"),
        .executableTarget(name: "App", dependencies: ["Store"], path: "Sources/Checkout"),
        .executableTarget(name: "Tool", dependencies: ["Store"]),
    ]
)
