//! ECHO, SET, PATH, PROMPT, REM, PAUSE, HELP.

use super::{Builtin, Pager};

pub const ECHO: Builtin = Builtin {
    names: &["ECHO"],
    summary: "Displays messages, or turns command-echoing on or off.",
    usage: "  ECHO [ON | OFF]\n  ECHO [message]\n\nType ECHO without parameters to display the current echo setting.\nType ECHO. to display an empty line.",
    run: |sh, a, io| {
        let raw = a.raw.trim_end_matches(['\r', '\n']);
        if a.glued && raw.starts_with(['.', ',', ';', '=', '[', ']', '+', '/']) && raw != "/?" {
            // "ECHO." prints an empty line, "ECHO.text" prints "text".
            let _ = writeln!(io.out, "{}", raw[1..].trim_end());
            return 0;
        }
        match raw.trim().to_ascii_uppercase().as_str() {
            "" => {
                let _ = writeln!(io.out, "ECHO is {}.", if sh.echo { "on" } else { "off" });
            }
            "ON" => sh.echo = true,
            "OFF" => sh.echo = false,
            "/?" => {
                let _ = writeln!(io.out, "{}\n\n{}", ECHO.summary, ECHO.usage);
            }
            _ => {
                let _ = writeln!(io.out, "{}", raw.trim_end());
            }
        }
        0
    },
};

pub const SET: Builtin = Builtin {
    names: &["SET"],
    summary: "Displays, sets, or removes environment variables.",
    usage: "SET [variable=[string]]\n\n  variable  Specifies the environment-variable name.\n  string    Specifies a series of characters to assign to the variable.\n\nType SET without parameters to display the current environment variables.\nSET LFN=OFF shows short 8.3 file names; SET LFN=ON shows long names.",
    run: |sh, a, io| {
        let raw = a.raw.trim();
        if raw.is_empty() {
            let mut vars: Vec<(String, String)> = std::env::vars().collect();
            vars.sort_by_key(|(k, _)| k.to_ascii_uppercase());
            let mut pager = Pager::new(false);
            for (k, v) in vars {
                let _ = writeln!(io.out, "{k}={v}");
                if !pager.line(io) {
                    break;
                }
            }
            return 0;
        }
        let Some((name, value)) = raw.split_once('=') else {
            // NT extension: "SET PA" lists variables starting with PA.
            let pre = raw.to_ascii_uppercase();
            let mut found = false;
            let mut vars: Vec<(String, String)> = std::env::vars().collect();
            vars.sort();
            for (k, v) in vars {
                if k.to_ascii_uppercase().starts_with(&pre) {
                    let _ = writeln!(io.out, "{k}={v}");
                    found = true;
                }
            }
            if !found {
                let _ = writeln!(io.out, "Environment variable {raw} not defined");
                return 1;
            }
            return 0;
        };
        let name = name.trim();
        if name.is_empty() {
            let _ = writeln!(io.out, "Syntax error");
            return 1;
        }
        if name.eq_ignore_ascii_case("LFN") {
            sh.dos.lfn = !value.trim().eq_ignore_ascii_case("OFF");
        }
        sh.setenv(name, if value.is_empty() { None } else { Some(value) });
        0
    },
};

pub const PATH: Builtin = Builtin {
    names: &["PATH"],
    summary: "Displays or sets a search path for executable files.",
    usage: "PATH [[drive:]path[;...]]\nPATH ;\n\nType PATH ; to clear all search-path settings.\nType PATH without parameters to display the current path.",
    run: |sh, a, io| {
        let raw = a.raw.trim();
        if raw.is_empty() {
            let p = sh.getenv("PATH").unwrap_or_default();
            let shown: Vec<String> =
                p.split(':').filter(|s| !s.is_empty()).map(|d| sh.dos.display(std::path::Path::new(d))).collect();
            if shown.is_empty() {
                let _ = writeln!(io.out, "No Path");
            } else {
                let _ = writeln!(io.out, "PATH={}", shown.join(";"));
            }
            return 0;
        }
        if raw == ";" {
            sh.setenv("PATH", Some(""));
            return 0;
        }
        let raw = raw.strip_prefix('=').unwrap_or(raw).trim();
        let mut dirs = Vec::new();
        for part in raw.split(';').filter(|s| !s.trim().is_empty()) {
            match sh.dos.resolve(part.trim()) {
                Ok(p) => dirs.push(p.to_string_lossy().into_owned()),
                Err(e) => {
                    let _ = writeln!(io.out, "{e}: {part}");
                    return 1;
                }
            }
        }
        sh.setenv("PATH", Some(&dirs.join(":")));
        0
    },
};

pub const PROMPT: Builtin = Builtin {
    names: &["PROMPT"],
    summary: "Changes the RetroDOS command prompt.",
    usage: "PROMPT [text]\n\n  text    Specifies a new command prompt.\n\nPrompt can be made up of normal characters and the following special codes:\n\n  $Q   = (equal sign)            $D   Current date\n  $$   $ (dollar sign)           $P   Current drive and path\n  $T   Current time              $V   RetroDOS version number\n  $N   Current drive             $G   > (greater-than sign)\n  $L   < (less-than sign)        $B   | (pipe)\n  $H   Backspace                 $E   Escape code (ASCII code 27)\n  $_   Carriage return and linefeed\n\nType PROMPT without parameters to reset the prompt to $P$G.",
    run: |sh, a, _io| {
        let t = a.raw.trim();
        sh.setenv("PROMPT", Some(if t.is_empty() { "$P$G" } else { t }));
        0
    },
};

pub const REM: Builtin = Builtin {
    names: &["REM"],
    summary: "Records comments (remarks) in a batch file or AUTOEXEC.BAT.",
    usage: "REM [comment]",
    run: |_, _, _| 0,
};

pub const PAUSE: Builtin = Builtin {
    names: &["PAUSE"],
    summary: "Suspends processing of a batch program and displays the message\n\"Press any key to continue . . .\"",
    usage: "PAUSE",
    run: |_, _, io| {
        let _ = io.out.flush();
        if super::wait_any_key("Press any key to continue . . .") {
            0
        } else {
            1
        }
    },
};

pub const HELP: Builtin = Builtin {
    names: &["HELP"],
    summary: "Provides Help information for RetroDOS commands.",
    usage: "HELP [command]\n\n  command  Displays help information on that command.",
    run: |_, a, io| {
        if let Some(name) = a.words.first() {
            return match super::find(name) {
                Some(b) => {
                    let _ = writeln!(io.out, "{}\n\n{}", b.summary, b.usage.trim_end());
                    0
                }
                None => {
                    let _ = writeln!(
                        io.out,
                        "This command is not supported by the help utility.  Try \"{name} /?\"."
                    );
                    1
                }
            };
        }
        let _ = writeln!(
            io.out,
            "For more information on a specific command, type HELP command-name\n"
        );
        let mut pager = Pager::new(true);
        let mut list: Vec<&Builtin> = super::all().iter().collect();
        list.sort_by_key(|b| b.names[0]);
        for b in list {
            let first = b.summary.lines().next().unwrap_or("");
            let _ = writeln!(io.out, "{:<10} {first}", b.names[0]);
            if !pager.line(io) {
                break;
            }
        }
        let _ = writeln!(
            io.out,
            "\nAny program on the PATH can be run by typing its name. UNIX opens a POSIX shell."
        );
        0
    },
};
