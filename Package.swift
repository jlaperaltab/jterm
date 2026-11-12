// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "jTerm",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(path: "Vendor/SwiftTerm")
    ],
    targets: [
        .executableTarget(
            name: "jTerm",
            dependencies: [.product(name: "SwiftTerm", package: "SwiftTerm")],
            path: "Sources/jTerm"
        )
    ]
)
