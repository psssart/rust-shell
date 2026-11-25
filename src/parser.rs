#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    // Quoted words are never expanded
    pub quoted: bool,
}

pub fn tokenize(line: &str) -> Result<Vec<Word>, String> {
    let mut words = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        let (word, len) = read_word(rest)?;
        words.push(word);
        rest = rest[len..].trim_start();
    }
    Ok(words)
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
            c if c.is_whitespace() => return Ok((word, i)),
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
