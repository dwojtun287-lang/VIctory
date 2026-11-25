#![cfg(windows)]

#[test]
fn precompile_refusals_fail_even_when_cached() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("missing-dxil.bin");
    let mut bytes = vec![0; 32];
    bytes[..4].copy_from_slice(b"DXBC");
    bytes[24..28].copy_from_slice(&32u32.to_le_bytes());
    std::fs::write(&input, bytes).unwrap();
    for _ in 0..2 {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_victory"))
            .arg("precompile")
            .arg(&input)
            .env("VICTORY_CACHE_DIR", dir.path().join("cache"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("0 cached, 0 translated, 1 failed"),
            "{stdout}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("game launch stopped"), "{stderr}");
    }
}
