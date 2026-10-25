group = "dev.shop"
version = "1.0"

val artifact = "util"

dependencies {
    implementation("dev.kt:toolbox:3.1")
    api(group = "dev.kt.core", name = "core", version = "3.1")
    api("dev.kt:toolbox:3.1") {
        exclude(group = "org.slf4j")
    }
    compileOnly("""dev.kt.http:client:3.1""")
    runtimeOnly("dev.kt:util")
    testImplementation("dev.kt:toolbox:3.1")
    testCompileOnly("dev.kt.core:core:3.1")
    testRuntimeOnly("dev.kt.http:client:3.1")
    /*
     * Kotlin block comments nest.
     /* old */
     runtimeOnly("dev.kt.core:core:9.9")
     */
    implementation(project(":web"))
    implementation(kotlin("stdlib"))
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("dev.kt:${artifact}:3.1")
}
