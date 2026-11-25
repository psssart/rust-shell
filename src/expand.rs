use std::env;
use std::fs;
use std::path::Path;

use crate::parser::Word;

pub fn expand_word(word: &Word) -> Vec<String> {
    if word.quoted {
        return vec![word.text.clone()];
    }
    let text = expand_path(&word.text);
    if text.contains('*') {
        let matches = glob(&text);
        // Like bash, keep the pattern as is if nothing matches
        if !matches.is_empty() {
            return matches;
        }
    }
    vec![text]
}

// `~` -> $HOME, `.` -> current dir
pub fn expand_path(text: &str) -> String {
    let (base, rest) = if let Some(rest) = text.strip_prefix('~') {
        (env::var("HOME").unwrap_or_default(), rest)
    } else if let Some(rest) = text.strip_prefix('.') {
        let cwd = env::current_dir().unwrap_or_default();
        (cwd.display().to_string(), rest)
    } else {
        return text.to_string();
    };
    // Skip things like `~user`, `..` or `.git`
    if rest.is_empty() || rest.starts_with('/') {
        format!("{base}{rest}")
    } else {
        text.to_string()
    }
}

fn glob(pattern: &str) -> Vec<String> {
    let (root, rest) = match pattern.strip_prefix('/') {
        Some(rest) => ("/".to_string(), rest),
        None => (String::new(), pattern),
    };
    let mut paths = vec![root];

    for part in rest.split('/') {
        let mut next = Vec::new();
        for base in &paths {
            if !part.contains('*') {
                next.push(join(base, part));
                continue;
            }
            let dir = if base.is_empty() { "." } else { base };
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Hidden files only match when asked explicitly
                if name.starts_with('.') && !part.starts_with('.') {
                    continue;
                }
                if matches(part, &name) {
                    next.push(join(base, &name));
                }
            }
        }
        paths = next;
    }

    paths.retain(|path| Path::new(path).exists());
    paths.sort();
    paths
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

fn matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if name.len() < first.len() + last.len() || !name.starts_with(first) || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
}
