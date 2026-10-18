use crate::model::{AnswerFormat, Question};

// Deliberately numeric, not a symbolic algebra system. No variables, execution,
// implicit multiplication or unbounded recursion; unsupported expressions fail closed.
pub fn numeric(input: &str) -> Option<f64> {
    if input.len() > 512 {
        return None;
    }
    let text = input
        .trim()
        .replace('π', "pi")
        .replace('−', "-")
        .replace('×', "*")
        .replace('÷', "/");
    let mut p = Parser {
        bytes: text.as_bytes(),
        at: 0,
        depth: 0,
        ops: 0,
    };
    let n = p.expr()?;
    p.space();
    (p.at == p.bytes.len() && n.is_finite() && n.abs() <= 1e100).then_some(n)
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: u32,
    ops: u32,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1
        }
    }
    fn eat(&mut self, b: u8) -> bool {
        self.space();
        if self.bytes.get(self.at) == Some(&b) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Option<f64> {
        let mut n = self.term()?;
        loop {
            if self.eat(b'+') {
                n += self.term()?
            } else if self.eat(b'-') {
                n -= self.term()?
            } else {
                break;
            }
            self.count()?;
        }
        Some(n)
    }
    fn term(&mut self) -> Option<f64> {
        let mut n = self.unary()?;
        loop {
            if self.eat(b'*') {
                n *= self.unary()?
            } else if self.eat(b'/') {
                let d = self.unary()?;
                if d == 0.0 {
                    return None;
                }
                n /= d;
            } else {
                break;
            }
            self.count()?;
        }
        Some(n)
    }
    fn count(&mut self) -> Option<()> {
        self.ops += 1;
        (self.ops <= 128).then_some(())
    }
    fn unary(&mut self) -> Option<f64> {
        self.depth += 1;
        if self.depth > 24 {
            return None;
        }
        let n = if self.eat(b'+') {
            self.unary()?
        } else if self.eat(b'-') {
            -self.unary()?
        } else {
            self.power()?
        };
        self.depth -= 1;
        Some(n)
    }
    fn power(&mut self) -> Option<f64> {
        let mut n = self.atom()?;
        if self.eat(b'^') {
            let exponent = self.unary()?;
            if exponent.abs() > 1000.0 {
                return None;
            }
            n = n.powf(exponent);
            self.count()?;
        }
        n.is_finite().then_some(n)
    }
    fn atom(&mut self) -> Option<f64> {
        self.space();
        if self.eat(b'(') {
            let n = self.expr()?;
            return self.eat(b')').then_some(n);
        }
        if self.bytes[self.at..].starts_with(b"sqrt") {
            self.at += 4;
            if !self.eat(b'(') {
                return None;
            }
            let n = self.expr()?;
            return (self.eat(b')') && n >= 0.0).then(|| n.sqrt());
        }
        if self.bytes[self.at..].starts_with(b"pi") {
            self.at += 2;
            return Some(std::f64::consts::PI);
        }
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
        {
            self.at += 1
        }
        if self
            .bytes
            .get(self.at)
            .is_some_and(|b| *b == b'e' || *b == b'E')
        {
            self.at += 1;
            if self
                .bytes
                .get(self.at)
                .is_some_and(|b| *b == b'+' || *b == b'-')
            {
                self.at += 1
            }
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1
            }
        }
        if start == self.at {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse::<f64>()
            .ok()
    }
}
pub fn equivalent(a: &str, b: &str) -> bool {
    match (numeric(a), numeric(b)) {
        (Some(a), Some(b)) => (a - b).abs() <= 1e-9_f64.max(1e-8 * a.abs().max(b.abs())),
        _ => false,
    }
}
pub fn correct(q: &Question, answer: &str) -> Option<bool> {
    match q.answer_format {
        AnswerFormat::SelfAssessed => None,
        AnswerFormat::Choice => Some(answer == q.answer),
        AnswerFormat::Text => {
            let mut answers = std::iter::once(&q.answer).chain(q.accepted_answers.iter());
            if numeric(&q.answer).is_some() {
                Some(answers.any(|expected| equivalent(expected, answer)))
            } else {
                Some(answers.any(|expected| expected.trim().eq_ignore_ascii_case(answer.trim())))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_math() {
        assert!(equivalent("1/2", "0.5"));
        assert!(equivalent("2*pi", "6.283185307179586"));
        assert!(equivalent("sqrt(9)+2^3", "11"));
        assert!(equivalent("-2^2", "-4"));
        assert!(equivalent("2^-2", "1/4"));
    }
    #[test]
    fn parser_fails_closed() {
        for x in [
            "x+x",
            "std::process::exit(0)",
            "1/0",
            "sqrt(-1)",
            "NaN",
            "2(3)",
            "1e999",
            "2^99999",
        ] {
            assert!(numeric(x).is_none(), "{x}")
        }
        assert!(!equivalent("x+x", "2*x"));
        assert!(numeric(&"(".repeat(1000)).is_none());
    }
}
