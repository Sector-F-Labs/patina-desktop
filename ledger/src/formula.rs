//! Cell values and formulas: `=A1+B2*3`, `=SUM(B2:B9)`, `=ROUND(C1/3, 2)`.
//!
//! A cell's *input* is the text the user typed. If it starts with `=` it's a
//! formula; if it reads as a number it's a number; otherwise it's text. The
//! input is what's stored (ADR-0002); values are worked out from it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use crate::shell::{self, Block, Job, Options, Status, Target};

/// A cell position, zero-based: `(row, col)`. `A1` is `(0, 0)`.
pub type Pos = (u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Text(String),
    Error(String),
    /// A shell command that's running.
    Pending,
}

impl Value {
    /// What a cell shows.
    pub fn display(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => format_number(*n),
            Value::Text(t) => t.clone(),
            Value::Error(e) => format!("#{e}"),
            Value::Pending => "…".into(),
        }
    }
}

/// A number as shown: no float noise (`0.1+0.2` is `0.3`), no trailing zeros.
pub fn format_number(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM".into();
    }
    let s = format!("{:.10}", n);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

/// `0` → `A`, `25` → `Z`, `26` → `AA`.
pub fn col_name(mut col: u32) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap()
}

/// `(0, 0)` → `A1`.
pub fn cell_name((row, col): Pos) -> String {
    format!("{}{}", col_name(col), row + 1)
}

/// `A1` → `(0, 0)`; also accepts `$A$1` and lower case.
pub fn parse_ref(text: &str) -> Option<Pos> {
    let text = text.replace('$', "");
    let letters = text.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let (l, d) = text.split_at(letters);
    if l.is_empty() || l.len() > 3 || d.is_empty() || !d.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col = l.to_ascii_uppercase().bytes().fold(0u32, |acc, b| acc * 26 + (b - b'A') as u32 + 1) - 1;
    let row: u32 = d.parse().ok()?;
    if row == 0 || row > 1_048_576 {
        return None;
    }
    Some((row - 1, col))
}

/// What the input of a cell is, without working out formulas.
pub fn parse_input(input: &str) -> Value {
    let t = input.trim();
    if t.is_empty() {
        return Value::Empty;
    }
    match t.replace(',', "").parse::<f64>() {
        Ok(n) if n.is_finite() && !t.eq_ignore_ascii_case("nan") && !t.to_ascii_lowercase().contains("inf") => {
            Value::Number(n)
        }
        _ => Value::Text(input.to_string()),
    }
}

// --- Parsing -----------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(char),
    Comma,
    Colon,
    Eq,
    LParen,
    RParen,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '+' | '-' | '*' | '/' | '^' | '&' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            ',' | ';' => {
                out.push(Tok::Comma);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            '=' => {
                out.push(Tok::Eq);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("PARSE".into()),
                        Some('"') if chars.get(i + 1) == Some(&'"') => {
                            s.push('"');
                            i += 2;
                        }
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(&c) => {
                            s.push(c);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let n = chars[start..i].iter().collect::<String>().parse().map_err(|_| "PARSE".to_string())?;
                out.push(Tok::Num(n));
            }
            c if c.is_ascii_alphabetic() || c == '$' || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '$' || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            _ => return Err("PARSE".into()),
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Ref(Pos),
    Range(Pos, Pos),
    Neg(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// `name=value` in a call's arguments (shell options).
    Named(String, Box<Expr>),
    /// Where shell output goes: `:F20` (no start), `B2:F` (open ended).
    Block(Option<Pos>, Target),
}

/// `F20` or `F`: the end of a block.
fn parse_end(text: &str) -> Option<Target> {
    if let Some((row, col)) = parse_ref(text) {
        return Some(Target { end_col: Some(col), end_row: Some(row) });
    }
    let letters = text.chars().all(|c| c.is_ascii_alphabetic());
    if letters && !text.is_empty() && text.len() <= 3 {
        let col = text.to_ascii_uppercase().bytes().fold(0u32, |acc, b| acc * 26 + (b - b'A') as u32 + 1) - 1;
        return Some(Target { end_col: Some(col), end_row: None });
    }
    None
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    // expr := concat; concat := sum ('&' sum)*
    fn expr(&mut self) -> Result<Expr, String> {
        let mut left = self.sum()?;
        while self.peek() == Some(&Tok::Op('&')) {
            self.next();
            left = Expr::Bin('&', Box::new(left), Box::new(self.sum()?));
        }
        Ok(left)
    }
    fn sum(&mut self) -> Result<Expr, String> {
        let mut left = self.product()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.next();
            left = Expr::Bin(c, Box::new(left), Box::new(self.product()?));
        }
        Ok(left)
    }
    fn product(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.next();
            left = Expr::Bin(c, Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.next();
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.next();
                self.unary()
            }
            _ => self.power(),
        }
    }
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.next();
            return Ok(Expr::Bin('^', Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err("PARSE".into()),
                }
            }
            Some(Tok::Colon) => match self.next() {
                Some(Tok::Ident(n)) => Ok(Expr::Block(None, parse_end(&n).ok_or_else(|| "NAME".to_string())?)),
                _ => Err("PARSE".into()),
            },
            Some(Tok::Ident(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.next();
                    } else {
                        loop {
                            if let (Some(Tok::Ident(n)), Some(Tok::Eq)) = (self.toks.get(self.at), self.toks.get(self.at + 1)) {
                                let name = n.to_ascii_lowercase();
                                self.at += 2;
                                // A bare word is text: `rest=drop`.
                                let value = match (self.toks.get(self.at), self.toks.get(self.at + 1)) {
                                    (Some(Tok::Ident(w)), next)
                                        if parse_ref(w).is_none() && next != Some(&Tok::LParen) =>
                                    {
                                        let w = w.clone();
                                        self.at += 1;
                                        Expr::Str(w)
                                    }
                                    _ => self.expr()?,
                                };
                                args.push(Expr::Named(name, Box::new(value)));
                            } else {
                                args.push(self.expr()?);
                            }
                            match self.next() {
                                Some(Tok::Comma) => {}
                                Some(Tok::RParen) => break,
                                _ => return Err("PARSE".into()),
                            }
                        }
                    }
                    return Ok(Expr::Call(name.to_ascii_uppercase(), args));
                }
                let a = parse_ref(&name).ok_or_else(|| "NAME".to_string())?;
                if self.peek() == Some(&Tok::Colon) {
                    self.next();
                    match self.next() {
                        Some(Tok::Ident(n2)) => {
                            if let Some(b) = parse_ref(&n2) {
                                return Ok(Expr::Range(a, b));
                            }
                            let end = parse_end(&n2).ok_or_else(|| "NAME".to_string())?;
                            return Ok(Expr::Block(Some(a), end));
                        }
                        _ => return Err("PARSE".into()),
                    }
                }
                Ok(Expr::Ref(a))
            }
            _ => Err("PARSE".into()),
        }
    }
}

/// Parses the text after the `=`.
pub fn parse_formula(src: &str) -> Result<Expr, String> {
    let mut p = Parser { toks: tokenize(src)?, at: 0 };
    let e = p.expr()?;
    if p.at < p.toks.len() { Err("PARSE".into()) } else { Ok(e) }
}

// --- Evaluating --------------------------------------------------------

/// What the evaluator needs to know about shell cells.
#[derive(Default)]
pub struct ShellState {
    /// The last result of each shell cell, by the cell.
    pub runs: HashMap<Pos, shell::Run>,
    /// Shell cells whose command is running.
    pub pending: HashSet<Pos>,
}

/// Everything worked out for a sheet.
pub struct Evaluated {
    /// The value of every non-empty cell, including cells filled by shell output.
    pub values: HashMap<Pos, Value>,
    /// The blocks shell commands fill.
    pub blocks: Vec<Block>,
    /// Shell cells whose block is in the way of something typed or of another block.
    pub conflicts: HashSet<Pos>,
}

/// What a shell formula asks for, from its arguments.
struct ShellSpec {
    target: Target,
    options: Options,
    timeout: Duration,
}

fn literal_text(e: &Expr) -> Option<String> {
    match e {
        Expr::Str(s) => Some(s.clone()),
        Expr::Num(n) => Some(format_number(*n)),
        _ => None,
    }
}

/// The shell formula's call arguments, if `src` (after the `=`) is one.
fn shell_call(src: &str) -> Option<Vec<Expr>> {
    match parse_formula(src) {
        Ok(Expr::Call(name, args)) if name == "SH" => Some(args),
        _ => None,
    }
}

fn shell_spec(owner: Pos, args: &[Expr]) -> Result<ShellSpec, String> {
    let mut spec = ShellSpec { target: Target::default(), options: Options::default(), timeout: shell::DEFAULT_TIMEOUT };
    let mut have_target = false;
    for arg in args.iter().skip(1) {
        match arg {
            Expr::Range(a, b) if !have_target => {
                if *a != owner {
                    return Err("RANGE".into());
                }
                spec.target = Target { end_col: Some(b.1), end_row: Some(b.0) };
                have_target = true;
            }
            Expr::Block(start, target) if !have_target => {
                if start.is_some_and(|s| s != owner) {
                    return Err("RANGE".into());
                }
                spec.target = *target;
                have_target = true;
            }
            Expr::Named(name, value) => match (name.as_str(), literal_text(value)) {
                ("sep", Some(s)) => spec.options.sep = Some(s),
                ("skip", Some(n)) => spec.options.skip = n.parse().map_err(|_| "VALUE".to_string())?,
                ("rest", Some(r)) if r == "keep" || r == "drop" => spec.options.keep_rest = r == "keep",
                ("timeout", Some(n)) => {
                    spec.timeout = Duration::from_secs_f64(n.parse::<f64>().map_err(|_| "VALUE".to_string())?.clamp(0.1, 600.0))
                }
                ("arg", _) => {}
                _ => return Err("VALUE".into()),
            },
            _ => return Err("VALUE".into()),
        }
    }
    if let (Some(c), true) = (spec.target.end_col, true)
        && c < owner.1
    {
        return Err("RANGE".into());
    }
    if spec.target.end_row.is_some_and(|r| r < owner.0) {
        return Err("RANGE".into());
    }
    Ok(spec)
}

/// Works out the values of a sheet's cells from their inputs, each once.
pub struct Evaluator<'a> {
    inputs: &'a BTreeMap<Pos, String>,
    state: &'a ShellState,
    done: HashMap<Pos, Value>,
    visiting: HashSet<Pos>,
    blocks: Vec<Block>,
    conflicts: HashSet<Pos>,
    /// The cells shell blocks fill, other than the formula's own.
    spills: HashMap<Pos, Value>,
}

impl<'a> Evaluator<'a> {
    #[cfg(test)]
    pub fn new(inputs: &'a BTreeMap<Pos, String>) -> Self {
        static NONE: std::sync::OnceLock<ShellState> = std::sync::OnceLock::new();
        Self::with_shell(inputs, NONE.get_or_init(ShellState::default))
    }

    pub fn with_shell(inputs: &'a BTreeMap<Pos, String>, state: &'a ShellState) -> Self {
        let mut ev = Self {
            inputs,
            state,
            done: HashMap::new(),
            visiting: HashSet::new(),
            blocks: Vec::new(),
            conflicts: HashSet::new(),
            spills: HashMap::new(),
        };
        ev.place_blocks();
        ev
    }

    /// Lays out the output of every shell cell that has a good result, and
    /// refuses any block that would overwrite typed cells or another block.
    fn place_blocks(&mut self) {
        for (&owner, input) in self.inputs {
            let Some(src) = input.strip_prefix('=') else { continue };
            let Some(args) = shell_call(src) else { continue };
            let Some(run) = self.state.runs.get(&owner).filter(|r| &r.formula == input && r.status == Status::Ok) else {
                continue;
            };
            let Ok(spec) = shell_spec(owner, &args) else { continue };
            let block = shell::layout(owner, spec.target, &spec.options, &run.stdout);
            if block.rows == 0 || block.cols == 0 {
                continue;
            }
            let (er, ec) = block.end();
            let typed = self
                .inputs
                .range((owner.0, 0)..=(er, u32::MAX))
                .any(|(&p, _)| p != owner && block.contains(p));
            let overlaps = self.blocks.iter().any(|b| {
                let (br, bc) = b.end();
                owner.0 <= br && b.owner.0 <= er && owner.1 <= bc && b.owner.1 <= ec
            });
            if typed || overlaps {
                self.conflicts.insert(owner);
                continue;
            }
            for (i, row) in block.cells.iter().enumerate() {
                for (j, field) in row.iter().enumerate() {
                    let pos = (owner.0 + i as u32, owner.1 + j as u32);
                    if pos != owner && !field.is_empty() {
                        self.spills.insert(pos, parse_input(field));
                    }
                }
            }
            self.blocks.push(block);
        }
    }

    /// The value of every non-empty cell, and the blocks.
    pub fn all(mut self) -> Evaluated {
        let keys: Vec<Pos> = self.inputs.keys().copied().collect();
        for k in keys {
            self.value(k);
        }
        let mut values = std::mem::take(&mut self.spills);
        values.extend(self.done);
        Evaluated { values, blocks: self.blocks, conflicts: self.conflicts }
    }

    pub fn value(&mut self, pos: Pos) -> Value {
        if let Some(v) = self.done.get(&pos) {
            return v.clone();
        }
        let Some(input) = self.inputs.get(&pos) else {
            return self.spills.get(&pos).cloned().unwrap_or(Value::Empty);
        };
        let value = match input.strip_prefix('=') {
            None => parse_input(input),
            Some(src) => {
                if !self.visiting.insert(pos) {
                    return Value::Error("CYCLE".into());
                }
                let v = match parse_formula(src) {
                    Ok(Expr::Call(name, args)) if name == "SH" => self.shell_value(pos, input, &args),
                    Ok(expr) => self.eval(&expr),
                    Err(e) => Value::Error(e),
                };
                self.visiting.remove(&pos);
                v
            }
        };
        // A cycle's inner cells were answered with an error without being
        // stored; only finished cells are cached.
        self.done.insert(pos, value.clone());
        value
    }

    /// A shell cell's own value: the top-left of its block, or why there isn't one.
    fn shell_value(&mut self, pos: Pos, input: &str, args: &[Expr]) -> Value {
        if let Err(e) = shell_spec(pos, args) {
            return Value::Error(e);
        }
        if self.state.pending.contains(&pos) {
            return Value::Pending;
        }
        let Some(run) = self.state.runs.get(&pos).filter(|r| r.formula == input) else {
            return Value::Error("NORUN".into());
        };
        match &run.status {
            Status::Ok => {}
            Status::Exit(_) => return Value::Error("EXIT".into()),
            Status::Timeout => return Value::Error("TIMEOUT".into()),
            Status::Failed(_) => return Value::Error("FAILED".into()),
        }
        if self.conflicts.contains(&pos) {
            return Value::Error("SPILL".into());
        }
        match self.blocks.iter().find(|b| b.owner == pos).and_then(|b| b.get(pos)) {
            Some(first) => parse_input(first),
            None => Value::Empty,
        }
    }

    /// What to run for a shell cell: the command and its `arg=` cells, as they are now.
    pub fn shell_job(&mut self, pos: Pos) -> Result<Job, String> {
        let input = self.inputs.get(&pos).ok_or("not a shell cell")?;
        let args = input.strip_prefix('=').and_then(shell_call).ok_or("not a shell cell")?;
        let spec = shell_spec(pos, &args).map_err(|e| format!("#{e}"))?;
        let command = match args.first() {
            Some(e) => match self.eval(e) {
                Value::Error(e) => return Err(format!("#{e}")),
                v => v.display(),
            },
            None => return Err("SH needs a command".into()),
        };
        let mut cmd_args = Vec::new();
        for arg in &args {
            if let Expr::Named(name, value) = arg
                && name == "arg"
            {
                match self.eval(value) {
                    Value::Error(e) => return Err(format!("#{e}")),
                    v => cmd_args.push(v.display()),
                }
            }
        }
        Ok(Job { formula: input.clone(), command, args: cmd_args, timeout: spec.timeout })
    }

    fn number(&mut self, e: &Expr) -> Result<f64, Value> {
        match self.eval(e) {
            Value::Number(n) => Ok(n),
            Value::Empty => Ok(0.0),
            Value::Text(t) => match parse_input(&t) {
                Value::Number(n) => Ok(n),
                _ => Err(Value::Error("VALUE".into())),
            },
            err @ Value::Error(_) => Err(err),
            Value::Pending => Err(Value::Pending),
        }
    }

    fn eval(&mut self, e: &Expr) -> Value {
        match e {
            Expr::Num(n) => Value::Number(*n),
            Expr::Str(s) => Value::Text(s.clone()),
            Expr::Ref(p) => self.value(*p),
            Expr::Range(..) | Expr::Block(..) | Expr::Named(..) => Value::Error("VALUE".into()),
            Expr::Neg(x) => match self.number(x) {
                Ok(n) => Value::Number(-n),
                Err(e) => e,
            },
            Expr::Bin('&', a, b) => {
                let (a, b) = (self.eval(a), self.eval(b));
                match (a, b) {
                    (err @ Value::Error(_), _) | (_, err @ Value::Error(_)) => err,
                    (a, b) => Value::Text(a.display() + &b.display()),
                }
            }
            Expr::Bin(op, a, b) => {
                let a = match self.number(a) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
                let b = match self.number(b) {
                    Ok(n) => n,
                    Err(e) => return e,
                };
                let n = match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' if b == 0.0 => return Value::Error("DIV/0".into()),
                    '/' => a / b,
                    '^' => a.powf(b),
                    _ => return Value::Error("PARSE".into()),
                };
                if n.is_finite() { Value::Number(n) } else { Value::Error("NUM".into()) }
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    /// Numbers in the arguments; ranges contribute their numeric cells only.
    fn numbers(&mut self, args: &[Expr]) -> Result<Vec<f64>, Value> {
        let mut out = Vec::new();
        for arg in args {
            match arg {
                Expr::Range(a, b) => {
                    for r in a.0.min(b.0)..=a.0.max(b.0) {
                        for c in a.1.min(b.1)..=a.1.max(b.1) {
                            if !self.inputs.contains_key(&(r, c)) && !self.spills.contains_key(&(r, c)) {
                                continue;
                            }
                            match self.value((r, c)) {
                                Value::Number(n) => out.push(n),
                                err @ (Value::Error(_) | Value::Pending) => return Err(err),
                                _ => {}
                            }
                        }
                    }
                }
                other => out.push(self.number(other)?),
            }
        }
        Ok(out)
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Value {
        let nums = |this: &mut Self| this.numbers(args);
        let n = match name {
            "SUM" => nums(self).map(|v| v.iter().sum()),
            "AVERAGE" | "AVG" => nums(self).and_then(|v| {
                if v.is_empty() { Err(Value::Error("DIV/0".into())) } else { Ok(v.iter().sum::<f64>() / v.len() as f64) }
            }),
            "MIN" => nums(self).map(|v| v.into_iter().fold(f64::INFINITY, f64::min)).map(|m| if m.is_finite() { m } else { 0.0 }),
            "MAX" => nums(self).map(|v| v.into_iter().fold(f64::NEG_INFINITY, f64::max)).map(|m| if m.is_finite() { m } else { 0.0 }),
            "COUNT" => nums(self).map(|v| v.len() as f64),
            "ABS" if args.len() == 1 => self.number(&args[0]).map(f64::abs),
            "ROUND" if (1..=2).contains(&args.len()) => {
                let x = match self.number(&args[0]) {
                    Ok(x) => x,
                    Err(e) => return e,
                };
                let digits = match args.get(1) {
                    Some(d) => match self.number(d) {
                        Ok(d) => d as i32,
                        Err(e) => return e,
                    },
                    None => 0,
                };
                let f = 10f64.powi(digits);
                Ok((x * f).round() / f)
            }
            _ => return Value::Error("NAME".into()),
        };
        match n {
            Ok(n) if n.is_finite() => Value::Number(n),
            Ok(_) => Value::Error("NUM".into()),
            Err(e) => e,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(cells: &[(&str, &str)], at: &str) -> Value {
        let map: BTreeMap<Pos, String> = cells.iter().map(|(k, v)| (parse_ref(k).unwrap(), v.to_string())).collect();
        Evaluator::new(&map).value(parse_ref(at).unwrap())
    }

    #[test]
    fn names_round_trip() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        for col in [0, 5, 25, 26, 51, 701, 702, 1000] {
            assert_eq!(parse_ref(&cell_name((9, col))), Some((9, col)));
        }
        assert_eq!(parse_ref("$b$3"), Some((2, 1)));
        assert_eq!(parse_ref("A0"), None);
        assert_eq!(parse_ref("SUM"), None);
    }

    #[test]
    fn inputs_are_numbers_or_text() {
        assert_eq!(parse_input("12.50"), Value::Number(12.5));
        assert_eq!(parse_input("1,200"), Value::Number(1200.0));
        assert_eq!(parse_input("Rent"), Value::Text("Rent".into()));
        assert_eq!(parse_input("inf"), Value::Text("inf".into()));
        assert_eq!(parse_input("  "), Value::Empty);
    }

    #[test]
    fn arithmetic() {
        assert_eq!(eval(&[("A1", "=1+2*3")], "A1"), Value::Number(7.0));
        assert_eq!(eval(&[("A1", "=(1+2)*3")], "A1"), Value::Number(9.0));
        assert_eq!(eval(&[("A1", "=-2^2")], "A1"), Value::Number(-4.0));
        assert_eq!(eval(&[("A1", "=2^3^2")], "A1"), Value::Number(512.0));
        assert_eq!(eval(&[("A1", "=1/0")], "A1"), Value::Error("DIV/0".into()));
        assert_eq!(eval(&[("A1", "=1+")], "A1"), Value::Error("PARSE".into()));
    }

    #[test]
    fn references_and_ranges() {
        let c = [("A1", "10"), ("A2", "20.5"), ("A3", "x"), ("B1", "=A1+A2"), ("B2", "=SUM(A1:A3)"), ("B3", "=AVERAGE(A1:A2)")];
        assert_eq!(eval(&c, "B1"), Value::Number(30.5));
        assert_eq!(eval(&c, "B2"), Value::Number(30.5));
        assert_eq!(eval(&c, "B3"), Value::Number(15.25));
        assert_eq!(eval(&[("A1", "=SUM(B1:B9)")], "A1"), Value::Number(0.0));
        assert_eq!(eval(&[("A1", "=A9+1")], "A1"), Value::Number(1.0));
        assert_eq!(eval(&[("A1", "=A2+1"), ("A2", "x")], "A1"), Value::Error("VALUE".into()));
    }

    #[test]
    fn functions_and_text() {
        assert_eq!(eval(&[("A1", "=ROUND(2.345, 2)")], "A1"), Value::Number(2.35));
        assert_eq!(eval(&[("A1", "=MAX(1,5,3)")], "A1"), Value::Number(5.0));
        assert_eq!(eval(&[("A1", "=COUNT(B1:B3)"), ("B1", "1"), ("B2", "x"), ("B3", "3")], "A1"), Value::Number(2.0));
        assert_eq!(eval(&[("A1", "=\"a\"&\"b\"&1")], "A1"), Value::Text("ab1".into()));
        assert_eq!(eval(&[("A1", "=FOO(1)")], "A1"), Value::Error("NAME".into()));
    }

    #[test]
    fn cycles_are_errors_not_hangs() {
        let c = [("A1", "=B1"), ("B1", "=A1"), ("C1", "=C1+1")];
        assert_eq!(eval(&c, "A1"), Value::Error("CYCLE".into()));
        assert_eq!(eval(&c, "C1"), Value::Error("CYCLE".into()));
    }

    #[test]
    fn numbers_show_without_noise() {
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1200.0), "1200");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(2.5), "2.5");
    }

    fn run(formula: &str, stdout: &str) -> shell::Run {
        shell::Run { formula: formula.into(), status: Status::Ok, stdout: stdout.into(), stderr: String::new(), ran_at: 0 }
    }

    fn shell_sheet(cells: &[(&str, &str)], runs: &[(&str, &str)]) -> Evaluated {
        let map: BTreeMap<Pos, String> = cells.iter().map(|(k, v)| (parse_ref(k).unwrap(), v.to_string())).collect();
        let mut state = ShellState::default();
        for (at, out) in runs {
            let pos = parse_ref(at).unwrap();
            state.runs.insert(pos, run(&map[&pos], out));
        }
        Evaluator::with_shell(&map, &state).all()
    }

    const DF: &str = "Filesystem Size Used\n/dev/sda1 100 40\ntmpfs 8 0\n";

    #[test]
    fn shell_output_fills_a_block_from_the_formula_cell() {
        let e = shell_sheet(&[("B2", "=SH(\"df\", :D)"), ("F2", "=SUM(C3:C4)")], &[("B2", DF)]);
        let at = |s: &str| e.values.get(&parse_ref(s).unwrap()).cloned().unwrap_or(Value::Empty);
        assert_eq!(at("B2"), Value::Text("Filesystem".into()), "the formula cell shows the block's first cell");
        assert_eq!(at("D2"), Value::Text("Used".into()));
        assert_eq!(at("C3"), Value::Number(100.0));
        assert_eq!(at("F2"), Value::Number(108.0), "formulas read the block");
        assert_eq!(e.blocks.len(), 1);
        assert_eq!(e.blocks[0].end(), (3, 3));
    }

    #[test]
    fn a_block_never_overwrites_typed_cells_or_another_block() {
        let e = shell_sheet(&[("A1", "=SH(\"df\")"), ("B2", "typed")], &[("A1", DF)]);
        assert_eq!(e.values[&(0, 0)], Value::Error("SPILL".into()));
        assert!(e.blocks.is_empty() && e.values[&(1, 1)] == Value::Text("typed".into()));
        // B1:C3 and A3:C5 overlap at B3:C3, though neither covers the other's formula.
        let e = shell_sheet(&[("B1", "=SH(\"a\", :C)"), ("A3", "=SH(\"b\")")], &[("B1", DF), ("A3", DF)]);
        assert_eq!(e.values[&(0, 1)], Value::Text("Filesystem".into()));
        assert_eq!(e.values[&(2, 0)], Value::Error("SPILL".into()), "the later formula gives way");
        // A block covering another formula's cell gives way too.
        let e = shell_sheet(&[("A1", "=SH(\"a\")"), ("B2", "=SH(\"b\")")], &[("A1", DF), ("B2", DF)]);
        assert_eq!(e.values[&(0, 0)], Value::Error("SPILL".into()));
    }

    #[test]
    fn shell_cells_say_why_they_have_no_value() {
        let e = shell_sheet(&[("A1", "=SH(\"x\")")], &[]);
        assert_eq!(e.values[&(0, 0)], Value::Error("NORUN".into()));
        // A result for an earlier version of the formula doesn't count.
        let map: BTreeMap<Pos, String> = [((0, 0), "=SH(\"new\")".to_string())].into();
        let mut state = ShellState::default();
        state.runs.insert((0, 0), run("=SH(\"old\")", "1\n"));
        assert_eq!(Evaluator::with_shell(&map, &state).value((0, 0)), Value::Error("NORUN".into()));
        state.runs.insert((0, 0), run("=SH(\"new\")", "1\n"));
        state.runs.get_mut(&(0, 0)).unwrap().status = Status::Exit(2);
        assert_eq!(Evaluator::with_shell(&map, &state).value((0, 0)), Value::Error("EXIT".into()));
        state.runs.get_mut(&(0, 0)).unwrap().status = Status::Ok;
        state.pending.insert((0, 0));
        assert_eq!(Evaluator::with_shell(&map, &state).value((0, 0)), Value::Pending);
        let e = shell_sheet(&[("B2", "=SH(\"x\", A1:C3)")], &[]);
        assert_eq!(e.values[&(1, 1)], Value::Error("RANGE".into()), "the range starts at the formula's cell");
    }

    #[test]
    fn jobs_pass_cells_as_arguments() {
        let map: BTreeMap<Pos, String> = [
            ((0, 0), "a b; rm".to_string()),
            ((0, 1), "=SH(\"echo \"\"$1\"\" \"&\"x\", B1:C, arg=A1, arg=2*3, timeout=2, sep=\",\", rest=drop)".to_string()),
        ]
        .into();
        let job = Evaluator::new(&map).shell_job((0, 1)).unwrap();
        assert_eq!(job.command, "echo \"$1\" x");
        assert_eq!(job.args, ["a b; rm", "6"]);
        assert_eq!(job.timeout, Duration::from_secs(2));
        assert!(Evaluator::new(&map).shell_job((0, 0)).is_err());
    }

    #[test]
    fn shell_formulas_parse_their_ranges_and_options() {
        let args = shell_call("SH(\"c\", C3:F, skip=1, rest=drop)").unwrap();
        let spec = shell_spec((2, 2), &args).unwrap();
        assert_eq!(spec.target, Target { end_col: Some(5), end_row: None });
        assert_eq!((spec.options.skip, spec.options.keep_rest), (1, false));
        let spec = shell_spec((0, 0), &shell_call("SH(\"c\", :B20)").unwrap()).unwrap();
        assert_eq!(spec.target, Target { end_col: Some(1), end_row: Some(19) });
        assert!(shell_spec((0, 0), &shell_call("SH(\"c\", bogus=1)").unwrap()).is_err());
        assert!(shell_spec((5, 5), &shell_call("SH(\"c\", :B2)").unwrap()).is_err(), "can't end before it starts");
    }
}
