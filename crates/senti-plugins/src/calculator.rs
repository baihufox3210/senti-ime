//! `v`-mode calculator: typing `v1+2*3` in Chinese mode offers `7`.

use senti_core::{Candidate, CandidateSource, InputMode, Plugin, PluginManifest, PredictionQuery};

const ID: &str = "calculator";
const PREFIX: char = 'v';

#[derive(Debug, Clone, Copy, Default)]
pub struct CalculatorPlugin;

impl Plugin for CalculatorPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: ID,
            name: "計算機",
            description: "中文模式下以 v 開頭輸入算式（如 v(1+2)*3），直接輸出計算結果。",
            enabled_by_default: true,
        }
    }

    fn suggest(&self, query: &PredictionQuery<'_>) -> Vec<Candidate> {
        if query.mode != InputMode::Chinese {
            return Vec::new();
        }
        let Some(expression) = query.input.strip_prefix(PREFIX) else {
            return Vec::new();
        };
        if !expression.chars().any(|c| c.is_ascii_digit()) {
            return Vec::new();
        }
        let Ok(value) = evaluate(expression) else {
            return Vec::new();
        };
        let result = format_number(value);
        let source = CandidateSource::Plugin(ID.to_owned());
        vec![
            Candidate::new(&result, query.input.len(), 2.0, source.clone()),
            Candidate::new(
                format!("{expression}={result}"),
                query.input.len(),
                1.99,
                source,
            ),
        ]
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.10}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Evaluate an arithmetic expression with `+ - * / % ^`, parentheses,
/// unary minus and decimals.
pub fn evaluate(expression: &str) -> Result<f64, String> {
    let tokens: Vec<char> = expression.chars().filter(|c| !c.is_whitespace()).collect();
    let mut parser = Parser { tokens, pos: 0 };
    let value = parser.expr()?;
    if parser.pos != parser.tokens.len() {
        return Err(format!("unexpected `{}`", parser.tokens[parser.pos]));
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err("result is not a finite number".into())
    }
}

/// Recursive-descent parser:
/// `expr := term (('+'|'-') term)*`, `term := power (('*'|'/'|'%') power)*`,
/// `power := unary ('^' power)?`, `unary := '-' unary | atom`,
/// `atom := number | '(' expr ')'`.
struct Parser {
    tokens: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.tokens.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let matched = self.peek() == Some(c);
        if matched {
            self.pos += 1;
        }
        matched
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut value = self.term()?;
        loop {
            if self.eat('+') {
                value += self.term()?;
            } else if self.eat('-') {
                value -= self.term()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut value = self.power()?;
        loop {
            if self.eat('*') {
                value *= self.power()?;
            } else if self.eat('/') {
                let divisor = self.power()?;
                if divisor == 0.0 {
                    return Err("division by zero".into());
                }
                value /= divisor;
            } else if self.eat('%') {
                let divisor = self.power()?;
                if divisor == 0.0 {
                    return Err("division by zero".into());
                }
                value %= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.unary()?;
        if self.eat('^') {
            Ok(base.powf(self.power()?))
        } else {
            Ok(base)
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat('-') {
            Ok(-self.unary()?)
        } else {
            self.atom()
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        if self.eat('(') {
            let value = self.expr()?;
            return if self.eat(')') {
                Ok(value)
            } else {
                Err("missing `)`".into())
            };
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.pos += 1;
        }
        if start == self.pos {
            return Err(match self.peek() {
                Some(c) => format!("unexpected `{c}`"),
                None => "unexpected end of expression".into(),
            });
        }
        let literal: String = self.tokens[start..self.pos].iter().collect();
        literal
            .parse()
            .map_err(|_| format!("invalid number `{literal}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_query;
    use senti_core::InputContext;

    #[test]
    fn evaluates_with_precedence() {
        assert_eq!(evaluate("1+2*3"), Ok(7.0));
        assert_eq!(evaluate("(1+2)*3"), Ok(9.0));
        assert_eq!(evaluate("2^3^2"), Ok(512.0));
        assert_eq!(evaluate("-3+10%4"), Ok(-1.0));
        assert_eq!(evaluate("1.5*2"), Ok(3.0));
    }

    #[test]
    fn reports_errors() {
        assert!(evaluate("1/0").is_err());
        assert!(evaluate("(1+2").is_err());
        assert!(evaluate("1+").is_err());
        assert!(evaluate("1..2").is_err());
        assert!(evaluate("2)").is_err());
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(7.0), "7");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(-2.5), "-2.5");
    }

    #[test]
    fn suggests_only_for_v_prefixed_chinese_input() {
        let run = |mode, input: &str| {
            with_query(input, mode, InputContext::default(), |q| {
                CalculatorPlugin
                    .suggest(q)
                    .into_iter()
                    .map(|c| c.text)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(run(InputMode::Chinese, "v1+2*3"), ["7", "1+2*3=7"]);
        assert!(run(InputMode::Chinese, "very").is_empty());
        assert!(run(InputMode::English, "v1+1").is_empty());
        assert!(run(InputMode::Chinese, "v1+").is_empty());
    }
}
