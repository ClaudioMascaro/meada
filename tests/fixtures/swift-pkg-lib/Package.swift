// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Shared",
    products: [
        .library(name: "Shared", targets: ["Shared", "KitMoney"]),
    ],
    targets: [
        .target(name: "Shared"),
        .target(name: "KitMoney"),
    ]
)
