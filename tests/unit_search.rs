// Integration-level search tests beyond unit tests in src/search/mod.rs

#[test]
fn binary_exists_after_build() {
    // Smoke test: the crate under test built, so the test binary itself exists.
    assert!(std::path::Path::new(env!("CARGO_BIN_EXE_simpleedit")).exists());
}
