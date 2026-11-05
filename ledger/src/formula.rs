//! Cell values and formulas: `=A1+B2*3`, `=SUM(B2:B9)`, `=ROUND(C1/3, 2)`.
//!
//! A cell's *input* is the text the user typed. If it starts with `=` it's a
//! formula; if it reads as a number it's a number; otherwise it's text. The
//! input is what's stored (ADR-0002); values are worked out from it.

use std::collections::{BTreeMap, HashMap, HashSet};

/// A cell position, zero-based: `(row, col)`. `A1` is `(0, 0)`.
pub type Pos = (u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Number(f64),
    Text(String),
    Error(String),
}

impl Value {
    /// What a cell shows.
    pub fn display(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => format_number(*n),
            Value::Text(t) => t.clone(),
            Value::Error(e) => format!("#{e}"),
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
            Some(Tok::Ident(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.next();
                    } else {
                        loop {
                            args.push(self.expr()?);
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
                            let b = parse_ref(&n2).ok_or_else(|| "NAME".to_string())?;
                            return Ok(Expr::Range(a, b));
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

/// Works out the values of a sheet's cells from their inputs, each once.
pub struct Evaluator<'a> {
    inputs: &'a BTreeMap<Pos, String>,
    done: HashMap<Pos, Value>,
    visiting: HashSet<Pos>,
}

impl<'a> Evaluator<'a> {
    pub fn new(inputs: &'a BTreeMap<Pos, String>) -> Self {
        Self { inputs, done: HashMap::new(), visiting: HashSet::new() }
    }

    /// The value of every non-empty cell.
    pub fn all(mut self) -> HashMap<Pos, Value> {
        let keys: Vec<Pos> = self.inputs.keys().copied().collect();
        for k in keys {
            self.value(k);
        }
        self.done
    }

    pub fn value(&mut self, pos: Pos) -> Value {
        if let Some(v) = self.done.get(&pos) {
            return v.clone();
        }
        let Some(input) = self.inputs.get(&pos) else { return Value::Empty };
        let value = match input.strip_prefix('=') {
            None => parse_input(input),
            Some(src) => {
                if !self.visiting.insert(pos) {
                    return Value::Error("CYCLE".into());
                }
                let v = match parse_formula(src) {
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

    fn number(&mut self, e: &Expr) -> Result<f64, Value> {
        match self.eval(e) {
            Value::Number(n) => Ok(n),
            Value::Empty => Ok(0.0),
            Value::Text(t) => match parse_input(&t) {
                Value::Number(n) => Ok(n),
                _ => Err(Value::Error("VALUE".into())),
            },
            err @ Value::Error(_) => Err(err),
        }
    }

    fn eval(&mut self, e: &Expr) -> Value {
        match e {
            Expr::Num(n) => Value::Number(*n),
            Expr::Str(s) => Value::Text(s.clone()),
            Expr::Ref(p) => self.value(*p),
            Expr::Range(..) => Value::Error("VALUE".into()),
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
                            if !self.inputs.contains_key(&(r, c)) {
                                continue;
                            }
                            match self.value((r, c)) {
                                Value::Number(n) => out.push(n),
                                err @ Value::Error(_) => return Err(err),
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
}
