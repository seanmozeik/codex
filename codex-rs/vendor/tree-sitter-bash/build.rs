fn main() {
    let mut build = cc::Build::new();
    build
        .std("c11")
        .include("src")
        .flag_if_supported("-Wno-unused-value");

    #[cfg(target_env = "msvc")]
    build.flag("-utf-8");

    // Rename definitions and references together without editing generated C.
    for symbol in [
        "tree_sitter_bash",
        "tree_sitter_bash_external_scanner_create",
        "tree_sitter_bash_external_scanner_destroy",
        "tree_sitter_bash_external_scanner_scan",
        "tree_sitter_bash_external_scanner_serialize",
        "tree_sitter_bash_external_scanner_deserialize",
    ] {
        build.define(symbol, format!("codex_{symbol}").as_str());
    }
    for path in ["src/parser.c", "src/scanner.c"] {
        build.file(path);
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=src/tree_sitter");
    build.compile("codex-tree-sitter-bash");
}
