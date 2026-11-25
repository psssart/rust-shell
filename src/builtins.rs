use std::env;
use std::fs::{self, Metadata};
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;

use crate::shell::Shell;

pub const BUILTINS: [&str; 5] = ["alias", "cd", "exit", "ls", "pwd"];

// io::Error without the " (os error N)" suffix
pub fn error_text(e: &io::Error) -> String {
    let text = e.to_string();
    match text.find(" (os error") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

pub fn run(
    shell: &mut Shell,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<i32> {
    match args[0].as_str() {
        "alias" => alias(shell, &args[1..], out, err),
        "cd" => cd(&args[1..], out, err),
        "exit" => exit(shell, &args[1..], err),
        "ls" => ls(&args[1..], out, err),
        "pwd" => pwd(out),
        _ => unreachable!("not a builtin"),
    }
}

fn cd(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> io::Result<i32> {
    let back = args.first().is_some_and(|arg| arg == "-");
    let target = match args.first().map(String::as_str) {
        None => env::var("HOME").unwrap_or_else(|_| "/".into()),
        Some("-") => match env::var("OLDPWD") {
            Ok(dir) => dir,
            Err(_) => {
                writeln!(err, "cd: OLDPWD not set")?;
                return Ok(1);
            }
        },
        Some(dir) => dir.to_string(),
    };
    let old = env::current_dir().unwrap_or_default();
    if let Err(e) = env::set_current_dir(&target) {
        writeln!(err, "cd: {target}: {}", error_text(&e))?;
        return Ok(1);
    }
    let new = env::current_dir()?;
    // SAFETY: the shell is single-threaded, nobody else touches env meanwhile
    unsafe {
        env::set_var("OLDPWD", old);
        env::set_var("PWD", &new);
    }
    if back {
        writeln!(out, "{}", new.display())?;
    }
    Ok(0)
}

fn pwd(out: &mut dyn Write) -> io::Result<i32> {
    writeln!(out, "{}", env::current_dir()?.display())?;
    Ok(0)
}

fn exit(shell: &mut Shell, args: &[String], err: &mut dyn Write) -> io::Result<i32> {
    let code = match args.first() {
        None => shell.status,
        Some(arg) => match arg.parse() {
            Ok(code) => code,
            Err(_) => {
                writeln!(err, "exit: {arg}: numeric argument required")?;
                2
            }
        },
    };
    shell.exit = Some(code);
    Ok(code)
}

fn alias(
    shell: &mut Shell,
    args: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<i32> {
    if args.is_empty() {
        for (name, value) in &shell.aliases {
            writeln!(out, "alias {name}='{value}'")?;
        }
        return Ok(0);
    }

    let mut status = 0;
    for arg in args {
        match arg.split_once('=') {
            Some((name, value)) if !name.is_empty() => {
                shell.aliases.insert(name.to_string(), value.to_string());
            }
            Some(_) => {
                writeln!(err, "alias: {arg}: invalid alias name")?;
                status = 1;
            }
            None => match shell.aliases.get(arg) {
                Some(value) => writeln!(out, "alias {arg}='{value}'")?,
                None => {
                    writeln!(err, "alias: {arg}: not found")?;
                    status = 1;
                }
            },
        }
    }
    Ok(status)
}

fn ls(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> io::Result<i32> {
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
                            writeln!(err, "ls: invalid option -- '{flag}'")?;
                            return Ok(2);
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
                writeln!(err, "ls: cannot access '{path}': {}", error_text(&e))?;
                status = 2;
                continue;
            }
        };
        if !meta.is_dir() {
            print_entry(out, path, &meta, long)?;
            continue;
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(e) => {
                writeln!(err, "ls: cannot open directory '{path}': {}", error_text(&e))?;
                status = 2;
                continue;
            }
        };

        if paths.len() > 1 {
            let gap = if i > 0 { "\n" } else { "" };
            writeln!(out, "{gap}{path}:")?;
        }
        let mut entries: Vec<_> = entries
            .flatten()
            .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry))
            .filter(|(name, _)| all || !name.starts_with('.'))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        if all {
            for name in [".", ".."] {
                print_entry(out, name, &fs::metadata(format!("{path}/{name}"))?, long)?;
            }
        }
        for (name, entry) in entries {
            print_entry(out, &name, &entry.metadata()?, long)?;
        }
    }
    Ok(status)
}

fn print_entry(out: &mut dyn Write, name: &str, meta: &Metadata, long: bool) -> io::Result<()> {
    if !long {
        return writeln!(out, "{name}");
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
    writeln!(out, "{kind}{perms} {:>8} {name}", meta.len())
}
