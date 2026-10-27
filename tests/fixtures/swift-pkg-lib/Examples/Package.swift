// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Examples",
    dependencies: [
        .package(path: ".."),
    ],
    targets: [
        .executableTarget(
            name: "Demo",
            dependencies: [.product(name: "Shared", package: "swift-pkg-lib")]
        ),
    ]
)
