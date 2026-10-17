//! M2 commands through the real binary.

use std::path::PathBuf;
use std::process::Command;

fn home(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("retrodos-cmd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(home: &PathBuf, line: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_retrodos"))
        .args(["/C", line])
        .env("HOME", home)
        .env_remove("RETROOS_CONSOLE")
        .current_dir(home)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

#[test]
fn xcopy_tree_and_attrib() {
    let h = home("xcopy");
    std::fs::create_dir_all(h.join("SRC/SUB")).unwrap();
    std::fs::write(h.join("SRC/A.TXT"), "a").unwrap();
    std::fs::write(h.join("SRC/SUB/B.TXT"), "b").unwrap();
    let out = run(&h, "XCOPY SRC DEST /S /Q");
    assert!(out.contains("2 File(s) copied"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.join("DEST/SUB/B.TXT")).unwrap(),
        "b"
    );
    run(&h, "ATTRIB +R DEST\\A.TXT");
    let out = run(&h, "ATTRIB DEST\\A.TXT");
    assert!(out.trim_start().starts_with('R'), "{out}");
    let out = run(&h, "ATTRIB -R DEST\\A.TXT & ATTRIB DEST\\A.TXT");
    assert!(!out.trim_start().starts_with('R'), "{out}");
}

#[test]
fn mode_and_chkdsk_report() {
    let h = home("mode");
    let out = run(&h, "MODE CON");
    assert!(
        out.contains("Status for device CON") && out.contains("Columns="),
        "{out}"
    );
    let out = run(&h, "MODE CON COLS=80 LINES=25");
    assert!(
        out.contains("only change the size of the RetroOS console"),
        "{out}"
    );
    let out = run(&h, "CHKDSK C:");
    assert!(
        out.contains("bytes total disk space") && out.contains("bytes available on disk"),
        "{out}"
    );
}
