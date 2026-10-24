group = "example"
version = "1.0"
dependencies {
    implementation("example:models:1.0")
    runtimeOnly("example:$artifact:$version")
}
