//! Aoxn lexer: source text -> tokens with positions.
//!
//! Python-style layout: NEWLINE / INDENT / DEDENT tokens, `#` comments,
//! blank and comment-only lines produce no tokens, and inside parentheses
//! newlines are ignored (implicit line joining). Tabs count as 4 columns.

use crate::ast::Pos;
use crate::Diag;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Def,
    Struct,
    Extern,
    Import,
    If,
    Elif,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    Return,
    Pass,
    True,
    False,
    /// `None` / `none` — the absence of a value (v0.40.0). A real keyword:
    /// nothing in the tree spells an identifier this way, and leaving it as a
    /// context-sensitive `Ident` would make `x is none` ambiguous.
    None,
    /// exceptions (v0.40.0)
    Raise,
    Try,
    Except,
    TyInt,
    TyFloat,
    TyBool,
    TyString,
    TyVoid,
    Newline,
    Indent,
    Dedent,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Semi,
    Dot,
    Colon,
    Arrow,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    /// `//` — Python-style integer division (a synonym of `/` on two ints)
    FloorDiv,
    /// bitwise operators (v0.37.0): int-only, no augmented forms
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    /// `+=`, `-=`, `*=`, `/=`, `%=` — Python-style augmented assignment
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Comma,
    FStr(Vec<FStrPart>),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

/// part of an f-string: literal text or an embedded expression (pre-lexed)
#[derive(Debug, Clone, PartialEq)]
pub enum FStrPart {
    Lit(String),
    ExprTokens(Vec<Token>),
}

pub fn lex(src: &str, file_id: u32) -> Result<Vec<Token>, Diag> {
    let chars: Vec<char> = src.chars().collect();
    let lx = Lexer {
        chars: &chars,
        file_id,
        i: 0,
        line: 1,
        col: 1,
        indent_stack: vec![0],
        paren_depth: 0,
        at_line_start: true,
        end: chars.len(),
        virtual_paren: false,
    };
    lx.run()
}

/// tokenize one f-string `{...}` interpolation in place, over the main
/// buffer's char slice (no re-collection, no padded copy). The virtual
/// `paren_depth = 1` reproduces the old wrap-in-parens trick: line-start
/// indent tracking is disabled, and starting the column at `brace_col + 1`
/// keeps every token's column identical to its source column.
fn lex_interpolation(
    chars: &[char],
    start: usize,
    end: usize,
    brace_col: usize,
    file_id: u32,
) -> Result<Vec<Token>, Diag> {
    let lx = Lexer {
        chars,
        file_id,
        i: start,
        line: 1,
        col: brace_col + 1,
        indent_stack: vec![0],
        paren_depth: 1,
        at_line_start: false,
        end,
        virtual_paren: true,
    };
    lx.run()
}

/// the four string escapes shared by plain strings and f-strings
fn escape_char(esc: char) -> Option<char> {
    match esc {
        'n' => Some('\n'),
        't' => Some('\t'),
        '\\' => Some('\\'),
        '"' => Some('"'),
        _ => None,
    }
}

/// advance one char, tracking line/column — a macro (not a method) so the
/// hot scan loops stay call-free in debug builds; the single definition is
/// shared by the main scan and f-string interpolation scanning
macro_rules! adv {
    ($s:ident) => {{
        if $s.chars[$s.i] == '\n' {
            $s.line += 1;
            $s.col = 1;
        } else {
            $s.col += 1;
        }
        $s.i += 1;
    }};
}

/// char at `i` if inside the scan bound
macro_rules! at {
    ($s:ident, $idx:expr) => {
        if $idx < $s.end {
            $s.chars.get($idx).copied()
        } else {
            None
        }
    };
}

struct Lexer<'c> {
    /// the char buffer being scanned (main source, or an interpolation slice)
    chars: &'c [char],
    file_id: u32,
    i: usize,
    line: usize,
    col: usize,
    indent_stack: Vec<usize>,
    paren_depth: i32,
    at_line_start: bool,
    /// exclusive scan bound: the main lexer runs to `chars.len()`,
    /// interpolation sub-scans stop before the closing `'}'`
    end: usize,
    /// interpolation sub-scans open with a virtual `'('`: never report it
    /// as unclosed at the end
    virtual_paren: bool,
}

impl<'c> Lexer<'c> {
    fn err(&self, line: usize, col: usize, message: impl Into<String>) -> Diag {
        Diag {
            stage: "lex",
            file: self.file_id,
            line,
            col,
            message: message.into(),
        }
    }

    fn run(mut self) -> Result<Vec<Token>, Diag> {
        let mut out: Vec<Token> = Vec::new();

        loop {
            // ---- line start: measure indentation (unless inside brackets) ----
            if self.at_line_start && self.paren_depth == 0 {
                let mut indent = 0usize;
                loop {
                    match at!(self, self.i) {
                        Some(' ') => {
                            indent += 1;
                            adv!(self);
                        }
                        Some('\t') => {
                            indent = (indent / 4 + 1) * 4;
                            adv!(self);
                        }
                        _ => break,
                    }
                }
                match at!(self, self.i) {
                    None => {
                        self.at_line_start = false;
                    }
                    Some('\r') | Some('\n') => {
                        // blank line: no tokens
                        if at!(self, self.i) == Some('\r') {
                            adv!(self);
                        }
                        adv!(self); // '\n'
                        continue;
                    }
                    Some('#') => {
                        while self.i < self.end && self.chars[self.i] != '\n' {
                            adv!(self);
                        }
                        continue;
                    }
                    Some(_) => {
                        let pos = Pos { line: self.line, col: 1, file: self.file_id };
                        let top = *self.indent_stack.last().unwrap();
                        if indent > top {
                            self.indent_stack.push(indent);
                            out.push(Token { tok: Tok::Indent, pos });
                        } else if indent < top {
                            while *self.indent_stack.last().unwrap() > indent {
                                self.indent_stack.pop();
                                out.push(Token { tok: Tok::Dedent, pos });
                            }
                            if *self.indent_stack.last().unwrap() != indent {
                                return Err(self.err(
                                    pos.line,
                                    pos.col,
                                    format!(
                                        "unindent does not match any outer indentation level (expected one of {:?}, found {})",
                                        self.indent_stack, indent
                                    ),
                                ));
                            }
                        }
                        self.at_line_start = false;
                        // fall through to token scanning
                    }
                }
            }

            let Some(c) = at!(self, self.i) else { break };

            let pos = Pos { line: self.line, col: self.col, file: self.file_id };

            // ---- whitespace / line breaks ----
            if c == ' ' || c == '\t' || c == '\r' {
                adv!(self);
                continue;
            }
            if c == '\n' {
                adv!(self);
                if self.paren_depth == 0 {
                    out.push(Token { tok: Tok::Newline, pos });
                    self.at_line_start = true;
                }
                continue;
            }
            if c == '#' {
                while self.i < self.end && self.chars[self.i] != '\n' {
                    adv!(self);
                }
                continue;
            }

            // ---- identifiers / keywords ----
            if c.is_ascii_alphabetic() || c == '_' {
                let tok = self.scan_word(pos)?;
                out.push(Token { tok, pos });
                continue;
            }

            // ---- numbers ----
            if c.is_ascii_digit() {
                let tok = self.scan_number(pos)?;
                out.push(Token { tok, pos });
                continue;
            }

            // ---- strings ----
            if c == '"' {
                let tok = self.scan_string(pos)?;
                out.push(Token { tok, pos });
                continue;
            }

            // ---- punctuation / operators ----
            let tok = self.scan_punct(c, pos)?;
            out.push(Token { tok, pos });
        }

        if self.paren_depth > 0 && !self.virtual_paren {
            return Err(self.err(
                self.line,
                self.col,
                "unclosed bracket: '(' or '[' was never closed",
            ));
        }

        // final NEWLINE, then flush remaining DEDENTs, then EOF
        let eof_pos = Pos { line: self.line, col: self.col, file: self.file_id };
        if !matches!(out.last().map(|t| &t.tok), Some(Tok::Newline)) {
            out.push(Token { tok: Tok::Newline, pos: eof_pos });
        }
        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            out.push(Token { tok: Tok::Dedent, pos: eof_pos });
        }
        out.push(Token { tok: Tok::Eof, pos: eof_pos });
        Ok(out)
    }

    /// identifier / keyword / f-string opener (the current char is a letter)
    fn scan_word(&mut self, pos: Pos) -> Result<Tok, Diag> {
        let start = self.i;
        while self.i < self.end && (self.chars[self.i].is_ascii_alphanumeric() || self.chars[self.i] == '_') {
            adv!(self);
        }
        let word: String = self.chars[start..self.i].iter().collect();

        // f-string: `f"` (or `F"`) switches to interpolation scanning
        if (word == "f" || word == "F") && at!(self, self.i) == Some('"') {
            let parts = self.scan_fstring(pos)?;
            return Ok(Tok::FStr(parts));
        }

        Ok(match word.as_str() {
            "def" => Tok::Def,
            "struct" => Tok::Struct,
            "extern" => Tok::Extern,
            "import" => Tok::Import,
            "if" => Tok::If,
            "elif" => Tok::Elif,
            "else" => Tok::Else,
            "while" => Tok::While,
            "for" => Tok::For,
            "in" => Tok::In,
            "break" => Tok::Break,
            "continue" => Tok::Continue,
            "return" => Tok::Return,
            "pass" => Tok::Pass,
            // exceptions (v0.40.0): nothing in the tree spells these as
            // identifiers, so they are real keywords
            "raise" => Tok::Raise,
            "try" => Tok::Try,
            "except" => Tok::Except,
            "and" => Tok::AndAnd,
            "or" => Tok::OrOr,
            "not" => Tok::Bang,
            "true" | "True" => Tok::True,
            "false" | "False" => Tok::False,
            "none" | "None" => Tok::None,
            "int" => Tok::TyInt,
            "float" => Tok::TyFloat,
            "bool" => Tok::TyBool,
            "string" => Tok::TyString,
            "void" => Tok::TyVoid,
            _ => Tok::Ident(word),
        })
    }

    /// integer / float literal (the current char is a digit)
    fn scan_number(&mut self, pos: Pos) -> Result<Tok, Diag> {
        let start = self.i;
        while self.i < self.end && self.chars[self.i].is_ascii_digit() {
            adv!(self);
        }
        let mut is_float = false;
        if at!(self, self.i) == Some('.')
            && at!(self, self.i + 1).map(|c| c.is_ascii_digit()).unwrap_or(false)
        {
            is_float = true;
            adv!(self); // '.'
            while self.i < self.end && self.chars[self.i].is_ascii_digit() {
                adv!(self);
            }
        }
        let text: String = self.chars[start..self.i].iter().collect();
        if is_float {
            let v: f64 = text
                .parse()
                .map_err(|_| self.err(pos.line, pos.col, format!("invalid float literal '{text}'")))?;
            Ok(Tok::Float(v))
        } else {
            let v: i64 = text.parse().map_err(|_| {
                self.err(
                    pos.line,
                    pos.col,
                    format!("integer literal '{text}' out of range (max 9223372036854775807)"),
                )
            })?;
            Ok(Tok::Int(v))
        }
    }

    /// plain string literal (the current char is `"`)
    fn scan_string(&mut self, pos: Pos) -> Result<Tok, Diag> {
        adv!(self); // opening quote
        let mut s = String::new();
        loop {
            let Some(ch) = at!(self, self.i) else {
                return Err(self.err(pos.line, pos.col, "unterminated string literal"));
            };
            if ch == '"' {
                adv!(self);
                break;
            }
            if ch == '\\' {
                adv!(self);
                let Some(esc) = at!(self, self.i) else {
                    return Err(self.err(pos.line, pos.col, "unterminated string literal"));
                };
                match escape_char(esc) {
                    Some(c) => s.push(c),
                    None => {
                        return Err(self.err(
                            pos.line,
                            pos.col,
                            format!("unknown escape sequence '\\{esc}'"),
                        ))
                    }
                }
                adv!(self);
                continue;
            }
            s.push(ch);
            adv!(self);
        }
        Ok(Tok::Str(s))
    }

    /// punctuation / operators; brackets adjust `paren_depth` and
    /// unmatched closers are rejected here
    fn scan_punct(&mut self, c: char, pos: Pos) -> Result<Tok, Diag> {
        macro_rules! two {
            ($second:expr) => {
                at!(self, self.i + 1) == Some($second)
            };
        }
        let tok = match c {
            '(' => {
                self.paren_depth += 1;
                adv!(self);
                Tok::LParen
            }
            '[' => {
                self.paren_depth += 1;
                adv!(self);
                Tok::LBracket
            }
            // braces join lines too (v0.40.0): `{"k": v}` is a dict literal,
            // and the TS front end's object literals want the same rule
            '{' => {
                self.paren_depth += 1;
                adv!(self);
                Tok::LBrace
            }
            '}' => {
                self.paren_depth -= 1;
                if self.paren_depth < 0 {
                    return Err(self.err(pos.line, pos.col, "unmatched closing '}'"));
                }
                adv!(self);
                Tok::RBrace
            }
            ')' | ']' => {
                self.paren_depth -= 1;
                if self.paren_depth < 0 {
                    return Err(self.err(pos.line, pos.col, format!("unmatched closing '{c}'")));
                }
                adv!(self);
                if c == ')' {
                    Tok::RParen
                } else {
                    Tok::RBracket
                }
            }
            ';' | '.' | ',' | ':' => {
                adv!(self);
                match c {
                    ';' => Tok::Semi,
                    '.' => Tok::Dot,
                    ',' => Tok::Comma,
                    _ => Tok::Colon,
                }
            }
            // `+=` / `*=`: the augmented forms; a bare `+`/`*` is the binary
            // (or unary) operator
            '+' => {
                if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::PlusAssign
                } else {
                    adv!(self);
                    Tok::Plus
                }
            }
            '*' => {
                if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::StarAssign
                } else {
                    adv!(self);
                    Tok::Star
                }
            }
            // `//` is Python-style integer division; `/=` is the augmented form
            '/' => {
                if two!('/') {
                    adv!(self);
                    adv!(self);
                    Tok::FloorDiv
                } else if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::SlashAssign
                } else {
                    adv!(self);
                    Tok::Slash
                }
            }
            '%' => {
                if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::PercentAssign
                } else {
                    adv!(self);
                    Tok::Percent
                }
            }
            '-' => {
                if two!('>') {
                    adv!(self);
                    adv!(self);
                    Tok::Arrow
                } else if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::MinusAssign
                } else {
                    adv!(self);
                    Tok::Minus
                }
            }
            '=' => {
                if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::Eq
                } else {
                    adv!(self);
                    Tok::Assign
                }
            }
            '!' => {
                if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::Ne
                } else {
                    adv!(self);
                    Tok::Bang
                }
            }
            '<' => {
                if two!('<') {
                    adv!(self);
                    adv!(self);
                    Tok::Shl
                } else if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::Le
                } else {
                    adv!(self);
                    Tok::Lt
                }
            }
            '>' => {
                if two!('>') {
                    adv!(self);
                    adv!(self);
                    Tok::Shr
                } else if two!('=') {
                    adv!(self);
                    adv!(self);
                    Tok::Ge
                } else {
                    adv!(self);
                    Tok::Gt
                }
            }
            '&' => {
                if two!('&') {
                    adv!(self);
                    adv!(self);
                    Tok::AndAnd
                } else {
                    adv!(self);
                    Tok::Amp
                }
            }
            '|' => {
                if two!('|') {
                    adv!(self);
                    adv!(self);
                    Tok::OrOr
                } else {
                    adv!(self);
                    Tok::Pipe
                }
            }
            '^' => {
                adv!(self);
                Tok::Caret
            }
            '~' => {
                adv!(self);
                Tok::Tilde
            }
            _ => {
                return Err(self.err(pos.line, pos.col, format!("unexpected character '{c}'")));
            }
        };
        Ok(tok)
    }

    /// scan an f-string literal (after `f"`): literal parts with escapes,
    /// `{expr}` interpolations (lexed in place over the main buffer),
    /// `{{`/`}}` escapes.
    fn scan_fstring(&mut self, open_pos: Pos) -> Result<Vec<FStrPart>, Diag> {
        let mut parts: Vec<FStrPart> = Vec::new();
        let mut lit = String::new();

        adv!(self); // consume opening quote

        loop {
            let Some(ch) = at!(self, self.i) else {
                return Err(self.err(
                    open_pos.line,
                    open_pos.col,
                    "unterminated f-string literal",
                ));
            };

            if ch == '"' {
                adv!(self);
                break;
            }

            if ch == '{' {
                if at!(self, self.i + 1) == Some('{') {
                    lit.push('{');
                    adv!(self);
                    adv!(self);
                    continue;
                }
                if !lit.is_empty() {
                    parts.push(FStrPart::Lit(std::mem::take(&mut lit)));
                }
                let brace_col = self.col;
                adv!(self); // consume '{'
                let start = self.i;
                let mut depth: i32 = 0;
                loop {
                    if self.i >= self.end {
                        return Err(self.err(
                            open_pos.line,
                            open_pos.col,
                            "unterminated '{' in f-string",
                        ));
                    }
                    let c2 = self.chars[self.i];
                    match c2 {
                        '(' | '[' => depth += 1,
                        ')' | ']' => depth -= 1,
                        '"' => {
                            adv!(self);
                            while self.i < self.end && self.chars[self.i] != '"' {
                                if self.chars[self.i] == '\\' && self.i + 1 < self.end {
                                    adv!(self);
                                }
                                adv!(self);
                            }
                        }
                        '}' if depth == 0 => break,
                        _ => {}
                    }
                    adv!(self);
                }
                if at!(self, self.i) != Some('}') {
                    return Err(self.err(
                        open_pos.line,
                        open_pos.col,
                        "unterminated '{' in f-string",
                    ));
                }
                let close = self.i;
                adv!(self); // consume '}'
                let expr_text: String = self.chars[start..close].iter().collect();
                if expr_text.trim().is_empty() {
                    return Err(self.err(
                        open_pos.line,
                        open_pos.col,
                        "empty '{}' in f-string",
                    ));
                }
                // lex the interpolation in place over the main buffer: the
                // virtual paren disables indent tracking and the column
                // offset keeps every token at its source column
                let toks = lex_interpolation(self.chars, start, close, brace_col, self.file_id)?;
                parts.push(FStrPart::ExprTokens(toks));
                continue;
            }

            if ch == '}' {
                if at!(self, self.i + 1) == Some('}') {
                    lit.push('}');
                    adv!(self);
                    adv!(self);
                    continue;
                }
                return Err(self.err(
                    open_pos.line,
                    open_pos.col,
                    "single '}' in f-string (use '}}' for a literal brace)",
                ));
            }

            if ch == '\\' {
                adv!(self);
                let Some(esc) = at!(self, self.i) else {
                    return Err(self.err(
                        open_pos.line,
                        open_pos.col,
                        "unterminated f-string literal",
                    ));
                };
                match escape_char(esc) {
                    Some(c) => lit.push(c),
                    None => {
                        return Err(self.err(
                            open_pos.line,
                            open_pos.col,
                            format!("unknown escape sequence '\\{esc}' in f-string"),
                        ))
                    }
                }
                adv!(self);
                continue;
            }

            lit.push(ch);
            adv!(self);
        }

        if !lit.is_empty() {
            parts.push(FStrPart::Lit(lit));
        }
        Ok(parts)
    }
}
