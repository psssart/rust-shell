use std::collections::BTreeMap;
use std::mem;

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
    Pipe,
    And,
    Or,
    Semi,
    Redirect(RedirectKind),
    ErrToOut,
}

impl Token {
    pub fn is_separator(&self) -> bool {
        matches!(self, Token::Pipe | Token::And | Token::Or | Token::Semi)
    }
}

// Longest first, so `>>` is matched before `>`
pub const OPERATORS: [(&str, Token); 11] = [
    ("2>&1", Token::ErrToOut),
    ("2>>", Token::Redirect(RedirectKind::ErrAppend)),
    ("2>", Token::Redirect(RedirectKind::Err)),
    ("&>", Token::Redirect(RedirectKind::Both)),
    (">>", Token::Redirect(RedirectKind::Append)),
    (">", Token::Redirect(RedirectKind::Out)),
    ("<", Token::Redirect(RedirectKind::In)),
    ("&&", Token::And),
    ("||", Token::Or),
    ("|", Token::Pipe),
    (";", Token::Semi),
];

pub const DELIMITERS: [char; 5] = ['|', '&', ';', '<', '>'];

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

impl Command {
    fn is_empty(&self) -> bool {
        self.args.is_empty() && self.redirects.is_empty()
    }
}

// How a pipeline is connected to the previous one
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Always,
    And,
    Or,
}

pub type Pipeline = Vec<Command>;

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
                        // Like bash, `\n` stays as is, only `\"`, `\\`, `\$` and `\`` are escapes
                        Some((_, '\\')) if c == '"' => match chars.next() {
                            Some((_, escaped)) if matches!(escaped, '"' | '\\' | '$' | '`') => {
                                word.text.push(escaped);
                            }
                            Some((_, other)) => {
                                word.text.push('\\');
                                word.text.push(other);
                            }
                            None => return Err(format!("unclosed quote {c}")),
                        },
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

pub fn expand_aliases(
    tokens: Vec<Token>,
    aliases: &BTreeMap<String, String>,
) -> Result<Vec<Token>, String> {
    expand_aliases_inner(tokens, aliases, &mut Vec::new())
}

// `seen` stops loops like `alias ls='ls -a'`
fn expand_aliases_inner(
    tokens: Vec<Token>,
    aliases: &BTreeMap<String, String>,
    seen: &mut Vec<String>,
) -> Result<Vec<Token>, String> {
    let mut result = Vec::new();
    let mut command_start = true;
    for token in tokens {
        if command_start
            && let Token::Word(word) = &token
            && !word.quoted
            && !seen.contains(&word.text)
            && let Some(value) = aliases.get(&word.text)
        {
            seen.push(word.text.clone());
            let expanded = expand_aliases_inner(tokenize(value)?, aliases, seen)?;
            seen.pop();
            command_start = expanded.last().is_none_or(Token::is_separator);
            result.extend(expanded);
            continue;
        }
        command_start = token.is_separator();
        result.push(token);
    }
    Ok(result)
}

pub fn parse(tokens: Vec<Token>) -> Result<Vec<(Op, Pipeline)>, String> {
    let mut chain = Vec::new();
    let mut pipeline = Vec::new();
    let mut command = Command::default();
    let mut op = Op::Always;
    let mut tokens = tokens.into_iter();

    while let Some(token) = tokens.next() {
        match token {
            Token::Word(word) => command.args.push(word),
            Token::Redirect(kind) => match tokens.next() {
                Some(Token::Word(target)) => command.redirects.push(Redirect::File(kind, target)),
                _ => return Err("missing file name after redirect".into()),
            },
            Token::ErrToOut => command.redirects.push(Redirect::ErrToOut),
            Token::Pipe => {
                if command.is_empty() {
                    return Err("syntax error: missing command before `|`".into());
                }
                pipeline.push(mem::take(&mut command));
            }
            Token::And | Token::Or | Token::Semi => {
                if command.is_empty() {
                    // Allow trailing `;`
                    if token == Token::Semi && pipeline.is_empty() && op == Op::Always {
                        continue;
                    }
                    return Err("syntax error: missing command".into());
                }
                pipeline.push(mem::take(&mut command));
                chain.push((op, mem::take(&mut pipeline)));
                op = match token {
                    Token::And => Op::And,
                    Token::Or => Op::Or,
                    _ => Op::Always,
                };
            }
        }
    }

    if !command.is_empty() {
        pipeline.push(command);
        chain.push((op, pipeline));
    } else if !pipeline.is_empty() || op != Op::Always {
        return Err("syntax error: unexpected end of line".into());
    }
    Ok(chain)
}
