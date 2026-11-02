rootProject.name = "kit"
include(":tools", ":extras:nested")
dependencyResolutionManagement {
    versionCatalogs {
        create("tools") {
            from(files("gradle/tools.versions.toml"))
        }
    }
}
