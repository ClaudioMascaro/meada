# JVM builds and Spring configuration

JVM readers shared by Java and Kotlin packs and by contract kinds that use Maven, Gradle or Spring.

## Interface

Maven and Gradle supply coordinates, dependencies and build metadata. The Spring reader resolves properties and returns the configured value of a recognized `@Value` annotation. `[maven.rs:read]` `[gradle.rs:read]` `[mod.rs:Build]` `[spring.rs:resolve]` `[spring.rs:inject]`

## Invariants

Literal coordinates validate group and artifact. Spring configuration considers the module’s base files and prefers `application.properties` over YAML. `[mod.rs:coordinate]` `[spring.rs:Configs]`

The Gradle reader returns an empty summary when blocks nest more than 64 levels deep. `[gradle.rs:GRADLE_DEPTH]`

## Side Effects

The readers interpret supplied content without running Maven, Gradle or the Spring application. `[maven.rs:read]` `[gradle.rs:read]` `[spring.rs:Configs]`

## Verification

From the repository root, run `cargo test -p meada --test it jvmpkg::`.
