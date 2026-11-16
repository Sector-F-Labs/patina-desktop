//! A calculator for the search box: typing `1 + 5` or `sqrt(2) * 3` gives
//! the answer as the first row. Numbers, `+ - * / % ^`, parentheses, the
//! constants `pi` and `e`, and a few functions.

/// The answer to `query` if it is arithmetic, formatted for showing and
/// copying. A bare number isn't: there's nothing to work out, and typing
/// "7zip" shouldn't offer "7".
pub fn answer(query: &str) -> Option<String> {
    let mut parser = Parser {
        chars: query.chars().collect(),
        at: 0,
        worked: false,
    };
    let value = parser.sum()?;
    parser.space();
    if parser.at != parser.chars.len() || !parser.worked || !value.is_finite() {
        return None;
    }
    Some(format(value))
}

fn format(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    // Ten significant digits, without the noise of 0.1 + 0.2.
    let digits = 10 - 1 - value.abs().log10().floor() as i32;
    let text = if (0..=15).contains(&digits) {
        format!("{value:.*}", digits as usize)
    } else {
        format!("{value:.9e}")
    };
    if text.contains('e') {
        return text;
    }
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct Parser {
    chars: Vec<char>,
    at: usize,
    /// Whether an operator, function or constant was used.
    worked: bool,
}

impl Parser {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.space();
        if self.chars.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// `a + b - c`
    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat('+') {
                self.worked = true;
                value += self.product()?;
            } else if self.eat('-') {
                self.worked = true;
                value -= self.product()?;
            } else {
                return Some(value);
            }
        }
    }

    /// `a * b / c % d`, with `x` and `×` for times and `÷` for divide.
    fn product(&mut self) -> Option<f64> {
        let mut value = self.unary()?;
        loop {
            if self.eat('*') || self.eat('×') || self.eat('x') {
                self.worked = true;
                value *= self.unary()?;
            } else if self.eat('/') || self.eat('÷') {
                self.worked = true;
                value /= self.unary()?;
            } else if self.eat('%') {
                self.worked = true;
                value %= self.unary()?;
            } else {
                return Some(value);
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            self.worked = true;
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    /// `a ^ b`, right to left: 2^3^2 is 2^9.
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat('^') {
            self.worked = true;
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        self.space();
        let c = *self.chars.get(self.at)?;
        if c == '(' {
            self.at += 1;
            let value = self.sum()?;
            return self.eat(')').then_some(value);
        }
        if c.is_ascii_digit() || c == '.' {
            let start = self.at;
            while self
                .chars
                .get(self.at)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                self.at += 1;
            }
            let text: String = self.chars[start..self.at].iter().collect();
            return text.parse().ok();
        }
        if c.is_alphabetic() {
            let start = self.at;
            while self.chars.get(self.at).is_some_and(|c| c.is_alphabetic()) {
                self.at += 1;
            }
            let name: String = self.chars[start..self.at].iter().collect();
            let name = name.to_lowercase();
            match name.as_str() {
                "pi" | "π" => return Some(std::f64::consts::PI),
                "e" => return Some(std::f64::consts::E),
                _ => {}
            }
            if !self.eat('(') {
                return None;
            }
            let arg = self.sum()?;
            if !self.eat(')') {
                return None;
            }
            self.worked = true;
            return match name.as_str() {
                "sqrt" => Some(arg.sqrt()),
                "abs" => Some(arg.abs()),
                "sin" => Some(arg.sin()),
                "cos" => Some(arg.cos()),
                "tan" => Some(arg.tan()),
                "ln" => Some(arg.ln()),
                "log" => Some(arg.log10()),
                "round" => Some(arg.round()),
                "floor" => Some(arg.floor()),
                "ceil" => Some(arg.ceil()),
                _ => None,
            };
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn works_out_arithmetic() {
        assert_eq!(answer("1 + 5").as_deref(), Some("6"));
        assert_eq!(answer("2+3*4").as_deref(), Some("14"));
        assert_eq!(answer("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(answer("10 / 4").as_deref(), Some("2.5"));
        assert_eq!(answer("2^3^2").as_deref(), Some("512"));
        assert_eq!(answer("-3 + 5").as_deref(), Some("2"));
        assert_eq!(answer("7 % 4").as_deref(), Some("3"));
        assert_eq!(answer("3 x 4").as_deref(), Some("12"));
    }

    #[test]
    fn rounds_away_float_noise() {
        assert_eq!(answer("0.1 + 0.2").as_deref(), Some("0.3"));
        assert_eq!(answer("1/3").as_deref(), Some("0.3333333333"));
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(answer("sqrt(16)").as_deref(), Some("4"));
        assert_eq!(answer("pi * 2").as_deref(), Some("6.283185307"));
        assert_eq!(answer("abs(-2) + 1").as_deref(), Some("3"));
    }

    #[test]
    fn what_isnt_arithmetic_is_left_to_the_apps() {
        for query in ["", "5", "7zip", "firefox", "1 +", "(1 + 2", "1 / 0", "e", "pi", "x", "2 x"] {
            assert_eq!(answer(query), None, "{query:?}");
        }
    }
}
