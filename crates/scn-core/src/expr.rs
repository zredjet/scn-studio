//! 条件式と代入式の字句解析と構文解析（仕様書14節）。
//!
//! 名前空間の補完（`勇気` → `flag.勇気`）とキャラクターIDへの正規化は、
//! 設定DBを使う `sema` で行う。ここでは書かれたとおりの形を返す。

use crate::diag::{Code, Diagnostic, Span};
use crate::line::{Piece, is_space};

/// 入れ子の深さの上限。極端な入力でスタックを使い切らないようにする。
const MAX_DEPTH: usize = 64;

/// 式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Int(i64),
    Str(String),
    Bool(bool),
    Var(VarRef),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

/// 変数名。`namespace` は書かれていなければ `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarRef {
    pub namespace: Option<String>,
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-`
    Neg,
    /// `not`
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
}

impl BinaryOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinaryOp::Or => "or",
            BinaryOp::And => "and",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Le => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::Ge => ">=",
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
        }
    }
}

/// 代入（`@設定`）の演算子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    /// `=`
    Set,
    /// `+=`
    Add,
    /// `-=`
    Sub,
}

impl AssignOp {
    pub fn as_str(self) -> &'static str {
        match self {
            AssignOp::Set => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
        }
    }
}

/// 代入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub target: VarRef,
    pub op: AssignOp,
    pub value: Expr,
}

/// 書かれた式の文字列と、解析の結果。構文が崩れていれば `expr` は `None`（E601）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceExpr {
    pub text: String,
    pub span: Span,
    pub expr: Option<Expr>,
}

/// 条件式を解析する。誤りがあれば E601 を1つ出す。
pub(crate) fn parse_condition(source: Piece<'_>, diags: &mut Vec<Diagnostic>) -> SourceExpr {
    let expr = run(source, diags, |p| {
        let expr = p.expr()?;
        p.expect_end()?;
        Ok(expr)
    });
    SourceExpr {
        text: source.text.to_owned(),
        span: source.span(),
        expr,
    }
}

/// 代入式（`変数名 (= | += | -=) 式`）を解析する。誤りがあれば E601 を1つ出す。
pub(crate) fn parse_assignment(
    source: Piece<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<Assignment> {
    run(source, diags, |p| {
        let target = p.var_ref()?;
        let op = match p.peek().tok {
            Tok::Assign => AssignOp::Set,
            Tok::PlusAssign => AssignOp::Add,
            Tok::MinusAssign => AssignOp::Sub,
            _ => return Err(p.unexpected("代入の演算子（=、+=、-=）")),
        };
        p.bump();
        let value = p.expr()?;
        p.expect_end()?;
        Ok(Assignment { target, op, value })
    })
}

fn run<T>(
    source: Piece<'_>,
    diags: &mut Vec<Diagnostic>,
    f: impl FnOnce(&mut Parser) -> Result<T, ExprError>,
) -> Option<T> {
    let result = tokenize(source).and_then(|tokens| {
        let mut parser = Parser {
            tokens,
            pos: 0,
            depth: 0,
            end: source.end(),
        };
        f(&mut parser)
    });
    match result {
        Ok(value) => Some(value),
        Err(err) => {
            diags.push(Diagnostic::new(
                Code::E601,
                err.span,
                format!("式の構文エラー：{}", err.message),
            ));
            None
        }
    }
}

#[derive(Debug)]
struct ExprError {
    span: Span,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Int(i64),
    Str(String),
    Name(String),
    True,
    False,
    And,
    Or,
    Not,
    Dot,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Assign,
    PlusAssign,
    MinusAssign,
    End,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Int(n) => n.to_string(),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Name(n) => n.clone(),
            Tok::True => "true".into(),
            Tok::False => "false".into(),
            Tok::And => "and".into(),
            Tok::Or => "or".into(),
            Tok::Not => "not".into(),
            Tok::Dot => ".".into(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::Plus => "+".into(),
            Tok::Minus => "-".into(),
            Tok::Star => "*".into(),
            Tok::Slash => "/".into(),
            Tok::EqEq => "==".into(),
            Tok::Ne => "!=".into(),
            Tok::Lt => "<".into(),
            Tok::Le => "<=".into(),
            Tok::Gt => ">".into(),
            Tok::Ge => ">=".into(),
            Tok::Assign => "=".into(),
            Tok::PlusAssign => "+=".into(),
            Tok::MinusAssign => "-=".into(),
            Tok::End => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    span: Span,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_name_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tokenize(source: Piece<'_>) -> Result<Vec<Token>, ExprError> {
    let text = source.text;
    let base = source.start;
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if is_space(c) {
            continue;
        }
        let start = base + at;
        let next = chars.peek().map(|&(_, n)| n);
        let simple = |tok: Tok, len: usize| Token {
            tok,
            span: Span::new(start, start + len),
        };
        let token = match (c, next) {
            ('(', _) => simple(Tok::LParen, 1),
            (')', _) => simple(Tok::RParen, 1),
            ('.', _) => simple(Tok::Dot, 1),
            ('*', _) => simple(Tok::Star, 1),
            ('/', _) => simple(Tok::Slash, 1),
            ('+', Some('=')) => {
                chars.next();
                simple(Tok::PlusAssign, 2)
            }
            ('+', _) => simple(Tok::Plus, 1),
            ('-', Some('=')) => {
                chars.next();
                simple(Tok::MinusAssign, 2)
            }
            ('-', _) => simple(Tok::Minus, 1),
            ('=', Some('=')) => {
                chars.next();
                simple(Tok::EqEq, 2)
            }
            ('=', _) => simple(Tok::Assign, 1),
            ('!', Some('=')) => {
                chars.next();
                simple(Tok::Ne, 2)
            }
            ('<', Some('=')) => {
                chars.next();
                simple(Tok::Le, 2)
            }
            ('<', _) => simple(Tok::Lt, 1),
            ('>', Some('=')) => {
                chars.next();
                simple(Tok::Ge, 2)
            }
            ('>', _) => simple(Tok::Gt, 1),
            ('"', _) => {
                let mut value = String::new();
                let mut end = None;
                while let Some((i, ch)) = chars.next() {
                    match ch {
                        '"' => {
                            end = Some(i + 1);
                            break;
                        }
                        '\\' if chars.peek().map(|&(_, n)| n) == Some('"') => {
                            chars.next();
                            value.push('"');
                        }
                        _ => value.push(ch),
                    }
                }
                let Some(end) = end else {
                    return Err(ExprError {
                        span: Span::new(start, source.end()),
                        message: "文字列の \" が閉じていません".into(),
                    });
                };
                Token {
                    tok: Tok::Str(value),
                    span: Span::new(start, base + end),
                }
            }
            (c, _) if c.is_ascii_digit() => {
                let mut end = at + c.len_utf8();
                while let Some(&(i, d)) = chars.peek() {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    end = i + d.len_utf8();
                    chars.next();
                }
                if let Some(&(i, n)) = chars.peek()
                    && is_name_continue(n)
                {
                    let mut name_end = i + n.len_utf8();
                    chars.next();
                    while let Some(&(j, m)) = chars.peek() {
                        if !is_name_continue(m) {
                            break;
                        }
                        name_end = j + m.len_utf8();
                        chars.next();
                    }
                    return Err(ExprError {
                        span: Span::new(start, base + name_end),
                        message: "名前は数字で始められません".into(),
                    });
                }
                let digits = text.get(at..end).unwrap_or_default();
                let Ok(value) = digits.parse::<i64>() else {
                    return Err(ExprError {
                        span: Span::new(start, base + end),
                        message: "整数が大きすぎます".into(),
                    });
                };
                Token {
                    tok: Tok::Int(value),
                    span: Span::new(start, base + end),
                }
            }
            (c, _) if is_name_start(c) => {
                let mut end = at + c.len_utf8();
                while let Some(&(i, n)) = chars.peek() {
                    if !is_name_continue(n) {
                        break;
                    }
                    end = i + n.len_utf8();
                    chars.next();
                }
                let word = text.get(at..end).unwrap_or_default();
                let tok = match word {
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    _ => Tok::Name(word.to_owned()),
                };
                Token {
                    tok,
                    span: Span::new(start, base + end),
                }
            }
            (c, _) => {
                return Err(ExprError {
                    span: Span::new(start, start + c.len_utf8()),
                    message: format!("式に使えない文字です：{c}（演算子と記号は半角で書きます）"),
                });
            }
        };
        tokens.push(token);
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token {
            tok: Tok::End,
            span: Span::new(self.end, self.end),
        })
    }

    fn bump(&mut self) -> Token {
        let token = self.peek();
        self.pos += 1;
        token
    }

    fn unexpected(&self, expected: &str) -> ExprError {
        let token = self.peek();
        let message = if token.tok == Tok::End {
            format!("{expected}がありません")
        } else {
            format!(
                "{expected}が必要ですが、{} があります",
                token.tok.describe()
            )
        };
        ExprError {
            span: token.span,
            message,
        }
    }

    fn expect_end(&self) -> Result<(), ExprError> {
        let token = self.peek();
        if token.tok == Tok::End {
            return Ok(());
        }
        let message = if comparison_op(&token.tok).is_some() {
            "比較は1つの式に1回だけ書けます".to_owned()
        } else {
            format!("式の後ろに余分な {} があります", token.tok.describe())
        };
        Err(ExprError {
            span: token.span,
            message,
        })
    }

    fn enter(&mut self, span: Span) -> Result<(), ExprError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(ExprError {
                span,
                message: "式の入れ子が深すぎます".into(),
            });
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Expr, ExprError> {
        self.or()
    }

    fn binary_chain(
        &mut self,
        next: fn(&mut Parser) -> Result<Expr, ExprError>,
        op_of: fn(&Tok) -> Option<BinaryOp>,
    ) -> Result<Expr, ExprError> {
        let mut lhs = next(self)?;
        while let Some(op) = op_of(&self.peek().tok) {
            self.bump();
            let rhs = next(self)?;
            let span = lhs.span.cover(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn or(&mut self) -> Result<Expr, ExprError> {
        self.binary_chain(Parser::and, |t| (*t == Tok::Or).then_some(BinaryOp::Or))
    }

    fn and(&mut self) -> Result<Expr, ExprError> {
        self.binary_chain(Parser::not, |t| (*t == Tok::And).then_some(BinaryOp::And))
    }

    fn not(&mut self) -> Result<Expr, ExprError> {
        let token = self.peek();
        if token.tok != Tok::Not {
            return self.comparison();
        }
        self.bump();
        self.enter(token.span)?;
        let operand = self.not()?;
        self.depth -= 1;
        let span = token.span.cover(operand.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            },
            span,
        })
    }

    fn comparison(&mut self) -> Result<Expr, ExprError> {
        let lhs = self.additive()?;
        let Some(op) = comparison_op(&self.peek().tok) else {
            return Ok(lhs);
        };
        self.bump();
        let rhs = self.additive()?;
        let span = lhs.span.cover(rhs.span);
        Ok(Expr {
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            span,
        })
    }

    fn additive(&mut self) -> Result<Expr, ExprError> {
        self.binary_chain(Parser::multiplicative, |t| match t {
            Tok::Plus => Some(BinaryOp::Add),
            Tok::Minus => Some(BinaryOp::Sub),
            _ => None,
        })
    }

    fn multiplicative(&mut self) -> Result<Expr, ExprError> {
        self.binary_chain(Parser::unary, |t| match t {
            Tok::Star => Some(BinaryOp::Mul),
            Tok::Slash => Some(BinaryOp::Div),
            _ => None,
        })
    }

    fn unary(&mut self) -> Result<Expr, ExprError> {
        let token = self.peek();
        if token.tok != Tok::Minus {
            return self.primary();
        }
        self.bump();
        self.enter(token.span)?;
        let operand = self.unary()?;
        self.depth -= 1;
        let span = token.span.cover(operand.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            },
            span,
        })
    }

    fn primary(&mut self) -> Result<Expr, ExprError> {
        let token = self.peek();
        let kind = match token.tok {
            Tok::Int(value) => ExprKind::Int(value),
            Tok::Str(ref value) => ExprKind::Str(value.clone()),
            Tok::True => ExprKind::Bool(true),
            Tok::False => ExprKind::Bool(false),
            Tok::Name(_) => {
                let var = self.var_ref()?;
                let span = var.span;
                return Ok(Expr {
                    kind: ExprKind::Var(var),
                    span,
                });
            }
            Tok::LParen => {
                self.bump();
                self.enter(token.span)?;
                let inner = self.expr()?;
                self.depth -= 1;
                let close = self.peek();
                if close.tok != Tok::RParen {
                    return Err(ExprError {
                        span: token.span,
                        message: "( に対応する ) がありません".into(),
                    });
                }
                self.bump();
                return Ok(inner);
            }
            Tok::And | Tok::Or | Tok::Not => {
                return Err(ExprError {
                    span: token.span,
                    message: format!("{} は名前に使えません", token.tok.describe()),
                });
            }
            _ => return Err(self.unexpected("値か変数")),
        };
        self.bump();
        Ok(Expr {
            kind,
            span: token.span,
        })
    }

    fn name(&mut self) -> Result<(String, Span), ExprError> {
        let token = self.peek();
        match token.tok {
            Tok::Name(name) => {
                self.bump();
                Ok((name, token.span))
            }
            Tok::And | Tok::Or | Tok::Not | Tok::True | Tok::False => Err(ExprError {
                span: token.span,
                message: format!("{} は名前に使えません", token.tok.describe()),
            }),
            _ => Err(self.unexpected("変数名")),
        }
    }

    fn var_ref(&mut self) -> Result<VarRef, ExprError> {
        let (first, first_span) = self.name()?;
        if self.peek().tok != Tok::Dot {
            return Ok(VarRef {
                namespace: None,
                name: first,
                span: first_span,
            });
        }
        self.bump();
        let (name, name_span) = self.name()?;
        Ok(VarRef {
            namespace: Some(first),
            name,
            span: first_span.cover(name_span),
        })
    }
}

fn comparison_op(tok: &Tok) -> Option<BinaryOp> {
    match tok {
        Tok::EqEq => Some(BinaryOp::Eq),
        Tok::Ne => Some(BinaryOp::Ne),
        Tok::Lt => Some(BinaryOp::Lt),
        Tok::Le => Some(BinaryOp::Le),
        Tok::Gt => Some(BinaryOp::Gt),
        Tok::Ge => Some(BinaryOp::Ge),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sexpr(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(n) => n.to_string(),
            ExprKind::Str(s) => format!("{s:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Var(v) => match &v.namespace {
                Some(ns) => format!("{ns}.{}", v.name),
                None => v.name.clone(),
            },
            ExprKind::Unary { op, operand } => {
                let op = match op {
                    UnaryOp::Neg => "neg",
                    UnaryOp::Not => "not",
                };
                format!("({op} {})", sexpr(operand))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                format!("({} {} {})", op.as_str(), sexpr(lhs), sexpr(rhs))
            }
        }
    }

    fn cond(text: &str) -> (Option<String>, Vec<Diagnostic>) {
        let mut diags = Vec::new();
        let parsed = parse_condition(Piece::new(text, 0), &mut diags);
        (parsed.expr.as_ref().map(sexpr), diags)
    }

    #[test]
    fn precedence() {
        assert_eq!(
            cond("not a >= 2 and b < 3 or c == \"灯\"").0.as_deref(),
            Some("(or (and (not (>= a 2)) (< b 3)) (== c \"灯\"))")
        );
        assert_eq!(
            cond("1 + 2 * 3 - 4 / 2").0.as_deref(),
            Some("(- (+ 1 (* 2 3)) (/ 4 2))")
        );
        assert_eq!(
            cond("(1 + 2) * -x").0.as_deref(),
            Some("(* (+ 1 2) (neg x))")
        );
        assert_eq!(
            cond("灯.好感度　>=　3").0.as_deref(),
            Some("(>= 灯.好感度 3)")
        );
        assert_eq!(cond("\"a\\\"b\"").0.as_deref(), Some("\"a\\\"b\""));
    }

    #[test]
    fn errors() {
        for text in [
            "",
            "(a",
            "1日目",
            "and == 1",
            "a.and",
            "”灯”",
            "２",
            "a < b < c",
            "a.b.c",
            "\"open",
            "99999999999999999999",
            "a = 1",
        ] {
            let (expr, diags) = cond(text);
            assert!(expr.is_none(), "{text} が通ってしまう");
            assert_eq!(diags.len(), 1, "{text}");
            assert_eq!(diags[0].code, Code::E601);
        }
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let text = format!("{}1{}", "(".repeat(1000), ")".repeat(1000));
        assert!(cond(&text).0.is_none());
        let text = format!("{}1", "not ".repeat(1000));
        assert!(cond(&text).0.is_none());
    }

    #[test]
    fn assignment() {
        let mut diags = Vec::new();
        let a = parse_assignment(Piece::new("灯.好感度 += 1", 0), &mut diags).unwrap();
        assert_eq!(a.op, AssignOp::Add);
        assert_eq!(a.target.namespace.as_deref(), Some("灯"));
        assert!(diags.is_empty());
        assert!(parse_assignment(Piece::new("勇気 == 1", 0), &mut diags).is_none());
        assert_eq!(diags.len(), 1);
    }
}
