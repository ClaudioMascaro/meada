// swift-tools-version:5.9
import PackageDescription

// The name is computed: the package is a provider no name can pick out.
let name = "Dyn"

let package = Package(
    name: name,
    products: [
        .library(name: "Dyn", targets: ["Dyn"]),
    ],
    targets: [
        .target(name: "Dyn"),
    ]
)
