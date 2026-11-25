#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    // Quoted words are never expanded
    pub quoted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RedirectKind {
    In,
    Out,
    Append,
    Err,
    ErrAppend,
    Both,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(Word),
    Redirect(RedirectKind),
    ErrToOut,
}

// Longest first, so `>>` is matched before `>`
pub const OPERATORS: [(&str, Token); 7] = [
    ("2>&1", Token::ErrToOut),
    ("2>>", Token::Redirect(RedirectKind::ErrAppend)),
    ("2>", Token::Redirect(RedirectKind::Err)),
    ("&>", Token::Redirect(RedirectKind::Both)),
    (">>", Token::Redirect(RedirectKind::Append)),
    (">", Token::Redirect(RedirectKind::Out)),
    ("<", Token::Redirect(RedirectKind::In)),
];

pub const DELIMITERS: [char; 3] = ['&', '<', '>'];

#[derive(Debug)]
pub enum Redirect {
    File(RedirectKind, Word),
    ErrToOut,
}

#[derive(Debug, Default)]
pub struct Command {
    pub args: Vec<Word>,
    pub redirects: Vec<Redirect>,
}

pub fn tokenize(line: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        if let Some((op, token)) = OPERATORS.iter().find(|(op, _)| rest.starts_with(op)) {
            tokens.push(token.clone());
            rest = &rest[op.len()..];
        } else if rest.starts_with('&') {
            return Err("background jobs are not supported yet".into());
        } else {
            let (word, len) = read_word(rest)?;
            tokens.push(Token::Word(word));
            rest = &rest[len..];
        }
        rest = rest.trim_start();
    }
    Ok(tokens)
}

// Returns the word and how many bytes it took
fn read_word(input: &str) -> Result<(Word, usize), String> {
    let mut word = Word {
        text: String::new(),
        quoted: false,
    };
    let mut chars = input.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            c if c.is_whitespace() || DELIMITERS.contains(&c) => return Ok((word, i)),
            '\'' | '"' => {
                word.quoted = true;
                loop {
                    match chars.next() {
                        Some((_, q)) if q == c => break,
                        Some((_, '\\')) if c == '"' => {
                            if let Some((_, escaped)) = chars.next() {
                                word.text.push(escaped);
                            }
                        }
                        Some((_, ch)) => word.text.push(ch),
                        None => return Err(format!("unclosed quote {c}")),
                    }
                }
            }
            '\\' => {
                word.quoted = true;
                if let Some((_, escaped)) = chars.next() {
                    word.text.push(escaped);
                }
            }
            _ => word.text.push(c),
        }
    }
    Ok((word, input.len()))
}

pub fn parse(tokens: Vec<Token>) -> Result<Command, String> {
    let mut command = Command::default();
    let mut tokens = tokens.into_iter();
    while let Some(token) = tokens.next() {
        match token {
            Token::Word(word) => command.args.push(word),
            Token::Redirect(kind) => match tokens.next() {
                Some(Token::Word(target)) => command.redirects.push(Redirect::File(kind, target)),
                _ => return Err("missing file name after redirect".into()),
            },
            Token::ErrToOut => command.redirects.push(Redirect::ErrToOut),
        }
    }
    Ok(command)
}
