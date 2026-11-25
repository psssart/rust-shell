use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{self, Child};
use std::{env, fs};

use crate::builtins::{self, BUILTINS, error_text};
use crate::expand::{expand_path, expand_word};
use crate::parser::{self, Command, Op, Redirect, RedirectKind};

#[derive(Default)]
pub struct Shell {
    pub aliases: BTreeMap<String, String>,
    // Exit status of the last command
    pub status: i32,
    // Set by `exit`
    pub exit: Option<i32>,
}

// Single command of a pipeline, `None` means inherit from the shell
struct Stage {
    args: Vec<String>,
    stdin: Option<OwnedFd>,
    stdout: Option<OwnedFd>,
    stderr: Option<OwnedFd>,
}

impl Shell {
    pub fn run_line(&mut self, line: &str) {
        let chain = parser::tokenize(line)
            .and_then(|tokens| parser::expand_aliases(tokens, &self.aliases))
            .and_then(parser::parse);
        let chain = match chain {
            Ok(chain) => chain,
            Err(e) => {
                eprintln!("rust-shell: {e}");
                self.status = 2;
                return;
            }
        };

        for (op, pipeline) in chain {
            let skip = match op {
                Op::Always => false,
                Op::And => self.status != 0,
                Op::Or => self.status == 0,
            };
            if skip {
                continue;
            }
            self.status = self.run_pipeline(&pipeline);
            if self.exit.is_some() {
                break;
            }
        }
    }

    fn run_pipeline(&mut self, pipeline: &[Command]) -> i32 {
        let stages = match prepare_stages(pipeline) {
            Ok(stages) => stages,
            Err(e) => {
                eprintln!("rust-shell: {e}");
                return 1;
            }
        };

        let mut statuses = vec![0; stages.len()];
        let mut children = Vec::new();
        let mut builtin_stages = Vec::new();
        // Spawn external commands first, so a builtin never blocks on a full pipe
        for (i, mut stage) in stages.into_iter().enumerate() {
            match stage.args.first() {
                None => {}
                Some(name) if BUILTINS.contains(&name.as_str()) => {
                    // Builtins don't read stdin
                    stage.stdin = None;
                    builtin_stages.push((i, stage));
                }
                Some(_) => match spawn(stage) {
                    Ok(child) => children.push((i, child)),
                    Err(status) => statuses[i] = status,
                },
            }
        }

        for (i, stage) in builtin_stages {
            statuses[i] = self.run_builtin(stage);
        }

        for (i, mut child) in children {
            statuses[i] = match child.wait() {
                Ok(status) => status
                    .code()
                    .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
                Err(e) => {
                    eprintln!("rust-shell: {e}");
                    1
                }
            };
        }
        statuses.last().copied().unwrap_or(0)
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
        match result {
            Ok(status) => status,
            // Reader went away, e.g. `ls | head -1`
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 1,
            Err(e) => {
                eprintln!("rust-shell: {}: {e}", stage.args[0]);
                1
            }
        }
    }
}

fn prepare_stages(pipeline: &[Command]) -> Result<Vec<Stage>, String> {
    let mut stages = Vec::new();
    let mut prev_reader: Option<OwnedFd> = None;
    for (i, command) in pipeline.iter().enumerate() {
        let mut stage = Stage {
            args: command.args.iter().flat_map(expand_word).collect(),
            stdin: prev_reader.take(),
            stdout: None,
            stderr: None,
        };
        if i + 1 < pipeline.len() {
            let (reader, writer) = io::pipe().map_err(|e| e.to_string())?;
            stage.stdout = Some(writer.into());
            prev_reader = Some(reader.into());
        }
        apply_redirects(&mut stage, &command.redirects)?;
        stages.push(stage);
    }
    Ok(stages)
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

fn spawn(stage: Stage) -> Result<Child, i32> {
    let name = &stage.args[0];
    let Some(path) = find_command(name) else {
        eprintln!("rust-shell: {name}: command not found");
        return Err(127);
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
    command.spawn().map_err(|e| {
        let code = if e.kind() == io::ErrorKind::NotFound { 127 } else { 126 };
        eprintln!("rust-shell: {name}: {}", error_text(&e));
        code
    })
}
