//! Compiles the vendored Kotlin grammar (`vendor/tree-sitter-kotlin-ng`, ADR 0017), with the
//! flags of the published grammar crate's own build script.

fn main() {
    let src = std::path::Path::new("vendor/tree-sitter-kotlin-ng/src");
    let mut build = cc::Build::new();
    build
        .std("c11")
        .include(src)
        .flag_if_supported("-Wno-unused-parameter")
        .file(src.join("parser.c"))
        .file(src.join("scanner.c"));
    #[cfg(target_env = "msvc")]
    build.flag("-utf-8");
    build.compile("tree-sitter-kotlin");
    println!("cargo:rerun-if-changed={}", src.display());
}
