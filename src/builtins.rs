use std::env;
use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::PermissionsExt;

use crate::shell::Shell;

pub const BUILTINS: [&str; 3] = ["cd", "exit", "ls"];

// io::Error without the " (os error N)" suffix
pub fn error_text(e: &io::Error) -> String {
    let text = e.to_string();
    match text.find(" (os error") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

pub fn run(shell: &mut Shell, args: &[String]) -> i32 {
    match args[0].as_str() {
        "cd" => cd(&args[1..]),
        "exit" => exit(shell, &args[1..]),
        "ls" => ls(&args[1..]),
        _ => unreachable!("not a builtin"),
    }
}

fn cd(args: &[String]) -> i32 {
    let back = args.first().is_some_and(|arg| arg == "-");
    let target = match args.first().map(String::as_str) {
        None => env::var("HOME").unwrap_or_else(|_| "/".into()),
        Some("-") => match env::var("OLDPWD") {
            Ok(dir) => dir,
            Err(_) => {
                eprintln!("cd: OLDPWD not set");
                return 1;
            }
        },
        Some(dir) => dir.to_string(),
    };
    let old = env::current_dir().unwrap_or_default();
    if let Err(e) = env::set_current_dir(&target) {
        eprintln!("cd: {target}: {}", error_text(&e));
        return 1;
    }
    let new = env::current_dir().unwrap_or_default();
    // SAFETY: the shell is single-threaded, nobody else touches env meanwhile
    unsafe {
        env::set_var("OLDPWD", old);
        env::set_var("PWD", &new);
    }
    if back {
        println!("{}", new.display());
    }
    0
}

fn exit(shell: &mut Shell, args: &[String]) -> i32 {
    let code = match args.first() {
        None => shell.status,
        Some(arg) => match arg.parse() {
            Ok(code) => code,
            Err(_) => {
                eprintln!("exit: {arg}: numeric argument required");
                2
            }
        },
    };
    shell.exit = Some(code);
    code
}

fn ls(args: &[String]) -> i32 {
    let mut all = false;
    let mut long = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.strip_prefix('-') {
            Some(flags) if !flags.is_empty() => {
                for flag in flags.chars() {
                    match flag {
                        'a' => all = true,
                        'l' => long = true,
                        _ => {
                            eprintln!("ls: invalid option -- '{flag}'");
                            return 2;
                        }
                    }
                }
            }
            _ => paths.push(arg.as_str()),
        }
    }
    if paths.is_empty() {
        paths.push(".");
    }

    let mut status = 0;
    for (i, path) in paths.iter().enumerate() {
        let meta = match fs::metadata(path) {
            Ok(meta) => meta,
            Err(e) => {
                eprintln!("ls: cannot access '{path}': {}", error_text(&e));
                status = 2;
                continue;
            }
        };
        if !meta.is_dir() {
            print_entry(path, &meta, long);
            continue;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("ls: cannot open directory '{path}': {}", error_text(&e));
                status = 2;
                continue;
            }
        };

        if paths.len() > 1 {
            let gap = if i > 0 { "\n" } else { "" };
            println!("{gap}{path}:");
        }
        let mut entries: Vec<_> = entries
            .flatten()
            .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry))
            .filter(|(name, _)| all || !name.starts_with('.'))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in entries {
            if let Ok(meta) = entry.metadata() {
                print_entry(&name, &meta, long);
            }
        }
    }
    status
}

fn print_entry(name: &str, meta: &Metadata, long: bool) {
    if !long {
        println!("{name}");
        return;
    }
    let kind = if meta.is_dir() {
        'd'
    } else if meta.is_symlink() {
        'l'
    } else {
        '-'
    };
    let mode = meta.permissions().mode();
    let perms: String = (0..9)
        .map(|i| if mode & (1 << (8 - i)) != 0 { b"rwx"[i % 3] as char } else { '-' })
        .collect();
    println!("{kind}{perms} {:>8} {name}", meta.len());
}
