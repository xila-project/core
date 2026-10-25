use std::process::Command;

/// Explicit opt-in: starts passt's normal user-namespace sandbox and binds a
/// local DNS fixture. Does not require Internet or create a TAP device.
#[test]
#[ignore = "set PASST_BIN to a local passt executable; requires user namespaces and an available unprivileged 127.0.0.2:53"]
fn local_tcp_udp_dns_through_passt() {
    let passt = std::env::var_os("PASST_BIN").expect("set PASST_BIN");
    let child = Command::new(env!("CARGO_BIN_EXE_xila-packet-network-prototype"))
        .arg(passt)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let directory = std::env::temp_dir().join(format!("xila-packet-{}", child.id()));
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("PASS local TCP echo"));
    assert!(
        !directory.exists(),
        "socket directory must be removed on teardown"
    );
    if cfg!(target_os = "linux") {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let helper_pid = stdout
            .lines()
            .find_map(|line| line.strip_prefix("helper_pid="))
            .unwrap();
        assert!(
            !std::path::Path::new(&format!("/proc/{helper_pid}")).exists(),
            "helper must be reaped before exiting"
        );
    }
}
