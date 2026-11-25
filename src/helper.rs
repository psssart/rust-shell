use std::borrow::Cow::{self, Owned};
use std::env;
use std::fs;
use std::path::Path;

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::HistoryHinter;
use rustyline::{Context, Helper, Hinter, Validator};

use crate::builtins::BUILTINS;
use crate::expand::expand_path;
use crate::parser::{DELIMITERS, OPERATORS};
use crate::shell::{find_command, is_executable};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";
const RESET: &str = "\x1b[0m";

#[derive(Helper, Hinter, Validator)]
pub struct ShellHelper {
    filenames: FilenameCompleter,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
}

impl ShellHelper {
    pub fn new() -> Self {
        Self {
            filenames: FilenameCompleter::new(),
            hinter: HistoryHinter::new(),
        }
    }

    fn is_command(&self, name: &str) -> bool {
        if BUILTINS.contains(&name) {
            return true;
        }
        if name.contains('/') {
            return is_executable(Path::new(&expand_path(name)));
        }
        find_command(name).is_some()
    }

    fn command_names(&self, prefix: &str) -> Vec<String> {
        let mut names: Vec<String> = BUILTINS
            .iter()
            .map(|name| name.to_string())
            .filter(|name| name.starts_with(prefix))
            .collect();
        let paths = env::var_os("PATH").unwrap_or_default();
        for dir in env::split_paths(&paths) {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(prefix) && is_executable(&entry.path()) {
                    names.push(name);
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

// Byte length of the word at the start of `input`, quotes included
fn word_len(input: &str) -> usize {
    let mut quote = None;
    for (i, c) in input.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c.is_whitespace() || DELIMITERS.contains(&c) => return i,
            None => {}
        }
    }
    input.len()
}

impl Completer for ShellHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = line[..pos].rfind([' ', '\t']).map_or(0, |i| i + 1);
        let word = &line[start..pos];

        // Only the first word is a command
        if line[..start].trim().is_empty() && !word.contains('/') {
            let pairs = self
                .command_names(word)
                .into_iter()
                .map(|name| Pair {
                    display: name.clone(),
                    replacement: name,
                })
                .collect();
            return Ok((start, pairs));
        }
        self.filenames.complete(line, pos, ctx)
    }
}

impl Highlighter for ShellHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let mut result = String::with_capacity(line.len() * 2);
        let mut command_start = true;
        let mut rest = line;

        while let Some(c) = rest.chars().next() {
            if c.is_whitespace() {
                result.push(c);
                rest = &rest[c.len_utf8()..];
                continue;
            }
            let (len, color) =
                if let Some((op, _)) = OPERATORS.iter().find(|(op, _)| rest.starts_with(op)) {
                    command_start = false;
                    (op.len(), CYAN)
                } else {
                    match word_len(rest) {
                        // Lone `&`, not supported yet
                        0 => (c.len_utf8(), RED),
                        len => {
                            let word = &rest[..len];
                            let color = if command_start {
                                if self.is_command(&word.replace(['\'', '"'], "")) {
                                    GREEN
                                } else {
                                    RED
                                }
                            } else if word.contains(['\'', '"']) {
                                YELLOW
                            } else {
                                ""
                            };
                            command_start = false;
                            (len, color)
                        }
                    }
                };

            if color.is_empty() {
                result.push_str(&rest[..len]);
            } else {
                result.push_str(&format!("{color}{}{RESET}", &rest[..len]));
            }
            rest = &rest[len..];
        }
        Owned(result)
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned(format!("{GRAY}{hint}{RESET}"))
    }

    fn highlight_char(&self, _line: &str, _pos: usize, kind: CmdKind) -> bool {
        kind != CmdKind::MoveCursor
    }
}
