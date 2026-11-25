use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process;
use std::{env, fs, io};

use crate::builtins::{self, BUILTINS, error_text};
use crate::expand::expand_word;
use crate::parser;

#[derive(Default)]
pub struct Shell {
    // Exit status of the last command
    pub status: i32,
    // Set by `exit`
    pub exit: Option<i32>,
}

impl Shell {
    pub fn run_line(&mut self, line: &str) {
        let words = match parser::tokenize(line) {
            Ok(words) => words,
            Err(e) => {
                eprintln!("rust-shell: {e}");
                self.status = 2;
                return;
            }
        };
        let args: Vec<String> = words.iter().flat_map(expand_word).collect();
        let Some(name) = args.first() else {
            return;
        };
        self.status = if BUILTINS.contains(&name.as_str()) {
            builtins::run(self, &args)
        } else {
            run_external(&args)
        };
    }
}

pub fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

pub fn find_command(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return Some(PathBuf::from(name));
    }
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| is_executable(path))
}

fn run_external(args: &[String]) -> i32 {
    let name = &args[0];
    let Some(path) = find_command(name) else {
        eprintln!("rust-shell: {name}: command not found");
        return 127;
    };
    let child = process::Command::new(path).arg0(name).args(&args[1..]).spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            eprintln!("rust-shell: {name}: {}", error_text(&e));
            return if e.kind() == io::ErrorKind::NotFound { 127 } else { 126 };
        }
    };
    match child.wait() {
        // Killed by a signal, e.g. Ctrl+C gives 130
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(e) => {
            eprintln!("rust-shell: {e}");
            1
        }
    }
}
