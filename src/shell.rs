use crate::builtins::{self, BUILTINS};

#[derive(Default)]
pub struct Shell {
    // Exit status of the last command
    pub status: i32,
    // Set by `exit`
    pub exit: Option<i32>,
}

impl Shell {
    pub fn run_line(&mut self, line: &str) {
        let args: Vec<String> = line.split_whitespace().map(String::from).collect();
        let Some(name) = args.first() else {
            return;
        };
        if !BUILTINS.contains(&name.as_str()) {
            eprintln!("rust-shell: {name}: command not found");
            self.status = 127;
            return;
        }
        self.status = builtins::run(self, &args);
    }
}
