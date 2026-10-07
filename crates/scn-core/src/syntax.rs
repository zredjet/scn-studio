//! 行の並びからASTを組み立てる。
//!
//! ブロックの入れ子は、再帰ではなく明示的なスタック（`Frame`）で扱う。対応が崩れても、
//! 診断を出して続ける（閉じていないブロックは次のシーン見出しかファイルの終わりで閉じ、
//! 閉じていない台詞は空行や命令の手前で終える）。

use crate::ast::{
    Chapter, ChapterHeading, Choice, ChoiceOption, CondBranch, Dialogue, Directive, DirectiveKind,
    ElseBranch, IfBlock, Jump, LineIdSlot, MetaEntry, MetaValue, Narration, Position, Raw, Scene,
    SceneMeta, Stmt,
};
use crate::diag::{Code, Diagnostic, Span, Spanned};
use crate::expr::{SourceExpr, parse_assignment, parse_condition};
use crate::inline::{Text, parse_seconds, parse_text};
use crate::line::{
    DialogueStart, HeadingLine, Line, LineKind, Piece, RawLineId, SourceLine, classify, find_close,
    is_indent, is_space, split_lines,
};

/// 台詞に使える括弧と、その表示スタイル（`project.toml` の `[dialogue.brackets]`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueBracket {
    pub open: char,
    pub close: char,
    pub style: String,
}

/// 構文解析の設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOptions {
    /// 台詞に使える括弧。開き括弧と閉じ括弧は互いに違う文字にする。
    pub brackets: Vec<DialogueBracket>,
}

impl Default for ParseOptions {
    /// プロジェクトの外で1ファイルだけを解析するときの設定。括弧は「」（`normal`）だけ。
    fn default() -> Self {
        Self {
            brackets: vec![DialogueBracket {
                open: '「',
                close: '」',
                style: "normal".to_owned(),
            }],
        }
    }
}

/// 構文解析の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub chapter: Chapter,
    /// 位置の順に並べてある。
    pub diagnostics: Vec<Diagnostic>,
}

/// 1つの `.scn` ファイルを構文解析する。設定DBは見ない。
pub fn parse(source: &str, options: &ParseOptions) -> Parsed {
    let mut parser = Parser {
        lines: split_lines(source),
        pos: 0,
        brackets: &options.brackets,
        diags: Vec::new(),
        heading: None,
        seen_top: false,
        scenes: Vec::new(),
        scene: None,
        stack: Vec::new(),
        dialogue: None,
    };
    parser.run();
    let mut diagnostics = parser.diags;
    diagnostics.sort_by_key(|d| d.span.start);
    Parsed {
        chapter: Chapter {
            heading: parser.heading,
            scenes: parser.scenes,
        },
        diagnostics,
    }
}

/// シーンIDとエンディングIDの形（英数字と `_` の1文字以上）。
fn is_valid_id(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn spanned(piece: Piece<'_>) -> Spanned<String> {
    Spanned::new(piece.text.to_owned(), piece.span())
}

fn slot(line_id: &RawLineId) -> LineIdSlot {
    match line_id {
        RawLineId::None => LineIdSlot::Absent,
        RawLineId::Valid(id) => LineIdSlot::Valid(id.clone()),
        RawLineId::Malformed(_) => LineIdSlot::Malformed,
    }
}

/// 1つの枝（シーンの本文、選択肢の中身、`@if` の各枝）の要素の並び。
#[derive(Default)]
struct StmtList {
    stmts: Vec<Stmt>,
    /// 本文の行の後ろに来た空行。次に本文の行が来たら `Break` にする。
    pending_break: Option<Span>,
    /// 空行とコメントを除いて、直前の行が本文の行（台詞か地の文）だったか。
    after_text: bool,
}

impl StmtList {
    fn push(&mut self, stmt: Stmt) {
        let is_text = matches!(stmt, Stmt::Dialogue(_) | Stmt::Narration(_));
        match self.pending_break.take() {
            Some(span) if is_text => self.stmts.push(Stmt::Break(span)),
            _ => {}
        }
        self.after_text = is_text;
        self.stmts.push(stmt);
    }

    /// 本文の行ではない行（誤りで捨てた行も含む）が来た。前後の空行は区切りにしない。
    fn interrupt(&mut self) {
        self.pending_break = None;
        self.after_text = false;
    }

    fn blank(&mut self, span: Span) {
        if self.after_text {
            self.pending_break = Some(match self.pending_break {
                Some(prev) => prev.cover(span),
                None => span,
            });
        }
    }
}

struct SceneBuilder {
    span: Span,
    title: String,
    id: Option<Spanned<String>>,
    meta: Option<SceneMeta>,
    body: StmtList,
}

enum Frame {
    Choice(ChoiceFrame),
    If(IfFrame),
}

struct ChoiceFrame {
    span: Span,
    prompt: Option<Text>,
    line_id: LineIdSlot,
    options: Vec<ChoiceOption>,
    /// 書いている途中の選択肢。`None` なら、まだ最初の選択肢の前。
    current: Option<OptionFrame>,
}

impl ChoiceFrame {
    fn finish_option(&mut self) {
        if let Some(option) = self.current.take() {
            self.options.push(ChoiceOption {
                span: option.span,
                text: option.text,
                cond: option.cond,
                jump: option.jump,
                line_id: option.line_id,
                body: option.body.stmts,
            });
        }
    }
}

struct OptionFrame {
    span: Span,
    text: Text,
    cond: Option<SourceExpr>,
    jump: Option<Spanned<String>>,
    line_id: LineIdSlot,
    body: StmtList,
}

enum BranchHead {
    Cond { span: Span, cond: SourceExpr },
    Else { span: Span },
}

struct IfFrame {
    if_span: Span,
    branches: Vec<CondBranch>,
    else_branch: Option<ElseBranch>,
    current: BranchHead,
    body: StmtList,
}

impl IfFrame {
    fn finish_branch(&mut self, next: Option<BranchHead>) {
        let body = std::mem::take(&mut self.body).stmts;
        let head = match next {
            Some(next) => std::mem::replace(&mut self.current, next),
            // 閉じるときは、ダミーの else を置いて取り出す（もう使わない）。
            None => std::mem::replace(&mut self.current, BranchHead::Else { span: self.if_span }),
        };
        match head {
            BranchHead::Cond { span, cond } => self.branches.push(CondBranch { span, cond, body }),
            BranchHead::Else { span } => self.else_branch = Some(ElseBranch { span, body }),
        }
    }
}

struct PendingDialogue {
    first_span: Span,
    span: Span,
    speaker: Spanned<String>,
    display_name: Option<Spanned<String>>,
    bracket: DialogueBracket,
    lines: Vec<Text>,
    depth: usize,
    /// 最後に読んだ行の行ID。
    line_id: RawLineId,
}

impl PendingDialogue {
    fn finish(self, closed: bool, trailing: Option<Text>) -> Dialogue {
        Dialogue {
            span: self.span,
            speaker: self.speaker,
            display_name: self.display_name,
            open: self.bracket.open,
            close: self.bracket.close,
            style: self.bracket.style,
            lines: self.lines,
            closed,
            trailing,
            line_id: slot(&self.line_id),
        }
    }
}

struct Parser<'a> {
    lines: Vec<SourceLine<'a>>,
    pos: usize,
    brackets: &'a [DialogueBracket],
    diags: Vec<Diagnostic>,
    heading: Option<ChapterHeading>,
    /// 空行とコメント以外の行を、すでに読んだか（章見出しの判定に使う）。
    seen_top: bool,
    scenes: Vec<Scene>,
    scene: Option<SceneBuilder>,
    stack: Vec<Frame>,
    /// 閉じ括弧を待っている台詞。
    dialogue: Option<PendingDialogue>,
}

impl<'a> Parser<'a> {
    fn run(&mut self) {
        while let Some(&source_line) = self.lines.get(self.pos) {
            self.pos += 1;
            let line = classify(source_line, self.brackets);
            self.handle(line);
        }
        if let Some(pending) = self.dialogue.take() {
            self.unclosed_dialogue(pending);
        }
        self.close_scene();
    }

    fn diag(&mut self, code: Code, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::new(code, span, message));
    }

    fn handle(&mut self, line: Line<'a>) {
        if let RawLineId::Malformed(id) = &line.line_id {
            let message = format!(
                "行IDの形が不正です：^{}（[0-9a-z] の4〜12文字で書きます）",
                id.value
            );
            self.diag(Code::E304, id.span, message);
        }

        if let Some(pending) = self.dialogue.take() {
            match line.kind {
                LineKind::Comment => {
                    self.dialogue = Some(pending);
                    return;
                }
                LineKind::Blank
                | LineKind::SceneHeading(_)
                | LineKind::Directive { .. }
                | LineKind::Option { .. }
                | LineKind::Jump { .. } => self.unclosed_dialogue(pending),
                _ => {
                    self.continue_dialogue(pending, &line);
                    return;
                }
            }
        }

        if self.scene.is_none() {
            self.handle_before_scene(line);
            return;
        }

        if self.in_choice_preamble() && !allowed_in_preamble(&line.kind) {
            self.diag(
                Code::E402,
                line.span,
                "@選択 と最初の選択肢の間に置けるのは、空行とコメントだけです",
            );
            return;
        }

        match line.kind {
            LineKind::Blank => {
                // 行IDだけが残った行（` ^abcd`）も空行になる。行IDは持てない。
                self.reject_line_id(&line);
                if let Some(list) = self.current_list() {
                    list.blank(line.span);
                }
            }
            LineKind::Comment => {}
            LineKind::SceneHeading(heading) => self.start_scene(&line, heading),
            LineKind::Hash { .. } => {
                self.diag(
                    Code::W112,
                    line.span,
                    "章見出し（# ）はファイルの先頭にだけ置けます。地の文として扱います",
                );
                self.narration(&line);
            }
            LineKind::TripleDash | LineKind::Narration => self.narration(&line),
            LineKind::Option { rest } => {
                if matches!(self.stack.last(), Some(Frame::Choice(_))) {
                    self.option(&line, rest);
                } else {
                    self.diag(
                        Code::E105,
                        line.span,
                        "選択ブロックの外にある選択肢の行です。地の文として扱います",
                    );
                    self.narration(&line);
                }
            }
            LineKind::Jump { target } => {
                self.reject_line_id(&line);
                self.push_stmt(Stmt::Jump(Jump {
                    span: line.span,
                    target: spanned(target),
                }));
            }
            LineKind::Dialogue(start) => self.start_dialogue(&line, start),
            LineKind::Directive { name, rest } => {
                // ディレクティブの前の空行は、誤りで捨てる行（E402・E104）の前でも区切りにしない。
                if let Some(list) = self.current_list() {
                    list.interrupt();
                }
                self.directive(&line, name, rest);
            }
        }
    }

    fn handle_before_scene(&mut self, line: Line<'a>) {
        match line.kind {
            LineKind::Blank => {
                self.reject_line_id(&line);
                return;
            }
            LineKind::Comment => return,
            LineKind::SceneHeading(heading) => self.start_scene(&line, heading),
            LineKind::Hash { title } if !self.seen_top => {
                self.reject_line_id(&line);
                self.heading = Some(ChapterHeading {
                    title: title.trim_by(is_indent).text.to_owned(),
                    span: line.span,
                });
            }
            LineKind::Hash { .. } => {
                self.diag(
                    Code::W112,
                    line.span,
                    "章見出し（# ）はファイルの先頭に1つだけ置けます",
                );
                self.before_first_scene(line.span);
            }
            _ => self.before_first_scene(line.span),
        }
        self.seen_top = true;
    }

    fn before_first_scene(&mut self, span: Span) {
        self.diag(
            Code::W101,
            span,
            "最初のシーン見出しより前の行です。この行は無視します",
        );
    }

    /// 行IDを持てない行に行IDがあれば、W303 を出す。
    fn reject_line_id(&mut self, line: &Line<'_>) {
        if let RawLineId::Valid(id) = &line.line_id {
            self.diag(Code::W303, id.span, "この行には行IDを付けられません");
        }
    }

    fn in_choice_preamble(&self) -> bool {
        matches!(self.stack.last(), Some(Frame::Choice(choice)) if choice.current.is_none())
    }

    /// 今の要素を足す先。最初の選択肢より前なら `None`。
    fn current_list(&mut self) -> Option<&mut StmtList> {
        match self.stack.last_mut() {
            Some(Frame::Choice(choice)) => choice.current.as_mut().map(|option| &mut option.body),
            Some(Frame::If(block)) => Some(&mut block.body),
            None => self.scene.as_mut().map(|scene| &mut scene.body),
        }
    }

    fn push_stmt(&mut self, stmt: Stmt) {
        if let Some(list) = self.current_list() {
            list.push(stmt);
        }
    }

    // ---- シーン ----

    fn start_scene(&mut self, line: &Line<'_>, heading: HeadingLine<'_>) {
        self.close_scene();
        self.reject_line_id(line);
        let id = match heading.id {
            None => {
                self.diag(
                    Code::W107,
                    line.span,
                    "シーン見出しにIDがありません（行末に ` #シーンID` を書きます）",
                );
                None
            }
            Some(id) if is_valid_id(id.text) => Some(spanned(id)),
            Some(id) => {
                let span = Span::new(id.start.saturating_sub(1), id.end());
                let message = format!("シーンIDは英数字と _ で書きます：#{}", id.text);
                self.diag(Code::E111, span, message);
                None
            }
        };
        let meta = self.meta();
        self.scene = Some(SceneBuilder {
            span: line.span,
            title: heading.title.text.to_owned(),
            id,
            meta,
            body: StmtList::default(),
        });
    }

    /// 開いているブロックを閉じ（E103）、今のシーンを確定する。
    fn close_scene(&mut self) {
        while let Some(frame) = self.stack.pop() {
            self.close_frame(frame, None);
        }
        if let Some(scene) = self.scene.take() {
            self.scenes.push(Scene {
                span: scene.span,
                title: scene.title,
                id: scene.id,
                meta: scene.meta,
                body: scene.body.stmts,
            });
        }
    }

    // ---- メタ情報 ----

    fn meta_line_ids(&mut self, line: &Line<'_>) {
        match &line.line_id {
            RawLineId::Malformed(id) => {
                let message = format!(
                    "行IDの形が不正です：^{}（[0-9a-z] の4〜12文字で書きます）",
                    id.value
                );
                self.diag(Code::E304, id.span, message);
            }
            RawLineId::Valid(_) => self.reject_line_id(line),
            RawLineId::None => {}
        }
    }

    fn classify_at(&self, index: usize) -> Option<Line<'a>> {
        self.lines
            .get(index)
            .map(|&source_line| classify(source_line, self.brackets))
    }

    /// シーン見出しの直後のメタ情報を読む（仕様書5節）。
    fn meta(&mut self) -> Option<SceneMeta> {
        let open = self.classify_at(self.pos)?;
        if !matches!(open.kind, LineKind::TripleDash) {
            return None;
        }
        self.pos += 1;
        self.meta_line_ids(&open);

        // 閉じの `---` は、次のシーン見出しより前でだけ探す。
        let mut close_at = None;
        let mut index = self.pos;
        while let Some(line) = self.classify_at(index) {
            match line.kind {
                LineKind::SceneHeading(_) => break,
                LineKind::TripleDash => {
                    close_at = Some(index);
                    break;
                }
                _ => index += 1,
            }
        }

        let mut entries = Vec::new();
        if let Some(close_at) = close_at {
            while self.pos < close_at {
                let Some(line) = self.classify_at(self.pos) else {
                    break;
                };
                self.pos += 1;
                self.meta_line_ids(&line);
                if matches!(line.kind, LineKind::Blank | LineKind::Comment) {
                    continue;
                }
                match split_meta_line(line.body) {
                    Some((key, value)) => self.meta_entry(&mut entries, key, value),
                    None => self.diag(
                        Code::E114,
                        line.span,
                        "メタ情報の行は `キー: 値` の形で書きます",
                    ),
                }
            }
            if let Some(close) = self.classify_at(close_at) {
                self.meta_line_ids(&close);
            }
            self.pos = close_at + 1;
        } else {
            self.diag(Code::E102, open.span, "メタ情報の --- が閉じていません");
            // `キー: 値` の形の行が続く間だけをメタ情報とみなす。
            while let Some(line) = self.classify_at(self.pos) {
                if matches!(line.kind, LineKind::Comment) {
                    self.pos += 1;
                    continue;
                }
                if matches!(line.kind, LineKind::SceneHeading(_)) {
                    break;
                }
                let Some((key, value)) = split_meta_line(line.body) else {
                    break;
                };
                self.pos += 1;
                self.meta_line_ids(&line);
                self.meta_entry(&mut entries, key, value);
            }
        }
        Some(SceneMeta {
            span: open.span,
            entries,
            closed: close_at.is_some(),
        })
    }

    fn meta_entry(&mut self, entries: &mut Vec<MetaEntry>, key: Piece<'_>, value: Piece<'_>) {
        let parsed = parse_meta_value(value.text);
        match key.text {
            "summary" | "route" | "tags" => {}
            "status" => {
                let valid = matches!(
                    &parsed,
                    MetaValue::Text(text) if matches!(text.as_str(), "draft" | "writing" | "done" | "ai-draft")
                );
                if !valid {
                    let message = format!(
                        "status の値が不正です：{}（draft・writing・done・ai-draft のどれかを書きます）",
                        value.text
                    );
                    self.diag(Code::E110, value.span(), message);
                }
            }
            other => {
                let message = format!("知らないメタ情報のキーです：{other}");
                self.diag(Code::I109, key.span(), message);
            }
        }
        let value = Spanned::new(parsed, value.span());
        if let Some(existing) = entries.iter_mut().find(|e| e.key.value == key.text) {
            let message = format!(
                "メタ情報のキー {} が重複しています。後の値を使います",
                key.text
            );
            self.diag(Code::W113, key.span(), message);
            existing.value = value;
        } else {
            entries.push(MetaEntry {
                key: spanned(key),
                value,
            });
        }
    }

    // ---- 本文の行 ----

    fn narration(&mut self, line: &Line<'_>) {
        let text = parse_text(line.content, &mut self.diags);
        self.push_stmt(Stmt::Narration(Narration {
            span: line.span,
            text,
            line_id: slot(&line.line_id),
        }));
    }

    fn start_dialogue(&mut self, line: &Line<'_>, start: DialogueStart<'_>) {
        let Some(bracket) = self.brackets.get(start.bracket).cloned() else {
            self.narration(line);
            return;
        };
        let pending = PendingDialogue {
            first_span: line.span,
            span: line.span,
            speaker: spanned(start.speaker),
            display_name: start.display_name.map(spanned),
            bracket,
            lines: Vec::new(),
            depth: 1,
            line_id: line.line_id.clone(),
        };
        self.feed_dialogue(pending, start.after_open);
    }

    fn continue_dialogue(&mut self, mut pending: PendingDialogue, line: &Line<'_>) {
        if let RawLineId::Valid(id) = &pending.line_id {
            self.diag(
                Code::W303,
                id.span,
                "複数行の台詞では、行IDは最後の行に付けます",
            );
        }
        pending.line_id = line.line_id.clone();
        pending.span = pending.span.cover(line.span);
        self.feed_dialogue(pending, line.content);
    }

    /// 台詞の本文の1行分を読む。閉じ括弧があれば台詞を確定し、無ければ続きを待つ。
    fn feed_dialogue(&mut self, mut pending: PendingDialogue, text: Piece<'_>) {
        match find_close(text.text, &pending.bracket, &mut pending.depth) {
            Some(at) => {
                let (body, rest) = text.split_at(at);
                let trailing = rest.skip(pending.bracket.close.len_utf8());
                pending.lines.push(parse_text(body, &mut self.diags));
                let trailing = if trailing.text.chars().all(is_space) {
                    None
                } else {
                    Some(parse_text(trailing, &mut self.diags))
                };
                let dialogue = pending.finish(true, trailing);
                self.push_stmt(Stmt::Dialogue(dialogue));
            }
            None => {
                pending.lines.push(parse_text(text, &mut self.diags));
                self.dialogue = Some(pending);
            }
        }
    }

    fn unclosed_dialogue(&mut self, pending: PendingDialogue) {
        let message = format!("台詞の閉じ括弧 {} がありません", pending.bracket.close);
        self.diag(Code::E201, pending.first_span, message);
        let dialogue = pending.finish(false, None);
        self.push_stmt(Stmt::Dialogue(dialogue));
    }

    /// 選択肢の行（仕様書11節）。右から行ID（切り出し済み）、最後の ` -> `、最後の ` if ` を切り出す。
    fn option(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        let mut text_part = rest;
        let mut jump = None;
        if let Some(at) = text_part.text.rfind(" -> ") {
            jump = Some(spanned(text_part.skip(at + 4).trim_by(is_space)));
            text_part = text_part.split_at(at).0;
        }
        let mut cond = None;
        if let Some(at) = text_part.text.rfind(" if ") {
            let expr = text_part.skip(at + 4).trim_by(is_space);
            cond = Some(parse_condition(expr, &mut self.diags));
            text_part = text_part.split_at(at).0;
        }
        let text_piece = text_part.trim_by(is_indent);
        if text_piece.is_empty() {
            self.diag(Code::E402, line.span, "選択肢の表示文がありません");
        }
        let text = parse_text(text_piece, &mut self.diags);
        if let Some(Frame::Choice(choice)) = self.stack.last_mut() {
            choice.finish_option();
            choice.current = Some(OptionFrame {
                span: line.span,
                text,
                cond,
                jump,
                line_id: slot(&line.line_id),
                body: StmtList::default(),
            });
        }
    }

    // ---- ディレクティブ ----

    fn directive(&mut self, line: &Line<'_>, name: Piece<'_>, rest: Piece<'_>) {
        match name.text {
            "選択" | "choice" => self.open_choice(line, rest),
            "if" => self.open_if(line, rest),
            "elif" => self.elif(line, rest),
            "else" => self.else_(line, rest),
            "end" => self.end(line, rest),
            "raw" => self.raw(line, rest),
            _ => {
                self.reject_line_id(line);
                if let Some(kind) = self.simple_directive(line, name.text, rest) {
                    self.push_stmt(Stmt::Directive(Directive {
                        span: line.span,
                        name: name.text.to_owned(),
                        kind,
                    }));
                }
            }
        }
    }

    /// ブロックではないディレクティブ。引数が合わなければ E402 を出して `None`。
    fn simple_directive(
        &mut self,
        line: &Line<'_>,
        name: &str,
        rest: Piece<'_>,
    ) -> Option<DirectiveKind> {
        let Some(canonical) = canonical_name(name) else {
            let message = format!("知らないディレクティブです：@{name}");
            self.diag(Code::W401, line.span, message);
            let (args, _) = split_args(rest);
            return Some(DirectiveKind::Unknown {
                args: args.into_iter().map(spanned).collect(),
            });
        };

        if canonical == "set" {
            if rest.is_empty() {
                self.diag(
                    Code::E402,
                    line.span,
                    format!("@{name} に代入の式がありません"),
                );
                return None;
            }
            let assignment = parse_assignment(rest, &mut self.diags);
            return Some(DirectiveKind::Set {
                source: spanned(rest),
                assignment,
            });
        }

        let (args, unclosed) = split_args(rest);
        if unclosed {
            self.diag(Code::E402, line.span, "引数の \" が閉じていません");
            return None;
        }
        let kind = match (canonical, args.as_slice()) {
            ("bg", [bg]) => DirectiveKind::Bg {
                name: spanned(*bg),
                transition: None,
            },
            ("bg", [bg, transition]) => DirectiveKind::Bg {
                name: spanned(*bg),
                transition: Some(spanned(*transition)),
            },
            ("show", [character, expression]) => DirectiveKind::Show {
                character: spanned(*character),
                expression: spanned(*expression),
                position: None,
                transition: None,
            },
            ("show", [character, expression, third]) => {
                let position = parse_position(third.text);
                DirectiveKind::Show {
                    character: spanned(*character),
                    expression: spanned(*expression),
                    position,
                    transition: position.is_none().then(|| spanned(*third)),
                }
            }
            ("show", [character, expression, position, transition]) => {
                let Some(position) = parse_position(position.text) else {
                    self.diag(
                        Code::E402,
                        line.span,
                        format!(
                            "@{name} の3つ目の引数は位置（左・中・右）です：{}",
                            position.text
                        ),
                    );
                    return None;
                };
                DirectiveKind::Show {
                    character: spanned(*character),
                    expression: spanned(*expression),
                    position: Some(position),
                    transition: Some(spanned(*transition)),
                }
            }
            ("hide", [character]) => DirectiveKind::Hide {
                character: spanned(*character),
                transition: None,
            },
            ("hide", [character, transition]) => DirectiveKind::Hide {
                character: spanned(*character),
                transition: Some(spanned(*transition)),
            },
            ("bgm", [bgm]) if matches!(bgm.text, "停止" | "stop") => DirectiveKind::BgmStop,
            ("bgm", [bgm]) => DirectiveKind::Bgm {
                name: spanned(*bgm),
            },
            ("se", [se]) => DirectiveKind::Se { name: spanned(*se) },
            ("fx", [fx]) => DirectiveKind::Fx {
                name: spanned(*fx),
                seconds: None,
            },
            ("fx", [fx, seconds]) => {
                let seconds = self.seconds(line, name, *seconds)?;
                DirectiveKind::Fx {
                    name: spanned(*fx),
                    seconds: Some(seconds),
                }
            }
            ("wait", [seconds]) => DirectiveKind::Wait {
                seconds: self.seconds(line, name, *seconds)?,
            },
            ("call", [target]) => DirectiveKind::Call {
                target: spanned(*target),
            },
            ("return", args) => {
                if !args.is_empty() {
                    self.diag(Code::E402, line.span, format!("@{name} に引数は書けません"));
                }
                DirectiveKind::Return
            }
            ("ending", [id]) if is_valid_id(id.text) => DirectiveKind::Ending {
                id: Some(spanned(*id)),
            },
            ("ending", [id]) => {
                let message = format!("エンディングIDは英数字と _ で書きます：{}", id.text);
                self.diag(Code::E111, id.span(), message);
                DirectiveKind::Ending { id: None }
            }
            _ => {
                let expected = expected_args(canonical);
                self.diag(
                    Code::E402,
                    line.span,
                    format!("@{name} の引数の数が合いません（{expected}）"),
                );
                return None;
            }
        };
        Some(kind)
    }

    fn seconds(&mut self, line: &Line<'_>, name: &str, arg: Piece<'_>) -> Option<f64> {
        let seconds = parse_seconds(arg.text);
        if seconds.is_none() {
            self.diag(
                Code::E402,
                line.span,
                format!("@{name} の秒は半角の数で書きます：{}", arg.text),
            );
        }
        seconds
    }

    fn open_choice(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        let prompt = (!rest.is_empty()).then(|| parse_text(rest, &mut self.diags));
        self.stack.push(Frame::Choice(ChoiceFrame {
            span: line.span,
            prompt,
            line_id: slot(&line.line_id),
            options: Vec::new(),
            current: None,
        }));
    }

    fn condition(&mut self, line: &Line<'_>, name: &str, rest: Piece<'_>) -> SourceExpr {
        if rest.is_empty() {
            self.diag(Code::E402, line.span, format!("@{name} に式がありません"));
            return SourceExpr {
                text: String::new(),
                span: rest.span(),
                expr: None,
            };
        }
        parse_condition(rest, &mut self.diags)
    }

    fn open_if(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        self.reject_line_id(line);
        let cond = self.condition(line, "if", rest);
        self.stack.push(Frame::If(IfFrame {
            if_span: line.span,
            branches: Vec::new(),
            else_branch: None,
            current: BranchHead::Cond {
                span: line.span,
                cond,
            },
            body: StmtList::default(),
        }));
    }

    fn elif(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        self.reject_line_id(line);
        match self.stack.last() {
            Some(Frame::If(block)) if matches!(block.current, BranchHead::Cond { .. }) => {}
            Some(Frame::If(_)) => {
                self.diag(Code::E104, line.span, "@else の後に @elif は置けません");
                return;
            }
            _ => {
                self.diag(Code::E104, line.span, "@elif に対応する @if がありません");
                return;
            }
        }
        let cond = self.condition(line, "elif", rest);
        if let Some(Frame::If(block)) = self.stack.last_mut() {
            block.finish_branch(Some(BranchHead::Cond {
                span: line.span,
                cond,
            }));
        }
    }

    fn else_(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        self.reject_line_id(line);
        match self.stack.last() {
            Some(Frame::If(block)) if matches!(block.current, BranchHead::Cond { .. }) => {}
            Some(Frame::If(_)) => {
                self.diag(Code::E104, line.span, "@else が2つあります");
                return;
            }
            _ => {
                self.diag(Code::E104, line.span, "@else に対応する @if がありません");
                return;
            }
        }
        if !rest.is_empty() {
            self.diag(Code::E402, line.span, "@else に引数は書けません");
        }
        if let Some(Frame::If(block)) = self.stack.last_mut() {
            block.finish_branch(Some(BranchHead::Else { span: line.span }));
        }
    }

    fn end(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        self.reject_line_id(line);
        let Some(frame) = self.stack.pop() else {
            self.diag(Code::E104, line.span, "@end に対応するブロックがありません");
            return;
        };
        if !rest.is_empty() {
            self.diag(Code::E402, line.span, "@end に引数は書けません");
        }
        self.close_frame(frame, Some(line.span));
    }

    /// ブロックを閉じて、外側の枝に足す。`end` が `None` なら閉じ忘れ（E103）。
    fn close_frame(&mut self, frame: Frame, end: Option<Span>) {
        match frame {
            Frame::Choice(mut choice) => {
                choice.finish_option();
                if end.is_none() {
                    self.diag(Code::E103, choice.span, "@選択 が @end で閉じていません");
                }
                if choice.options.is_empty() {
                    self.diag(Code::E106, choice.span, "@選択 に選択肢がありません");
                }
                self.push_stmt(Stmt::Choice(Choice {
                    span: choice.span,
                    prompt: choice.prompt,
                    line_id: choice.line_id,
                    options: choice.options,
                    end,
                }));
            }
            Frame::If(mut block) => {
                block.finish_branch(None);
                if end.is_none() {
                    self.diag(Code::E103, block.if_span, "@if が @end で閉じていません");
                }
                self.push_stmt(Stmt::If(IfBlock {
                    span: block.if_span,
                    branches: block.branches,
                    else_branch: block.else_branch,
                    end,
                }));
            }
        }
    }

    /// 生コード（仕様書15節）。中の行は解析せず、`@end` の行か、行頭に空白の無い `## ` の行まで読む。
    fn raw(&mut self, line: &Line<'_>, rest: Piece<'_>) {
        self.reject_line_id(line);
        let (args, unclosed) = split_args(rest);
        let engine = match args.as_slice() {
            [engine] if !unclosed => engine.text.to_owned(),
            _ => {
                self.diag(Code::E402, line.span, "@raw にはエンジン名を1つ書きます");
                args.first().map(|a| a.text.to_owned()).unwrap_or_default()
            }
        };
        let mut texts = Vec::new();
        let mut end = None;
        while let Some(&source_line) = self.lines.get(self.pos) {
            let text = source_line.piece.text;
            if text.trim_matches(is_space) == "@end" {
                end = Some(source_line.span());
                self.pos += 1;
                break;
            }
            if text.starts_with("## ") {
                break;
            }
            texts.push(text);
            self.pos += 1;
        }
        if end.is_none() {
            self.diag(Code::E103, line.span, "@raw が @end で閉じていません");
        }
        self.push_stmt(Stmt::Raw(Raw {
            span: line.span,
            engine,
            lines: dedent(&texts),
            end,
        }));
    }
}

fn allowed_in_preamble(kind: &LineKind<'_>) -> bool {
    match kind {
        LineKind::Blank
        | LineKind::Comment
        | LineKind::Option { .. }
        | LineKind::SceneHeading(_) => true,
        LineKind::Directive { name, .. } => name.text == "end",
        _ => false,
    }
}

/// 命令名を英語名にそろえる。ブロックの命令（`@選択`、`@if` など）は含めない。
fn canonical_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "背景" | "bg" => "bg",
        "立ち絵" | "show" => "show",
        "退場" | "hide" => "hide",
        "BGM" | "bgm" => "bgm",
        "SE" | "se" => "se",
        "効果" | "fx" => "fx",
        "待機" | "wait" => "wait",
        "設定" | "set" => "set",
        "呼出" | "call" => "call",
        "戻る" | "return" => "return",
        "エンディング" | "ending" => "ending",
        _ => return None,
    })
}

fn expected_args(canonical: &str) -> &'static str {
    match canonical {
        "bg" => "背景名 [トランジション]",
        "show" => "キャラ 差分 [位置] [トランジション]",
        "hide" => "キャラ [トランジション]",
        "bgm" => "曲名、または 停止",
        "se" => "効果音名",
        "fx" => "効果名 [秒]",
        "wait" => "秒",
        "call" => "シーンID",
        "ending" => "エンディングID",
        _ => "",
    }
}

fn parse_position(text: &str) -> Option<Position> {
    match text {
        "左" | "left" => Some(Position::Left),
        "中" | "center" => Some(Position::Center),
        "右" | "right" => Some(Position::Right),
        _ => None,
    }
}

/// 引数を空白（半角・全角・タブ）で分ける。`"…"` で囲んだ引数は空白を含められる。
/// 2つ目の値は、`"` が閉じていなかったか。
fn split_args(rest: Piece<'_>) -> (Vec<Piece<'_>>, bool) {
    let mut args = Vec::new();
    let mut unclosed = false;
    let mut cur = rest.trim_start_by(is_space);
    while !cur.is_empty() {
        if let Some(quoted) = cur.strip_prefix("\"") {
            match quoted.text.find('"') {
                Some(close) => {
                    args.push(quoted.split_at(close).0);
                    cur = quoted.skip(close + 1);
                }
                None => {
                    unclosed = true;
                    args.push(quoted);
                    cur = Piece::new("", quoted.end());
                }
            }
        } else {
            let len = cur.text.find(is_space).unwrap_or(cur.text.len());
            let (arg, after) = cur.split_at(len);
            args.push(arg);
            cur = after;
        }
        cur = cur.trim_start_by(is_space);
    }
    (args, unclosed)
}

/// メタ情報の行を `キー: 値` に分ける。キーは空白と `:` を含まない1文字以上で、
/// `:` の後は半角空白か行末。
fn split_meta_line(body: Piece<'_>) -> Option<(Piece<'_>, Piece<'_>)> {
    let colon = body.text.find(':')?;
    let (key, after) = body.split_at(colon);
    if key.is_empty() || key.text.contains(is_space) {
        return None;
    }
    let after = after.skip(1);
    if !(after.is_empty() || after.text.starts_with(' ')) {
        return None;
    }
    Some((key, after.trim_by(is_indent)))
}

fn parse_meta_value(text: &str) -> MetaValue {
    match text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        Some(inner) if inner.trim_matches(is_space).is_empty() => MetaValue::List(Vec::new()),
        Some(inner) => MetaValue::List(
            inner
                .split(',')
                .map(|item| item.trim_matches(is_space).to_owned())
                .collect(),
        ),
        None => MetaValue::Text(text.to_owned()),
    }
}

/// `@raw` の中の行から、共通の字下げ（半角空白とタブの並びとして文字どおり共通な部分）を取り除く。
/// 半角空白とタブだけの行は空行とみなし、計算から外して空文字列にする。
fn dedent(lines: &[&str]) -> Vec<String> {
    let is_blank = |line: &str| line.chars().all(is_indent);
    let mut common: Option<&str> = None;
    for line in lines.iter().filter(|line| !is_blank(line)) {
        let indent_len = line.len() - line.trim_start_matches(is_indent).len();
        let indent = line.get(..indent_len).unwrap_or_default();
        common = Some(match common {
            None => indent,
            Some(prev) => {
                let len = prev
                    .bytes()
                    .zip(indent.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                prev.get(..len).unwrap_or_default()
            }
        });
    }
    let common_len = common.map_or(0, str::len);
    lines
        .iter()
        .map(|line| {
            if is_blank(line) {
                String::new()
            } else {
                line.get(common_len..).unwrap_or(line).to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_keeps_relative_indent() {
        let lines = ["    a", "      b", "  ", "", "\tc"];
        assert_eq!(dedent(&lines[..2]), ["a", "  b"]);
        assert_eq!(dedent(&lines[..4]), ["a", "  b", "", ""]);
        // 字下げに共通する部分が無ければ、何も取り除かない。
        assert_eq!(dedent(&lines), ["    a", "      b", "", "", "\tc"]);
    }

    #[test]
    fn split_args_with_quotes() {
        let (args, unclosed) = split_args(Piece::new("\"屋上 夕方\"　fade", 0));
        let texts: Vec<_> = args.iter().map(|a| a.text).collect();
        assert_eq!(texts, ["屋上 夕方", "fade"]);
        assert!(!unclosed);
        let (_, unclosed) = split_args(Piece::new("\"屋上", 0));
        assert!(unclosed);
    }

    #[test]
    fn meta_line_shape() {
        fn ok(s: &str) -> Option<(&str, &str)> {
            split_meta_line(Piece::new(s, 0)).map(|(k, v)| (k.text, v.text))
        }
        assert_eq!(ok("summary: 要約"), Some(("summary", "要約")));
        assert_eq!(ok("summary:"), Some(("summary", "")));
        assert_eq!(ok("link: http://x"), Some(("link", "http://x")));
        assert_eq!(ok("summary 要約"), None);
        assert_eq!(ok("summary:要約"), None);
        assert_eq!(ok(": 値"), None);
    }
}
