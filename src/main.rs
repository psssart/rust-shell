mod builtins;
mod expand;
mod parser;
mod shell;

use std::env;
use std::io::{self, Write};
use std::process;

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
    let mut line = String::new();

    while shell.exit.is_none() {
        print!("{}", prompt());
        io::stdout().flush().unwrap();

        line.clear();
        match io::stdin().read_line(&mut line) {
            // Ctrl+D
            Ok(0) => break,
            Ok(_) => shell.run_line(&line),
            Err(e) => {
                eprintln!("rust-shell: {e}");
                break;
            }
        }
    }
    process::exit(shell.exit.unwrap_or(shell.status));
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
