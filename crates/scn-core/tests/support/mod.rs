//! テストの共通部品：ASTと診断を、人が読める形に書き出す。
//!
//! スナップショットは人が読んで確かめるので、1行に1つの要素を、行番号つきで書く。
//! 診断は `行:列-行:列`（列は文字数、1始まり）で位置を示す。

#![allow(dead_code)]

use std::fmt::Write as _;

use scn_core::ast::{
    Chapter, Choice, Dialogue, DirectiveKind, IfBlock, LineIdSlot, MetaValue, Scene, Stmt,
};
use scn_core::expr::{Assignment, Expr, ExprKind, SourceExpr, UnaryOp, VarRef};
use scn_core::inline::{Inline, Text};
use scn_core::{Diagnostic, Parsed, Span, Spanned};

/// バイト位置から、行（1始まり）と列（文字数、1始まり）を求める。
pub struct LineIndex<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut starts = vec![0];
        for (at, b) in source.bytes().enumerate() {
            if b == b'\n' {
                starts.push(at + 1);
            }
        }
        Self { source, starts }
    }

    pub fn line(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line(offset);
        let start = self.starts[line - 1];
        let col = self.source[start..offset].chars().count() + 1;
        (line, col)
    }

    pub fn range(&self, span: Span) -> String {
        let (l1, c1) = self.line_col(span.start);
        let (l2, c2) = self.line_col(span.end);
        format!("{l1}:{c1}-{l2}:{c2}")
    }
}

/// 解析結果を書き出す。
pub fn dump(source: &str, parsed: &Parsed) -> String {
    let index = LineIndex::new(source);
    let mut d = Dumper {
        index: &index,
        out: String::new(),
    };
    d.chapter(&parsed.chapter);
    d.out.push_str("\n# diagnostics\n");
    for diag in &parsed.diagnostics {
        d.diagnostic(diag);
    }
    d.out
}

/// 診断だけを書き出す。
pub fn dump_diagnostics(source: &str, diagnostics: &[Diagnostic]) -> String {
    let index = LineIndex::new(source);
    let mut d = Dumper {
        index: &index,
        out: String::new(),
    };
    for diag in diagnostics {
        d.diagnostic(diag);
    }
    d.out
}

struct Dumper<'a> {
    index: &'a LineIndex<'a>,
    out: String,
}

impl Dumper<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn at(&self, span: Span) -> String {
        format!("L{}", self.index.line(span.start))
    }

    fn diagnostic(&mut self, diag: &Diagnostic) {
        let line = format!(
            "{} {} {}",
            self.index.range(diag.span),
            diag.code,
            diag.message
        );
        self.line(0, &line);
    }

    fn chapter(&mut self, chapter: &Chapter) {
        if let Some(heading) = &chapter.heading {
            let line = format!(
                "chapter-heading {} {:?}",
                self.at(heading.span),
                heading.title
            );
            self.line(0, &line);
        }
        for scene in &chapter.scenes {
            self.scene(scene);
        }
    }

    fn scene(&mut self, scene: &Scene) {
        let id = match &scene.id {
            Some(id) => format!("#{}", id.value),
            None => "(no id)".to_owned(),
        };
        let line = format!("scene {} {id} {:?}", self.at(scene.span), scene.title);
        self.line(0, &line);
        if let Some(meta) = &scene.meta {
            let closed = if meta.closed { "closed" } else { "unclosed" };
            let line = format!("meta {} {closed}", self.at(meta.span));
            self.line(1, &line);
            for entry in &meta.entries {
                let value = match &entry.value.value {
                    MetaValue::Text(text) => format!("{text:?}"),
                    MetaValue::List(items) => format!("{items:?}"),
                };
                let line = format!("{}: {value}", entry.key.value);
                self.line(2, &line);
            }
        }
        self.stmts(1, &scene.body);
    }

    fn stmts(&mut self, depth: usize, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(depth, stmt);
        }
    }

    fn stmt(&mut self, depth: usize, stmt: &Stmt) {
        match stmt {
            Stmt::Dialogue(dialogue) => self.dialogue(depth, dialogue),
            Stmt::Narration(narration) => {
                let line = format!(
                    "narration {}{} {}",
                    self.at(narration.span),
                    line_id(&narration.line_id),
                    text(&narration.text)
                );
                self.line(depth, &line);
            }
            Stmt::Break(span) => {
                let line = format!("break {}", self.at(*span));
                self.line(depth, &line);
            }
            Stmt::Jump(jump) => {
                let line = format!("jump {} -> {:?}", self.at(jump.span), jump.target.value);
                self.line(depth, &line);
            }
            Stmt::Directive(directive) => {
                let line = format!(
                    "@{} {} {}",
                    directive.name,
                    self.at(directive.span),
                    directive_kind(&directive.kind)
                );
                self.line(depth, &line);
            }
            Stmt::Choice(choice) => self.choice(depth, choice),
            Stmt::If(block) => self.if_block(depth, block),
            Stmt::Raw(raw) => {
                let end = match raw.end {
                    Some(span) => format!("end={}", self.at(span)),
                    None => "unclosed".to_owned(),
                };
                let line = format!("raw {} engine={:?} {end}", self.at(raw.span), raw.engine);
                self.line(depth, &line);
                for text in &raw.lines {
                    let line = format!("| {text:?}");
                    self.line(depth + 1, &line);
                }
            }
        }
    }

    fn dialogue(&mut self, depth: usize, dialogue: &Dialogue) {
        let display = match &dialogue.display_name {
            Some(name) => format!("（{}）", name.value),
            None => String::new(),
        };
        let closed = if dialogue.closed { "" } else { " unclosed" };
        let line = format!(
            "dialogue {}{} {}{display} {}{}({}){closed}",
            self.at(dialogue.span),
            line_id(&dialogue.line_id),
            dialogue.speaker.value,
            dialogue.open,
            dialogue.close,
            dialogue.style
        );
        self.line(depth, &line);
        for body in &dialogue.lines {
            let line = format!("| {}", text(body));
            self.line(depth + 1, &line);
        }
        if let Some(trailing) = &dialogue.trailing {
            let line = format!("trailing {}", text(trailing));
            self.line(depth + 1, &line);
        }
    }

    fn choice(&mut self, depth: usize, choice: &Choice) {
        let prompt = match &choice.prompt {
            Some(prompt) => format!(" prompt={}", text(prompt)),
            None => String::new(),
        };
        let end = match choice.end {
            Some(span) => format!(" end={}", self.at(span)),
            None => " unclosed".to_owned(),
        };
        let line = format!(
            "choice {}{}{prompt}{end}",
            self.at(choice.span),
            line_id(&choice.line_id)
        );
        self.line(depth, &line);
        for option in &choice.options {
            let mut line = format!(
                "option {}{} {}",
                self.at(option.span),
                line_id(&option.line_id),
                text(&option.text)
            );
            if let Some(cond) = &option.cond {
                let _ = write!(line, " if {}", source_expr(cond));
            }
            if let Some(jump) = &option.jump {
                let _ = write!(line, " -> {:?}", jump.value);
            }
            self.line(depth + 1, &line);
            self.stmts(depth + 2, &option.body);
        }
    }

    fn if_block(&mut self, depth: usize, block: &IfBlock) {
        for (i, branch) in block.branches.iter().enumerate() {
            let keyword = if i == 0 { "if" } else { "elif" };
            let line = format!(
                "{keyword} {} {}",
                self.at(branch.span),
                source_expr(&branch.cond)
            );
            self.line(depth, &line);
            self.stmts(depth + 1, &branch.body);
        }
        if let Some(branch) = &block.else_branch {
            let line = format!("else {}", self.at(branch.span));
            self.line(depth, &line);
            self.stmts(depth + 1, &branch.body);
        }
        let line = match block.end {
            Some(span) => format!("end {}", self.at(span)),
            None => "end (unclosed)".to_owned(),
        };
        self.line(depth, &line);
    }
}

fn line_id(slot: &LineIdSlot) -> String {
    match slot {
        LineIdSlot::Absent => String::new(),
        LineIdSlot::Valid(id) => format!(" ^{}", id.value),
        LineIdSlot::Malformed => " ^(malformed)".to_owned(),
    }
}

fn arg(value: &Spanned<String>) -> String {
    format!("{:?}", value.value)
}

fn opt_arg(value: &Option<Spanned<String>>) -> String {
    match value {
        Some(value) => arg(value),
        None => "-".to_owned(),
    }
}

fn directive_kind(kind: &DirectiveKind) -> String {
    match kind {
        DirectiveKind::Bg { name, transition } => {
            format!("bg name={} transition={}", arg(name), opt_arg(transition))
        }
        DirectiveKind::Show {
            character,
            expression,
            position,
            transition,
        } => format!(
            "show character={} expression={} position={} transition={}",
            arg(character),
            arg(expression),
            position.map_or("-", |p| p.as_str()),
            opt_arg(transition)
        ),
        DirectiveKind::Hide {
            character,
            transition,
        } => format!(
            "hide character={} transition={}",
            arg(character),
            opt_arg(transition)
        ),
        DirectiveKind::Bgm { name } => format!("bgm name={}", arg(name)),
        DirectiveKind::BgmStop => "bgm_stop".to_owned(),
        DirectiveKind::Se { name } => format!("se name={}", arg(name)),
        DirectiveKind::Fx { name, seconds } => format!(
            "fx name={} seconds={}",
            arg(name),
            seconds.map_or("-".to_owned(), |s| s.to_string())
        ),
        DirectiveKind::Wait { seconds } => format!("wait seconds={seconds}"),
        DirectiveKind::Set { source, assignment } => match assignment {
            Some(assignment) => format!("set {}", self::assignment(assignment)),
            None => format!("set (error) {:?}", source.value),
        },
        DirectiveKind::Call { target } => format!("call {}", arg(target)),
        DirectiveKind::Return => "return".to_owned(),
        DirectiveKind::Ending { id } => format!("ending id={}", opt_arg(id)),
        DirectiveKind::Unknown { args } => {
            let args: Vec<_> = args.iter().map(arg).collect();
            format!("unknown args=[{}]", args.join(", "))
        }
    }
}

fn text(text: &Text) -> String {
    let parts: Vec<String> = text.parts.iter().map(inline).collect();
    format!("[{}]", parts.join(", "))
}

fn inline(part: &Inline) -> String {
    match part {
        Inline::Text { text, .. } => format!("{text:?}"),
        Inline::Ruby { base, reading, .. } => format!("ruby({base:?}, {reading:?})"),
        Inline::Emphasis { text, .. } => format!("emph({text:?})"),
        Inline::Wait { seconds, .. } => format!("wait({seconds})"),
        Inline::PageBreak { .. } => "page".to_owned(),
        Inline::Se { name, .. } => format!("se({name:?})"),
        Inline::Speed { speed, .. } => format!("speed({})", speed.as_str()),
        Inline::Interpolation { expr, .. } => format!("interp({})", source_expr(expr)),
    }
}

fn source_expr(source: &SourceExpr) -> String {
    match &source.expr {
        Some(expr) => self::expr(expr),
        None => format!("(error {:?})", source.text),
    }
}

fn var(var: &VarRef) -> String {
    match &var.namespace {
        Some(namespace) => format!("{namespace}.{}", var.name),
        None => var.name.clone(),
    }
}

fn expr(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Int(value) => value.to_string(),
        ExprKind::Str(value) => format!("{value:?}"),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Var(v) => var(v),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Neg => "neg",
                UnaryOp::Not => "not",
            };
            format!("({op} {})", self::expr(operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", op.as_str(), self::expr(lhs), self::expr(rhs))
        }
    }
}

fn assignment(assignment: &Assignment) -> String {
    format!(
        "({} {} {})",
        assignment.op.as_str(),
        var(&assignment.target),
        expr(&assignment.value)
    )
}

/// fixtureの先頭の `// expect: …` から、期待する診断コードを読む。
pub fn expected_codes(source: &str) -> Vec<String> {
    let first = source
        .trim_start_matches('\u{feff}')
        .lines()
        .next()
        .unwrap_or("");
    match first.strip_prefix("// expect:") {
        Some(codes) => codes.split_whitespace().map(str::to_owned).collect(),
        None => Vec::new(),
    }
}

/// スパンが範囲内にあり、文字の境界にあることを確かめる。
pub fn assert_span_ok(source: &str, span: Span, what: &str) {
    assert!(
        span.start <= span.end && span.end <= source.len(),
        "{what} のスパンが範囲外：{span:?}"
    );
    assert!(
        source.is_char_boundary(span.start) && source.is_char_boundary(span.end),
        "{what} のスパンが文字の境界に無い：{span:?}"
    );
}

/// ASTのすべてのスパンが、範囲内で文字の境界にあることを確かめる。
pub fn assert_ast_spans_ok(source: &str, chapter: &Chapter) {
    let ok = |span: Span, what: &str| assert_span_ok(source, span, what);
    if let Some(heading) = &chapter.heading {
        ok(heading.span, "章見出し");
    }
    for scene in &chapter.scenes {
        ok(scene.span, "シーン見出し");
        if let Some(id) = &scene.id {
            ok(id.span, "シーンID");
        }
        if let Some(meta) = &scene.meta {
            ok(meta.span, "メタ情報");
            for entry in &meta.entries {
                ok(entry.key.span, "メタ情報のキー");
                ok(entry.value.span, "メタ情報の値");
            }
        }
        stmts_spans(source, &scene.body);
    }
}

fn stmts_spans(source: &str, stmts: &[Stmt]) {
    let ok = |span: Span, what: &str| assert_span_ok(source, span, what);
    let slot = |line_id: &LineIdSlot| {
        if let LineIdSlot::Valid(id) = line_id {
            ok(id.span, "行ID");
        }
    };
    for stmt in stmts {
        match stmt {
            Stmt::Dialogue(dialogue) => {
                ok(dialogue.span, "台詞");
                ok(dialogue.speaker.span, "話者");
                if let Some(name) = &dialogue.display_name {
                    ok(name.span, "表示名");
                }
                for line in &dialogue.lines {
                    text_spans(source, line);
                }
                if let Some(trailing) = &dialogue.trailing {
                    text_spans(source, trailing);
                }
                slot(&dialogue.line_id);
            }
            Stmt::Narration(narration) => {
                ok(narration.span, "地の文");
                text_spans(source, &narration.text);
                slot(&narration.line_id);
            }
            Stmt::Break(span) => ok(*span, "段落の区切り"),
            Stmt::Jump(jump) => {
                ok(jump.span, "ジャンプ");
                ok(jump.target.span, "ジャンプ先");
            }
            Stmt::Directive(directive) => {
                ok(directive.span, "ディレクティブ");
                directive_spans(source, &directive.kind);
            }
            Stmt::Choice(choice) => {
                ok(choice.span, "@選択");
                if let Some(prompt) = &choice.prompt {
                    text_spans(source, prompt);
                }
                slot(&choice.line_id);
                for option in &choice.options {
                    ok(option.span, "選択肢");
                    text_spans(source, &option.text);
                    if let Some(cond) = &option.cond {
                        source_expr_spans(source, cond);
                    }
                    if let Some(jump) = &option.jump {
                        ok(jump.span, "選択肢のジャンプ先");
                    }
                    slot(&option.line_id);
                    stmts_spans(source, &option.body);
                }
                if let Some(end) = choice.end {
                    ok(end, "@end");
                }
            }
            Stmt::If(block) => {
                ok(block.span, "@if");
                for branch in &block.branches {
                    ok(branch.span, "@if・@elif");
                    source_expr_spans(source, &branch.cond);
                    stmts_spans(source, &branch.body);
                }
                if let Some(branch) = &block.else_branch {
                    ok(branch.span, "@else");
                    stmts_spans(source, &branch.body);
                }
                if let Some(end) = block.end {
                    ok(end, "@end");
                }
            }
            Stmt::Raw(raw) => {
                ok(raw.span, "@raw");
                if let Some(end) = raw.end {
                    ok(end, "@end");
                }
            }
        }
    }
}

fn directive_spans(source: &str, kind: &DirectiveKind) {
    let ok = |value: &Spanned<String>| assert_span_ok(source, value.span, "ディレクティブの引数");
    let opt = |value: &Option<Spanned<String>>| {
        if let Some(value) = value {
            ok(value);
        }
    };
    match kind {
        DirectiveKind::Bg { name, transition } => {
            ok(name);
            opt(transition);
        }
        DirectiveKind::Show {
            character,
            expression,
            transition,
            ..
        } => {
            ok(character);
            ok(expression);
            opt(transition);
        }
        DirectiveKind::Hide {
            character,
            transition,
        } => {
            ok(character);
            opt(transition);
        }
        DirectiveKind::Bgm { name }
        | DirectiveKind::Se { name }
        | DirectiveKind::Fx { name, .. } => ok(name),
        DirectiveKind::Set {
            source: text,
            assignment,
        } => {
            ok(text);
            if let Some(assignment) = assignment {
                assert_span_ok(source, assignment.target.span, "代入先");
                expr_spans(source, &assignment.value);
            }
        }
        DirectiveKind::Call { target } => ok(target),
        DirectiveKind::Ending { id } => opt(id),
        DirectiveKind::Unknown { args } => args.iter().for_each(ok),
        DirectiveKind::BgmStop | DirectiveKind::Wait { .. } | DirectiveKind::Return => {}
    }
}

fn text_spans(source: &str, text: &Text) {
    assert_span_ok(source, text.span, "文");
    for part in &text.parts {
        let span = match part {
            Inline::Text { span, .. }
            | Inline::Ruby { span, .. }
            | Inline::Emphasis { span, .. }
            | Inline::Wait { span, .. }
            | Inline::PageBreak { span }
            | Inline::Se { span, .. }
            | Inline::Speed { span, .. } => *span,
            Inline::Interpolation { expr, span } => {
                source_expr_spans(source, expr);
                *span
            }
        };
        assert_span_ok(source, span, "文中の記法");
    }
}

fn source_expr_spans(source: &str, expr: &SourceExpr) {
    assert_span_ok(source, expr.span, "式の文字列");
    if let Some(expr) = &expr.expr {
        expr_spans(source, expr);
    }
}

fn expr_spans(source: &str, expr: &Expr) {
    assert_span_ok(source, expr.span, "式");
    match &expr.kind {
        ExprKind::Var(var) => assert_span_ok(source, var.span, "変数名"),
        ExprKind::Unary { operand, .. } => expr_spans(source, operand),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_spans(source, lhs);
            expr_spans(source, rhs);
        }
        ExprKind::Int(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
    }
}
