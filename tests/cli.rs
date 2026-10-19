use std::process::Command;

#[test]
fn version_flag_prints_the_package_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_ref_inserter"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
}
