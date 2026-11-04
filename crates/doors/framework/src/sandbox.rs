//! The sandbox: what "a door has no filesystem or network access" means here, and how it is
//! checked.
//!
//! **How a door is kept away from the filesystem and the network.** A door is a library called
//! in-process (D98). It is handed only the types in this crate: keys, a [`Canvas`](crate::Canvas),
//! a [`Ctx`](crate::Ctx) (clock, seeded RNG, a *request* to save), a [`DoorEnv`](crate::DoorEnv)
//! and the bytes of its own save and world. None of them holds a path, a socket, a store, a
//! thread or a task, and every write goes through the board's save API, which caps sizes and
//! checks revisions. A door crate also depends on nothing but this crate.
//!
//! **What that does not guarantee.** Rust gives no in-process isolation: code a door author
//! wrote could still call `std::fs` or open a socket if it were allowed to compile. So the rule
//! is enforced by review and by [`audit_crate`], a test every door crate runs on itself, which
//! fails if its `Cargo.toml` names any dependency but the framework (and the allowed pure
//! helpers) or its source mentions an API that reaches outside: the filesystem, the network,
//! processes, the environment, threads and tasks, wall-clock time, operating-system
//! randomness, printing, `unsafe`, or FFI. That is a tripwire for first-party doors reviewed in
//! the tree, **not isolation against hostile code**. Running doors written by people the sysop
//! does not trust needs a process boundary (a separate process with its own filesystem and
//! network restrictions, speaking the `Door` trait over a pipe). That is a recorded decision
//! (D98) and is not built.

use std::fs;
use std::path::{Path, PathBuf};

/// Dependencies a door crate may have besides the framework itself.
pub const ALLOWED_DEPENDENCIES: &[&str] = &["russh-bbs-door", "serde", "toml"];

/// Source patterns a door crate may not contain. Matched after comments are removed.
pub const FORBIDDEN: &[(&str, &str)] = &[
    ("std::fs", "filesystem"),
    ("std::path", "filesystem paths"),
    ("std::net", "network"),
    ("std::process", "child processes"),
    ("std::env", "the process environment"),
    ("std::thread", "threads"),
    ("std::os", "operating-system APIs"),
    ("std::io::stdin", "standard input"),
    ("std::io::stdout", "standard output"),
    ("std::io::stderr", "standard error"),
    ("std::time::SystemTime", "the wall clock (use `Ctx::now`)"),
    ("SystemTime", "the wall clock (use `Ctx::now`)"),
    ("Instant::now", "the monotonic clock (use `Ctx::now`)"),
    ("tokio", "async tasks and I/O"),
    ("hyper", "network"),
    ("reqwest", "network"),
    ("libc", "FFI"),
    ("extern \"C\"", "FFI"),
    ("unsafe", "unsafe code"),
    ("thread_rng", "operating-system randomness (use `Ctx::rng`)"),
    ("getrandom", "operating-system randomness (use `Ctx::rng`)"),
    (
        "rand::",
        "randomness other than the seeded one (use `Ctx::rng`)",
    ),
    ("println!", "printing"),
    ("eprintln!", "printing"),
    ("print!", "printing"),
    ("eprint!", "printing"),
    ("dbg!", "printing"),
    (
        "include_bytes!(",
        "reading a file path other than an embedded table",
    ),
];

/// One thing the audit found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The file, relative to the crate.
    pub file: PathBuf,
    /// The line number (1-based; 0 for the manifest).
    pub line: usize,
    /// What was found and why it is not allowed.
    pub what: String,
}

fn strip_comments(line: &str) -> &str {
    match line.find("//") {
        Some(at) => &line[..at],
        None => line,
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The dependency names in a manifest's `[dependencies]` (and build/dev sections that would
/// also compile code into the crate).
fn manifest_dependencies(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]"
                || line == "[build-dependencies]"
                || line.starts_with("[dependencies.")
                || line.starts_with("[build-dependencies.");
            if let Some(name) = line
                .strip_prefix("[dependencies.")
                .or_else(|| line.strip_prefix("[build-dependencies."))
            {
                names.push(name.trim_end_matches(']').to_owned());
            }
            continue;
        }
        if in_deps && let Some((name, _)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && !name.starts_with('#') {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// Audit the door crate at `crate_dir` (pass `env!("CARGO_MANIFEST_DIR")`): its dependencies
/// must be in [`ALLOWED_DEPENDENCIES`], it must forbid `unsafe` at the crate root, and no source
/// line may contain a [`FORBIDDEN`] pattern. A door crate's test is
/// `assert_eq!(sandbox::audit_crate(env!("CARGO_MANIFEST_DIR")), Ok(()))`.
///
/// # Errors
/// Every [`Finding`], so one run shows all of them.
pub fn audit_crate(crate_dir: &str) -> Result<(), Vec<Finding>> {
    let root = Path::new(crate_dir);
    let mut findings = Vec::new();
    let manifest = root.join("Cargo.toml");
    match fs::read_to_string(&manifest) {
        Ok(text) => {
            for name in manifest_dependencies(&text) {
                if !ALLOWED_DEPENDENCIES.contains(&name.as_str()) {
                    findings.push(Finding {
                        file: PathBuf::from("Cargo.toml"),
                        line: 0,
                        what: format!("dependency `{name}` is not allowed in a door crate"),
                    });
                }
            }
            if !text.contains("[lints]") || !text.contains("workspace = true") {
                findings.push(Finding {
                    file: PathBuf::from("Cargo.toml"),
                    line: 0,
                    what: "the crate must inherit the workspace lints (`[lints] workspace = true`)"
                        .into(),
                });
            }
        }
        Err(_) => findings.push(Finding {
            file: PathBuf::from("Cargo.toml"),
            line: 0,
            what: "Cargo.toml could not be read".into(),
        }),
    }
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.sort();
    let lib = root.join("src").join("lib.rs");
    if let Ok(text) = fs::read_to_string(&lib)
        && !text.contains("#![forbid(unsafe_code)]")
    {
        findings.push(Finding {
            file: PathBuf::from("src/lib.rs"),
            line: 1,
            what: "src/lib.rs must start with `#![forbid(unsafe_code)]`".into(),
        });
    }
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let relative = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
        for (n, line) in text.lines().enumerate() {
            let code = strip_comments(line);
            for (pattern, why) in FORBIDDEN {
                // `unsafe` is also matched inside `#![forbid(unsafe_code)]`; that attribute is
                // the one required use.
                if code.contains(pattern) && !code.contains("forbid(unsafe_code)") {
                    findings.push(Finding {
                        file: relative.clone(),
                        line: n + 1,
                        what: format!("`{pattern}`: {why}"),
                    });
                }
            }
        }
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(findings)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn crate_with(manifest: &str, lib: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        fs::write(dir.path().join("src/lib.rs"), lib).unwrap();
        dir
    }

    const GOOD_MANIFEST: &str = "[package]\nname='x'\n[lints]\nworkspace = true\n[dependencies]\nrussh-bbs-door = { workspace = true }\n";

    #[test]
    fn a_clean_door_passes() {
        let dir = crate_with(
            GOOD_MANIFEST,
            "#![forbid(unsafe_code)]\n//! std::fs in a comment is fine\nfn f() {}\n",
        );
        assert_eq!(audit_crate(dir.path().to_str().unwrap()), Ok(()));
    }

    #[test]
    fn every_way_out_is_caught() {
        let source = "#![forbid(unsafe_code)]\n\
            use std::fs;\nuse std::net::TcpStream;\nfn a() { std::process::Command::new(\"x\"); }\n\
            fn b() { let _ = std::env::var(\"HOME\"); }\nfn c() { let _ = std::time::SystemTime::now(); }\n\
            fn d() { println!(\"x\"); }\nfn e() { unsafe {} }\nfn f() { rand::random::<u8>(); }\n\
            fn g() { std::thread::spawn(|| ()); }\n";
        let dir = crate_with(
            "[package]\nname='x'\n[lints]\nworkspace = true\n[dependencies]\nrussh-bbs-door = { workspace = true }\ntokio = { workspace = true }\nreqwest = '1'\n",
            source,
        );
        let findings = audit_crate(dir.path().to_str().unwrap()).unwrap_err();
        let text = findings
            .iter()
            .map(|f| f.what.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "dependency `tokio`",
            "dependency `reqwest`",
            "`std::fs`",
            "`std::net`",
            "`std::process`",
            "`std::env`",
            "`SystemTime`",
            "`println!`",
            "`unsafe`",
            "`rand::`",
            "`std::thread`",
        ] {
            assert!(text.contains(needle), "{needle} not found in:\n{text}");
        }
    }

    #[test]
    fn a_missing_unsafe_forbid_or_lint_inheritance_is_a_finding() {
        let dir = crate_with(
            "[package]\nname='x'\n[dependencies]\nrussh-bbs-door = { workspace = true }\n",
            "fn f() {}\n",
        );
        let findings = audit_crate(dir.path().to_str().unwrap()).unwrap_err();
        assert!(
            findings
                .iter()
                .any(|f| f.what.contains("forbid(unsafe_code)"))
        );
        assert!(findings.iter().any(|f| f.what.contains("workspace lints")));
    }

    #[test]
    fn the_framework_itself_is_not_held_to_the_door_rules_but_forbids_unsafe() {
        let lib =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
        assert!(lib.contains("#![forbid(unsafe_code)]"));
    }
}
