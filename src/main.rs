mod builtins;
mod expand;
mod helper;
mod parser;
mod shell;

use std::env;
use std::io::{self, BufRead, IsTerminal};
use std::process;

use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, Editor};

use helper::ShellHelper;
use shell::Shell;

unsafe extern "C" {
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
}

const SIGINT: i32 = 2;

extern "C" fn on_sigint(_: i32) {}

fn main() {
    // Ctrl+C should stop the running command, not the shell.
    // Unlike ignoring it, a handler is reset back to default in children.
    unsafe {
        signal(SIGINT, on_sigint);
    }

    let mut shell = Shell::default();
    if io::stdin().is_terminal() {
        if let Err(e) = interactive(&mut shell) {
            eprintln!("rust-shell: {e}");
        }
    } else {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            shell.run_line(&line);
            if shell.exit.is_some() {
                break;
            }
        }
    }
    process::exit(shell.exit.unwrap_or(shell.status));
}

fn interactive(shell: &mut Shell) -> rustyline::Result<()> {
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .history_ignore_dups(true)?
        .history_ignore_space(true)
        .build();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(ShellHelper::new()));

    let history = format!("{}/.rust_shell_history", env::var("HOME").unwrap_or_default());
    // Fails on first run, when there is no history yet
    let _ = rl.load_history(&history);

    while shell.exit.is_none() {
        let prompt = prompt();
        let colored = format!("\x1b[1;34m{prompt}\x1b[0m");
        match rl.readline(&(prompt, colored)) {
            Ok(line) => {
                if !line.trim().is_empty() {
                    rl.add_history_entry(line.as_str())?;
                }
                shell.run_line(&line);
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => return Err(e),
        }
    }
    rl.save_history(&history)
}

fn prompt() -> String {
    let cwd = env::current_dir()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default();
    let home = env::var("HOME").unwrap_or_default();
    match cwd.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest} $ ")
        }
        _ => format!("{cwd} $ "),
    }
}
