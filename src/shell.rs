use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process;
use std::{env, fs};

use crate::builtins::{self, BUILTINS, error_text};
use crate::expand::{expand_path, expand_word};
use crate::parser::{self, Redirect, RedirectKind};

#[derive(Default)]
pub struct Shell {
    // Exit status of the last command
    pub status: i32,
    // Set by `exit`
    pub exit: Option<i32>,
}

// Command ready to run, `None` means inherit from the shell
struct Stage {
    args: Vec<String>,
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
    stderr: Option<OwnedFd>,
}

impl Shell {
    pub fn run_line(&mut self, line: &str) {
        let command = match parser::tokenize(line).and_then(parser::parse) {
            Ok(command) => command,
            Err(e) => {
                eprintln!("rust-shell: {e}");
                self.status = 2;
                return;
            }
        };
        let mut stage = Stage {
            args: command.args.iter().flat_map(expand_word).collect(),
            stdin: None,
            stdout: None,
            stderr: None,
        };
        if let Err(e) = apply_redirects(&mut stage, &command.redirects) {
            eprintln!("rust-shell: {e}");
            self.status = 1;
            return;
        }
        self.status = match stage.args.first() {
            // Only redirects, like `> file`
            None => 0,
            Some(name) if BUILTINS.contains(&name.as_str()) => self.run_builtin(stage),
            Some(_) => run_external(stage),
        };
    }

    fn run_builtin(&mut self, stage: Stage) -> i32 {
        let mut out: Box<dyn Write> = match stage.stdout {
            Some(fd) => Box::new(File::from(fd)),
            None => Box::new(io::stdout()),
        };
        let mut err: Box<dyn Write> = match stage.stderr {
            Some(fd) => Box::new(File::from(fd)),
            None => Box::new(io::stderr()),
        };
        let result = builtins::run(self, &stage.args, &mut *out, &mut *err).and_then(|status| {
            out.flush()?;
            Ok(status)
        });
        result.unwrap_or_else(|e| {
            eprintln!("rust-shell: {}: {e}", stage.args[0]);
            1
        })
    }
}

fn apply_redirects(stage: &mut Stage, redirects: &[Redirect]) -> Result<(), String> {
    for redirect in redirects {
        match redirect {
            Redirect::File(kind, target) => {
                let path = if target.quoted {
                    target.text.clone()
                } else {
                    expand_path(&target.text)
                };
                let file = open(*kind, &path).map_err(|e| format!("{path}: {}", error_text(&e)))?;
                match kind {
                    RedirectKind::In => stage.stdin = Some(file.into()),
                    RedirectKind::Out | RedirectKind::Append => stage.stdout = Some(file.into()),
                    RedirectKind::Err | RedirectKind::ErrAppend => stage.stderr = Some(file.into()),
                    RedirectKind::Both => {
                        let copy = file.try_clone().map_err(|e| e.to_string())?;
                        stage.stdout = Some(file.into());
                        stage.stderr = Some(copy.into());
                    }
                }
            }
            Redirect::ErrToOut => {
                let out = match &stage.stdout {
                    Some(fd) => fd.try_clone(),
                    None => io::stdout().as_fd().try_clone_to_owned(),
                };
                stage.stderr = Some(out.map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(())
}

fn open(kind: RedirectKind, path: &str) -> io::Result<File> {
    match kind {
        RedirectKind::In => File::open(path),
        RedirectKind::Append | RedirectKind::ErrAppend => {
            OpenOptions::new().append(true).create(true).open(path)
        }
        _ => File::create(path),
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

fn run_external(stage: Stage) -> i32 {
    let name = &stage.args[0];
    let Some(path) = find_command(name) else {
        eprintln!("rust-shell: {name}: command not found");
        return 127;
    };
    let mut command = process::Command::new(path);
    command.arg0(name);
    command.args(&stage.args[1..]);
    if let Some(fd) = stage.stdin {
        command.stdin(fd);
    }
    if let Some(fd) = stage.stdout {
        command.stdout(fd);
    }
    if let Some(fd) = stage.stderr {
        command.stderr(fd);
    }
    let mut child = match command.spawn() {
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
