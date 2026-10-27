// swift-tools-version:5.9
import PackageDescription

let vendor = "kit"
let token = "t"

let package = Package(
    name: "Shop",
    dependencies: [
        .package(url: "https://example.com/swift/kit.git", from: "1.2.0"),
        .package(path: "../swift-pkg-lib"),
        .package(url: "git@example.com:swift/\(vendor).git", exact: "2.0.0"),
        .package(url: "https://example.com/swift/dyn.git", from: "0.1.0"),
        .package(url: "https://\(token)@example.com/swift/lib.git", from: "1.0.0"),
        .package(url: "https://github.com/vapor/vapor.git", from: "4.0.0"),
    ],
    targets: [
        .executableTarget(
            name: "App",
            dependencies: [
                .product(name: "Kit", package: "kit"),
                .product(name: "Shared", package: "swift-pkg-lib"),
            ]
        ),
    ]
)
