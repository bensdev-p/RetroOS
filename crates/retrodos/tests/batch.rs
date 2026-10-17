//! Batch files, run through the real binary.

use std::path::PathBuf;
use std::process::Command;

fn home(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("retrodos-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(home: &PathBuf, line: &str) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_retrodos"))
        .args(["/C", line])
        .env("HOME", home)
        .current_dir(home)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn parameters_labels_and_loops() {
    let h = home("params");
    std::fs::write(h.join("A.TXT"), "").unwrap();
    std::fs::write(h.join("B.TXT"), "").unwrap();
    std::fs::write(
        h.join("TEST.BAT"),
        "@ECHO OFF\r\nECHO all=%*\r\n:LOOP\r\nIF \"%1\"==\"\" GOTO DONE\r\nECHO arg %1\r\nSHIFT\r\ngoto loop\r\n:DONE\r\nFOR %%F IN (*.TXT) DO ECHO file %%F\r\nGOTO :EOF\r\nECHO never\r\n",
    )
    .unwrap();
    let (out, _) = run(&h, "test one two");
    assert_eq!(
        out, "all=one two\narg one\narg two\nfile A.TXT\nfile B.TXT\n",
        "{out}"
    );
}

#[test]
fn call_exit_b_and_errorlevel() {
    let h = home("call");
    std::fs::write(h.join("SUB.BAT"), "@ECHO in sub %1\r\n@EXIT /B 3\r\n").unwrap();
    std::fs::write(
        h.join("MAIN.BAT"),
        "@ECHO OFF\r\nCALL SUB x\r\nECHO back\r\nIF ERRORLEVEL 3 ECHO level3\r\nIF NOT ERRORLEVEL 4 ECHO below4\r\nIF NOT EXIST NOPE.TXT ECHO missing\r\nIF /I abc==ABC ECHO nocase\r\n",
    )
    .unwrap();
    let (out, code) = run(&h, "MAIN");
    assert_eq!(
        out, "in sub x\nback\nlevel3\nbelow4\nmissing\nnocase\n",
        "{out}"
    );
    // ECHO and IF do not reset ERRORLEVEL, like COMMAND.COM.
    assert_eq!(code, 3);
}

#[test]
fn choice_from_pipe() {
    let h = home("choice");
    let (out, code) = run(&h, "ECHO n| CHOICE /C:YNC Continue");
    assert_eq!(out, "Continue[Y,N,C]?N\n");
    assert_eq!(code, 2);
}

#[test]
fn echo_on_shows_commands() {
    let h = home("echo");
    std::fs::write(h.join("E.BAT"), "ECHO hi\r\n@ECHO quiet\r\n").unwrap();
    let (out, _) = run(&h, "E");
    assert!(out.contains(">ECHO hi\nhi\nquiet\n"), "{out}");
}
