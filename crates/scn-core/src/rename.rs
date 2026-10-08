//! 名前の変更（F2、T2-8、D059）：シーンID、キャラクターの名前、変数、設定DBのエントリのID を、作品の全体で
//! 置き換える。
//!
//! - 本文は、章ごとに構文解析した AST の位置から置き換える（見出しのID、ジャンプ・選択肢の飛び先・`@呼出`、
//!   話者と `@立ち絵`・`@退場` のキャラクター、式と `{=…}` の変数）。文字列を探して置き換えないので、台詞の
//!   本文や地の文の中の同じ文字は変えない。行ID・コメント・`@raw` の中も変えない（設計の原則5）。
//! - 設定ファイルは、変わる値の行だけを差し替える（`bible::set_field` と、キーの行の差し替え）。
//!   - シーンID：`project.toml` の `start_scene`、`layout.json` のシーンの位置、設定DBの `ref<scene>` の項目、
//!     アイデアメモの `promoted_to`。
//!   - キャラクターの名前：エントリの `name`。本文は、名前で書いた話者・立ち絵・変数の名前空間だけ（IDや呼び名で
//!     書いたところは、そのまま）。
//!   - 変数：`variables.yaml` のキー。本文は、そろえると同じ変数になる書き方のすべて（名前空間の書き方は残す）。
//!   - エントリのID：ファイル名、`id`、ほかのエントリの `ref<型>` の項目、アイデアメモの `promoted_to`。キャラク
//!     ターなら、IDで書いた話者・立ち絵・変数の名前空間、`variables.yaml` の名前空間、素材カタログと Ren'Py の
//!     出力プロファイルの `characters` のキー、企画書の `main_characters` も。
//! - ここでは置き換えの中身を作るだけ。書くのは `workspace::Workspace::rename`。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ast::{DirectiveKind, Scene, Stmt};
use crate::bible::{front_fields, set_field, yaml_scalar};
use crate::concept::CONCEPT_FILE;
use crate::diag::Span;
use crate::expr::{Expr, ExprKind, SourceExpr, VarRef, parse_condition};
use crate::ideas::idea_files;
use crate::inline::{Inline, Text};
use crate::line::Piece;
use crate::project::{Character, Project};
use crate::renpy::{RENPY_PROFILE, read_profile};
use crate::sema::{Resolution, resolve_character_in};
use crate::syntax::parse;

/// 名前を変えるもの。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenameTarget {
    /// シーンID。
    Scene { id: String },
    /// キャラクターの名前（設定DBの `name`）。
    CharacterName { id: String },
    /// 変数（`名前空間.名前`）。
    Variable { key: String },
    /// 設定DBのエントリのID。
    EntryId { type_id: String, id: String },
}

/// カーソルのところの、名前を変えられるもの（F2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub target: RenameTarget,
    /// 今の名前（シーンID、キャラクターの名前、変数のキー、エントリのID）。
    pub current: String,
    /// 見せ方（`シーンID`、`キャラクターの名前（akari）` など）。
    pub label: String,
}

/// 本文の1か所の置き換え。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextEdit {
    pub span: Span,
    pub text: String,
}

/// 本文のファイルの置き換え。`text` は置き換えた後の中身。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoryChange {
    pub file: String,
    pub edits: Vec<TextEdit>,
    pub text: String,
}

/// 設定ファイルの書き換え。`text` は書き換えた後の中身。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    pub file: String,
    pub text: String,
}

/// ファイル名の変更（エントリのID）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Move {
    pub from: String,
    pub to: String,
}

/// `layout.json` の位置のキーの変更。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayoutRename {
    pub level: String,
    pub from: String,
    pub to: String,
}

/// 名前の変更の中身。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenamePlan {
    pub story: Vec<StoryChange>,
    /// 設定ファイル（エントリのファイル名を変えるときは、変えた後のパス）。
    pub settings: Vec<FileChange>,
    pub moves: Vec<Move>,
    pub layout: Option<LayoutRename>,
    /// 本文の置き換えの数。
    pub story_edits: usize,
    /// 知らせ（変えないもの、変わってしまうもの）。
    pub notes: Vec<String>,
}

/// 本文の名前の現れから、置き換えを作る。
type EditOf = Box<dyn Fn(&Site) -> Option<TextEdit>>;

/// 本文の中の名前の現れ。
#[derive(Debug, Clone)]
enum Site {
    /// シーン見出しのID。
    SceneId(String, Span),
    /// ジャンプ・選択肢の飛び先・`@呼出` のシーンID。
    SceneRef(String, Span),
    /// 話者・`@立ち絵`・`@退場` のキャラクター（書かれたとおり）。
    Character(String, Span),
    /// 変数。名前空間の位置（書かれていれば）と、名前の位置。
    Var {
        var: VarRef,
        namespace: Option<Span>,
        name: Span,
    },
    /// 生コード（`@raw`）の中身（変えない。知らせに使う）。
    Raw(String, Span),
}

impl Site {
    fn span(&self) -> Span {
        match self {
            Site::SceneId(_, span)
            | Site::SceneRef(_, span)
            | Site::Character(_, span)
            | Site::Raw(_, span) => *span,
            Site::Var { var, .. } => var.span,
        }
    }
}

/// AST から名前の現れを集める。
struct Collector<'s> {
    source: &'s str,
    out: Vec<Site>,
}

impl Collector<'_> {
    fn scene(&mut self, scene: &Scene) {
        if let Some(id) = &scene.id {
            self.out.push(Site::SceneId(id.value.clone(), id.span));
        }
        for stmt in &scene.body {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Dialogue(d) => {
                // 閉じ括弧の後ろに文が続く行は地の文（仕様書6節）なので、話者として扱わない。
                if !d.has_trailing_text() {
                    self.out
                        .push(Site::Character(d.speaker.value.clone(), d.speaker.span));
                }
                for line in &d.lines {
                    self.text(line);
                }
                if let Some(trailing) = &d.trailing {
                    self.text(trailing);
                }
            }
            Stmt::Narration(n) => self.text(&n.text),
            Stmt::Break(_) => {}
            Stmt::Raw(raw) => self.out.push(Site::Raw(raw.lines.join("\n"), raw.span)),
            Stmt::Directive(d) => match &d.kind {
                DirectiveKind::Show { character, .. } | DirectiveKind::Hide { character, .. } => {
                    self.out
                        .push(Site::Character(character.value.clone(), character.span));
                }
                DirectiveKind::Set {
                    assignment: Some(assignment),
                    ..
                } => {
                    self.var(&assignment.target);
                    self.expr(&assignment.value);
                }
                DirectiveKind::Call { target } => {
                    self.out
                        .push(Site::SceneRef(target.value.clone(), target.span));
                }
                _ => {}
            },
            Stmt::Jump(j) => self
                .out
                .push(Site::SceneRef(j.target.value.clone(), j.target.span)),
            Stmt::Choice(c) => {
                if let Some(prompt) = &c.prompt {
                    self.text(prompt);
                }
                for option in &c.options {
                    self.text(&option.text);
                    if let Some(cond) = &option.cond {
                        self.source_expr(cond);
                    }
                    if let Some(jump) = &option.jump {
                        self.out.push(Site::SceneRef(jump.value.clone(), jump.span));
                    }
                    for stmt in &option.body {
                        self.stmt(stmt);
                    }
                }
            }
            Stmt::If(block) => {
                for branch in &block.branches {
                    self.source_expr(&branch.cond);
                    for stmt in &branch.body {
                        self.stmt(stmt);
                    }
                }
                if let Some(else_branch) = &block.else_branch {
                    for stmt in &else_branch.body {
                        self.stmt(stmt);
                    }
                }
            }
        }
    }

    fn text(&mut self, text: &Text) {
        for part in &text.parts {
            if let Inline::Interpolation { expr, .. } = part {
                self.source_expr(expr);
            }
        }
    }

    fn source_expr(&mut self, expr: &SourceExpr) {
        if let Some(expr) = &expr.expr {
            self.expr(expr);
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Var(var) => self.var(var),
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Int(_) | ExprKind::Str(_) | ExprKind::Bool(_) => {}
        }
    }

    fn var(&mut self, var: &VarRef) {
        let span = var.span;
        let written = self.source.get(span.start..span.end).unwrap_or_default();
        let (namespace, name) = match (&var.namespace, written.rfind('.')) {
            (Some(_), Some(dot)) => {
                let ns = written.get(..dot).unwrap_or_default().trim_end();
                let rest = written.get(dot + 1..).unwrap_or_default();
                let lead = rest.len() - rest.trim_start().len();
                (
                    Some(Span::new(span.start, span.start + ns.len())),
                    Span::new(span.start + dot + 1 + lead, span.end),
                )
            }
            _ => (None, span),
        };
        self.out.push(Site::Var {
            var: var.clone(),
            namespace,
            name,
        });
    }
}

/// 本文の名前の現れ（位置の順）。
fn sites(project: &Project, source: &str) -> Vec<Site> {
    let parsed = parse(source, &project.parse_options());
    let mut collector = Collector {
        source,
        out: Vec::new(),
    };
    for scene in &parsed.chapter.scenes {
        collector.scene(scene);
    }
    let mut out = collector.out;
    out.sort_by_key(|s| s.span().start);
    out
}

fn character_of<'p>(project: &'p Project, name: &str) -> Option<&'p str> {
    match resolve_character_in(&project.characters, name) {
        Resolution::Found(id) => Some(id),
        _ => None,
    }
}

/// 変数のキー（仕様書14節：名前空間が無ければ `flag`、キャラクターなら ID）。
fn var_key(project: &Project, var: &VarRef) -> String {
    let namespace = match &var.namespace {
        None => "flag",
        Some(ns) => character_of(project, ns).unwrap_or(ns),
    };
    format!("{namespace}.{}", var.name)
}

/// カーソルのところ（バイト位置）の、名前を変えられるもの。
pub fn symbol_at(project: &Project, file: &str, offset: usize) -> Option<Symbol> {
    let source = &project.files.iter().find(|f| f.path == file)?.text;
    let inside = |span: Span| span.start <= offset && offset <= span.end;
    let site = sites(project, source)
        .into_iter()
        .find(|s| inside(s.span()))?;
    let character = |text: &str| -> Option<Symbol> {
        let id = character_of(project, text)?;
        let entry = project.characters.iter().find(|c| c.id == id)?;
        if text == id {
            let type_id = project.bible.character_type().to_owned();
            Some(Symbol {
                target: RenameTarget::EntryId {
                    type_id,
                    id: id.to_owned(),
                },
                current: id.to_owned(),
                label: format!("キャラクターのID（{}）", entry.name),
            })
        } else {
            Some(Symbol {
                target: RenameTarget::CharacterName { id: id.to_owned() },
                current: entry.name.clone(),
                label: format!("キャラクターの名前（{id}）"),
            })
        }
    };
    match site {
        Site::SceneId(id, _) | Site::SceneRef(id, _) => Some(Symbol {
            label: "シーンID".to_owned(),
            current: id.clone(),
            target: RenameTarget::Scene { id },
        }),
        Site::Character(text, _) => character(&text),
        Site::Raw(..) => None,
        Site::Var { var, namespace, .. } => {
            if let (Some(ns_span), Some(ns)) = (namespace, &var.namespace)
                && inside(ns_span)
                && let Some(symbol) = character(ns)
            {
                return Some(symbol);
            }
            let key = var_key(project, &var);
            Some(Symbol {
                label: "変数".to_owned(),
                current: key.clone(),
                target: RenameTarget::Variable { key },
            })
        }
    }
}

/// 名前の変更を組み立てる途中の、設定ファイルの中身（パスごと）。
struct Settings<'a> {
    root: &'a Path,
    texts: BTreeMap<String, String>,
}

impl Settings<'_> {
    /// 今の中身（書き換えた後のものがあればそれ）。無い・読めなければ `None`。
    fn get(&self, file: &str) -> Option<String> {
        if let Some(text) = self.texts.get(file) {
            return Some(text.clone());
        }
        fs::read(self.root.join(file))
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    }

    fn set(&mut self, file: &str, text: String) {
        self.texts.insert(file.to_owned(), text);
    }

    /// フロントマターの1つの項目を書き換える（`set_field`）。書き換えられなければ誤りの文。
    fn set_field(&mut self, file: &str, key: &str, value: &Value) -> Result<(), String> {
        let Some(text) = self.get(file) else {
            return Err(format!("{file} を読めません"));
        };
        let next = set_field(&text, key, value).map_err(|e| format!("{file}：{e}"))?;
        self.set(file, next);
        Ok(())
    }
}

/// 値の中の `old` を `new` にする（文字列か、文字列のリスト）。変わらなければ `None`。
fn replaced(value: &Value, old: &str, new: &str) -> Option<Value> {
    match value {
        Value::String(s) if s == old => Some(Value::String(new.to_owned())),
        Value::Array(items) if items.iter().any(|v| v.as_str() == Some(old)) => Some(Value::Array(
            items
                .iter()
                .map(|v| {
                    if v.as_str() == Some(old) {
                        Value::String(new.to_owned())
                    } else {
                        v.clone()
                    }
                })
                .collect(),
        )),
        _ => None,
    }
}

/// 設定DBの `ref<target>` の項目（リストを含む）の、`old` を指す値を `new` にする。
fn rename_refs(
    project: &Project,
    settings: &mut Settings<'_>,
    target: &str,
    old: &str,
    new: &str,
) -> Result<(), String> {
    for entry in &project.bible.entries {
        let Some(ty) = project.bible.types.iter().find(|t| t.id == entry.type_id) else {
            continue;
        };
        for field in &ty.fields {
            if field.kind.ref_target() != Some(target) {
                continue;
            }
            if let Some(value) = entry
                .fields
                .get(&field.key)
                .and_then(|v| replaced(v, old, new))
            {
                settings.set_field(&entry.path, &field.key, &value)?;
            }
        }
    }
    Ok(())
}

/// アイデアメモの `promoted_to` が `old`（`scene/ID` か `型/ID`）なら `new` にする。
fn rename_promoted(settings: &mut Settings<'_>, old: &str, new: &str) -> Result<(), String> {
    for (file, text) in idea_files(settings.root) {
        let Some(text) = text else { continue };
        let Ok(fields) = front_fields(&text) else {
            continue;
        };
        if fields.get("promoted_to").and_then(Value::as_str) == Some(old) {
            settings.set_field(&file, "promoted_to", &Value::String(new.to_owned()))?;
        }
    }
    Ok(())
}

/// 行の並び（改行を含む）。
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// 行の字下げ（半角空白とタブ）の長さ。
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// 行が `key:` で始まるなら、キーの書き方の長さ（引用符で囲んだものも）。
fn key_len(body: &str, key: &str) -> Option<usize> {
    for written in [
        key.to_owned(),
        serde_json::to_string(key).unwrap_or_default(),
        format!("'{}'", key.replace('\'', "''")),
    ] {
        if let Some(rest) = body.strip_prefix(written.as_str())
            && let Some(after) = rest.strip_prefix(':')
            && (after.is_empty() || after.starts_with([' ', '\t', '\r', '\n']))
        {
            return Some(written.len());
        }
    }
    None
}

/// YAML のキーの行を差し替える：`section` が `None` なら字下げの無いキー、あれば `section:` の下の1段目のキー。
/// `olds` のどれかに当てはまる最初のキーを `new` にする。変わらなければ `None`。
fn rename_yaml_key(
    text: &str,
    section: Option<&str>,
    olds: &[String],
    new: &str,
) -> Option<String> {
    let all = lines(text);
    let mut out = String::with_capacity(text.len() + new.len());
    let mut inside = section.is_none();
    let mut level: Option<usize> = None;
    let mut done = false;
    for line in all {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start_matches([' ', '\t']);
        let blank = trimmed.is_empty() || trimmed.starts_with('#');
        if let Some(section) = section
            && !blank
            && indent(line) == 0
        {
            inside = key_len(body, section).is_some();
            level = None;
            out.push_str(line);
            continue;
        }
        if !done && inside && !blank {
            let here = indent(line);
            let want = match section {
                None => 0,
                Some(_) => *level.get_or_insert(here),
            };
            if here == want
                && let Some(len) = olds.iter().find_map(|old| key_len(trimmed, old))
            {
                out.push_str(line.get(..here).unwrap_or_default());
                out.push_str(&yaml_scalar(new));
                out.push_str(trimmed.get(len..).unwrap_or_default());
                out.push_str(line.get(body.len()..).unwrap_or_default());
                done = true;
                continue;
            }
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// `project.toml` の `start_scene = "old"` を `new` にする。変わらなければ `None`。
fn rename_start_scene(text: &str, old: &str, new: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut done = false;
    for line in lines(text) {
        let body = line.trim_end_matches(['\n', '\r']);
        let trimmed = body.trim_start();
        if !done
            && let Some(rest) = trimmed.strip_prefix("start_scene")
            && let Some(value) = rest.trim_start().strip_prefix('=')
        {
            // シーンIDは英数字と `_` なので、値の中に `#` は無い。`#` から後ろはコメント。
            let value = value.trim();
            let (value, comment) = match value.find('#') {
                Some(at) => (
                    value.get(..at).unwrap_or_default().trim_end(),
                    value.get(at..).unwrap_or_default(),
                ),
                None => (value, ""),
            };
            if value == format!("\"{old}\"") || value == format!("'{old}'") {
                let head = body.len() - trimmed.len();
                out.push_str(body.get(..head).unwrap_or_default());
                out.push_str(&format!("start_scene = \"{new}\""));
                if !comment.is_empty() {
                    out.push(' ');
                    out.push_str(comment);
                }
                out.push_str(line.get(body.len()..).unwrap_or_default());
                done = true;
                continue;
            }
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// フロントマターの `id:` の行の値を `new` にする。`id` が無ければ `None`。
fn rename_front_id(text: &str, new: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut in_front = false;
    let mut done = false;
    for (n, line) in lines(text).into_iter().enumerate() {
        let body = line.trim_end_matches(['\n', '\r']);
        if body.trim_start_matches('\u{feff}').trim_end() == "---" {
            if n == 0 {
                in_front = true;
            } else if in_front {
                in_front = false;
            }
            out.push_str(line);
            continue;
        }
        if in_front
            && !done
            && let Some(len) = key_len(body, "id")
        {
            let rest = body.get(len + 1..).unwrap_or_default();
            let comment = rest
                .find(" #")
                .map_or("", |at| rest.get(at..).unwrap_or_default());
            out.push_str(&format!("id: {}{comment}", yaml_scalar(new)));
            out.push_str(line.get(body.len()..).unwrap_or_default());
            done = true;
            continue;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// 本文の置き換えを当てる（位置の後ろから）。
fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
    let mut out = text.to_owned();
    for edit in edits.iter().rev() {
        if out.is_char_boundary(edit.span.start) && out.is_char_boundary(edit.span.end) {
            out.replace_range(edit.span.start..edit.span.end, &edit.text);
        }
    }
    out
}

fn is_id(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 名前の変更を組み立てる。名前が正しくない、もう使われている、設定ファイルを書き換えられないときは誤りの文。
pub fn plan_rename(
    project: &Project,
    root: &Path,
    target: &RenameTarget,
    new: &str,
) -> Result<RenamePlan, String> {
    let new = new.trim();
    let all_sites: Vec<(String, &str, Vec<Site>)> = project
        .files
        .iter()
        .map(|f| (f.path.clone(), f.text.as_str(), sites(project, &f.text)))
        .collect();
    let mut settings = Settings {
        root,
        texts: BTreeMap::new(),
    };
    let mut moves = Vec::new();
    let mut layout = None;
    let mut notes = Vec::new();
    let every = || all_sites.iter().flat_map(|(_, _, s)| s);
    // 生コードの中の古い名前は変えない（知らせる）。
    let raw_note = |old: &str| -> Option<String> {
        let n: usize = every()
            .map(|s| match s {
                Site::Raw(text, _) => text.matches(old).count(),
                _ => 0,
            })
            .sum();
        (n > 0).then(|| format!("生コード（@raw）の中の「{old}」{n}か所は変えません"))
    };
    let (profile, _) = read_profile(root);
    // 本文の置き換え：現れごとに、新しい書き方（変えなければ `None`）。
    let edit_of: EditOf = match target {
        RenameTarget::Scene { id: old } => {
            if !is_id(new) {
                return Err("シーンIDは英数字と _ で書きます".to_owned());
            }
            if every().any(|s| matches!(s, Site::SceneId(id, _) if id == new)) {
                return Err(format!("シーンID {new} はもう使われています"));
            }
            // 行き先の無いジャンプがあれば、そのジャンプがこのシーンへ飛ぶようになるので断る。
            if every().any(|s| matches!(s, Site::SceneRef(id, _) if id == new)) {
                return Err(format!(
                    "行き先の無いジャンプ・呼び出し（{new}）があります。そのままでは、それがこのシーンを指すようになります"
                ));
            }
            if project.settings.start_scene.as_deref() == Some(old.as_str()) {
                let next = settings
                    .get("project.toml")
                    .and_then(|text| rename_start_scene(&text, old, new))
                    .ok_or("project.toml の start_scene を書き換えられません。ソースで直してから変えてください")?;
                settings.set("project.toml", next);
            }
            notes.extend(raw_note(old));
            rename_refs(project, &mut settings, "scene", old, new)?;
            rename_promoted(
                &mut settings,
                &format!("scene/{old}"),
                &format!("scene/{new}"),
            )?;
            layout = Some(LayoutRename {
                level: "scene".to_owned(),
                from: old.clone(),
                to: new.to_owned(),
            });
            let (old, new) = (old.clone(), new.to_owned());
            Box::new(move |site| match site {
                Site::SceneId(id, span) | Site::SceneRef(id, span) if *id == old => {
                    Some(TextEdit {
                        span: *span,
                        text: new.clone(),
                    })
                }
                _ => None,
            })
        }
        RenameTarget::CharacterName { id } => {
            let Some(character) = project.characters.iter().find(|c| c.id == *id) else {
                return Err(format!("キャラクター {id} が設定DBにありません"));
            };
            check_character_name(project, id, new)?;
            let as_namespace = every().any(|s| {
                matches!(s, Site::Var { var, .. }
                    if var.namespace.as_deref() == Some(character.name.as_str()))
            });
            check_character_text(project, &all_sites, id, new, as_namespace)?;
            notes.extend(raw_note(&character.name));
            let Some(entry) = project
                .bible
                .entries
                .iter()
                .find(|e| e.type_id == project.bible.character_type() && e.id == *id)
            else {
                return Err(format!("キャラクター {id} のエントリがありません"));
            };
            settings.set_field(&entry.path, "name", &Value::String(new.to_owned()))?;
            let (old, new) = (character.name.clone(), new.to_owned());
            let (characters, id) = (project.characters.clone(), id.clone());
            Box::new(move |site| character_edit(site, &characters, &id, &old, &new))
        }
        RenameTarget::Variable { key: old } => {
            let mut diags = Vec::new();
            let parsed = parse_condition(Piece::new(new, 0), &mut diags);
            let Some(Expr {
                kind: ExprKind::Var(new_var),
                ..
            }) = parsed.expr
            else {
                return Err("変数の名前は「名前」か「名前空間.名前」で書きます".to_owned());
            };
            if !diags.is_empty() {
                return Err("変数の名前は「名前」か「名前空間.名前」で書きます".to_owned());
            }
            let new_key = var_key(project, &new_var);
            if new_key == *old {
                return Err("同じ名前です".to_owned());
            }
            let used = project.variables.iter().any(|v| v.key == new_key)
                || all_sites.iter().flat_map(|(_, _, s)| s).any(
                    |s| matches!(s, Site::Var { var, .. } if var_key(project, var) == new_key),
                );
            if used {
                return Err(format!("変数 {new_key} はもう使われています"));
            }
            if project.variables.iter().any(|v| v.key == *old) {
                let mut olds = vec![old.clone()];
                if let Some(bare) = old.strip_prefix("flag.") {
                    olds.push(bare.to_owned());
                }
                let next = settings
                    .get("variables.yaml")
                    .and_then(|text| rename_yaml_key(&text, None, &olds, &new_key))
                    .ok_or("variables.yaml の宣言を書き換えられません。ソースで直してから変えてください")?;
                settings.set("variables.yaml", next);
            }
            if profile.variables.contains_key(old.as_str()) {
                let next = settings
                    .get(RENPY_PROFILE)
                    .and_then(|text| {
                        rename_yaml_key(&text, Some("variables"), std::slice::from_ref(old), &new_key)
                    })
                    .ok_or("Ren'Py の出力プロファイル（export/renpy.yaml）の variables を書き換えられません。ソースで直してから変えてください")?;
                settings.set(RENPY_PROFILE, next);
            }
            notes.extend(raw_note(
                old.split_once('.').map_or(old.as_str(), |(_, n)| n),
            ));
            let (old_ns, _) = old.split_once('.').unwrap_or(("flag", old));
            let (new_ns, new_name) = new_key.split_once('.').unwrap_or(("flag", &new_key));
            let same_ns = old_ns == new_ns;
            let (old, new_key, new_name) = (old.clone(), new_key.clone(), new_name.to_owned());
            let new_ns = new_ns.to_owned();
            let project_chars = project.characters.clone();
            Box::new(move |site| {
                let Site::Var {
                    var,
                    namespace,
                    name,
                } = site
                else {
                    return None;
                };
                let ns = match &var.namespace {
                    None => "flag".to_owned(),
                    Some(ns) => match resolve_character_in(&project_chars, ns) {
                        Resolution::Found(id) => id.to_owned(),
                        _ => ns.clone(),
                    },
                };
                if format!("{ns}.{}", var.name) != old {
                    return None;
                }
                if same_ns {
                    return Some(TextEdit {
                        span: *name,
                        text: new_name.clone(),
                    });
                }
                let written = if new_ns == "flag" && namespace.is_none() {
                    new_name.clone()
                } else {
                    new_key.clone()
                };
                Some(TextEdit {
                    span: var.span,
                    text: written,
                })
            })
        }
        RenameTarget::EntryId { type_id, id: old } => {
            if !is_id(new) {
                return Err("IDは英数字と _ で書きます".to_owned());
            }
            if new == old {
                return Err("同じIDです".to_owned());
            }
            let bible = &project.bible;
            let Some(entry) = bible
                .entries
                .iter()
                .find(|e| e.type_id == *type_id && e.id == *old)
            else {
                return Err(format!("エントリ {type_id}/{old} がありません"));
            };
            let to = format!("bible/entries/{type_id}/{new}.md");
            let taken = bible
                .entries
                .iter()
                .any(|e| e.type_id == *type_id && e.id == new)
                || root.join(&to).exists();
            if taken {
                return Err(format!("{type_id}/{new} はもう使われています"));
            }
            let is_character = type_id == bible.character_type();
            if is_character
                && project
                    .characters
                    .iter()
                    .any(|c| c.id != *old && (c.name == new || c.aliases.iter().any(|a| a == new)))
            {
                return Err(format!(
                    "{new} はほかのキャラクターの名前か呼び名なので、IDにすると話者が分からなくなります"
                ));
            }
            if is_character {
                let as_namespace = project
                    .variables
                    .iter()
                    .any(|v| v.key.starts_with(&format!("{old}.")))
                    || every().any(|s| {
                        matches!(s, Site::Var { var, .. } if var.namespace.as_deref() == Some(old.as_str()))
                    });
                check_character_text(project, &all_sites, old, new, as_namespace)?;
                notes.extend(raw_note(old));
                if profile
                    .voice
                    .as_deref()
                    .is_some_and(|v| v.contains("{char_id}"))
                {
                    notes.push(
                        "ボイスのファイル名（{char_id}）が新しいIDで書き出されます。録ったファイルの名前は変えません"
                            .to_owned(),
                    );
                }
            }
            rename_refs(project, &mut settings, type_id, old, new)?;
            rename_promoted(
                &mut settings,
                &format!("{type_id}/{old}"),
                &format!("{type_id}/{new}"),
            )?;
            if let Some(text) = settings.get(&entry.path)
                && let Some(next) = rename_front_id(&text, new)
            {
                settings.set(&entry.path, next);
            }
            if is_character {
                // 書き換えられなければ断る（変数の名前空間、素材カタログと Ren'Py のキーがずれるため）。
                let cannot = |file: &str| {
                    format!(
                        "{file} のキャラクター {old} を書き換えられません。ソースで直してから変えてください"
                    )
                };
                let names: BTreeSet<String> = project
                    .variables
                    .iter()
                    .filter_map(|v| v.key.strip_prefix(&format!("{old}.")).map(str::to_owned))
                    .collect();
                for name in &names {
                    let next = settings
                        .get("variables.yaml")
                        .and_then(|text| {
                            rename_yaml_key(
                                &text,
                                None,
                                &[format!("{old}.{name}")],
                                &format!("{new}.{name}"),
                            )
                        })
                        .ok_or_else(|| cannot("variables.yaml"))?;
                    settings.set("variables.yaml", next);
                }
                let profile_vars: Vec<String> = profile
                    .variables
                    .keys()
                    .filter(|k| k.starts_with(&format!("{old}.")))
                    .cloned()
                    .collect();
                for key in &profile_vars {
                    let renamed = format!("{new}{}", key.get(old.len()..).unwrap_or_default());
                    let next = settings
                        .get(RENPY_PROFILE)
                        .and_then(|text| {
                            rename_yaml_key(
                                &text,
                                Some("variables"),
                                std::slice::from_ref(key),
                                &renamed,
                            )
                        })
                        .ok_or_else(|| cannot(RENPY_PROFILE))?;
                    settings.set(RENPY_PROFILE, next);
                }
                let sections = [
                    (
                        "assets/catalog.yaml",
                        project.catalog.characters.contains_key(old.as_str()),
                    ),
                    (RENPY_PROFILE, profile.characters.contains_key(old.as_str())),
                ];
                for (file, has) in sections {
                    if !has {
                        continue;
                    }
                    let next = settings
                        .get(file)
                        .and_then(|text| {
                            rename_yaml_key(
                                &text,
                                Some("characters"),
                                std::slice::from_ref(old),
                                new,
                            )
                        })
                        .ok_or_else(|| cannot(file))?;
                    settings.set(file, next);
                }
                if let Some(text) = settings.get(CONCEPT_FILE)
                    && let Ok(fields) = front_fields(&text)
                    && let Some(value) = fields
                        .get("main_characters")
                        .and_then(|v| replaced(v, old, new))
                {
                    settings.set_field(CONCEPT_FILE, "main_characters", &value)?;
                }
            }
            // エントリのファイル名を変える（書き換えた中身は、新しいパスで書く）。
            let text = settings
                .texts
                .remove(&entry.path)
                .or_else(|| settings.get(&entry.path));
            if let Some(text) = text {
                settings.set(&to, text);
            }
            moves.push(Move {
                from: entry.path.clone(),
                to,
            });
            let (old, new) = (old.clone(), new.to_owned());
            let characters = project.characters.clone();
            Box::new(move |site| {
                if is_character {
                    character_edit(site, &characters, &old, &old, &new)
                } else {
                    None
                }
            })
        }
    };
    let mut story = Vec::new();
    let mut story_edits = 0;
    for (file, text, file_sites) in &all_sites {
        let edits: Vec<TextEdit> = file_sites.iter().filter_map(&edit_of).collect();
        if edits.is_empty() {
            continue;
        }
        story_edits += edits.len();
        story.push(StoryChange {
            file: file.clone(),
            text: apply_edits(text, &edits),
            edits,
        });
    }
    let settings: Vec<FileChange> = settings
        .texts
        .into_iter()
        .map(|(file, text)| FileChange { file, text })
        .collect();
    Ok(RenamePlan {
        story,
        settings,
        moves,
        layout,
        story_edits,
        notes,
    })
}

/// キャラクター `id` を `old` と書いた話者・立ち絵・変数の名前空間を `new` にする。照合の段（仕様書6節）で
/// `id` に決まるものだけ（同じ名前の別のキャラクターや、解決できない話者は変えない）。
fn character_edit(
    site: &Site,
    characters: &[Character],
    id: &str,
    old: &str,
    new: &str,
) -> Option<TextEdit> {
    let resolves = |text: &str| matches!(resolve_character_in(characters, text), Resolution::Found(x) if x == id);
    match site {
        Site::Character(text, span) if text == old && resolves(text) => Some(TextEdit {
            span: *span,
            text: new.to_owned(),
        }),
        Site::Var {
            var,
            namespace: Some(span),
            ..
        } if var.namespace.as_deref() == Some(old) && resolves(old) => Some(TextEdit {
            span: *span,
            text: new.to_owned(),
        }),
        _ => None,
    }
}

/// キャラクターの新しい名前かIDが、本文で使えるかを確かめる：話者として書ける（`新「…」` が台詞候補になる）、
/// `as_namespace` なら変数の名前空間として書ける（`新.x` が変数になる）。また、まだ登録していない話者や、
/// キャラクターでない名前空間と同じ名前にすると、それがこのキャラクターのものになるので断る。
fn check_character_text(
    project: &Project,
    all_sites: &[(String, &str, Vec<Site>)],
    id: &str,
    new: &str,
    as_namespace: bool,
) -> Result<(), String> {
    // 作品の台詞の括弧（`[dialogue.brackets]`。読めなければ既定）の最初の組で試す。
    let options = project.parse_options();
    let (open, close) = options
        .brackets
        .first()
        .map_or(('「', '」'), |b| (b.open, b.close));
    let probe = format!("## a #zz\n{new}{open}x{close}\n");
    let parsed = parse(&probe, &options);
    let speaker_ok = parsed
        .chapter
        .scenes
        .first()
        .and_then(|s| s.body.first())
        .is_some_and(|stmt| matches!(stmt, Stmt::Dialogue(d) if d.speaker.value == new && !d.has_trailing_text()));
    if !speaker_ok {
        return Err(format!(
            "「{new}」は台詞の話者として書けません（記号で始まる、など）"
        ));
    }
    if as_namespace {
        let mut diags = Vec::new();
        let probe = format!("{new}.x");
        let parsed = parse_condition(Piece::new(&probe, 0), &mut diags);
        let ok = diags.is_empty()
            && matches!(parsed.expr, Some(Expr { kind: ExprKind::Var(v), .. }) if v.namespace.as_deref() == Some(new));
        if !ok {
            return Err(format!(
                "「{new}」は変数の名前空間として書けません（本文で「{new}.名前」と書けないため）"
            ));
        }
    }
    let unresolved = |text: &str| !matches!(resolve_character_in(&project.characters, text), Resolution::Found(x) if x == id);
    for (_, _, sites) in all_sites {
        for site in sites {
            match site {
                Site::Character(text, _) if text == new && unresolved(text) => {
                    return Err(format!(
                        "本文に、まだ登録していない話者「{new}」の台詞があります。そのままでは、その台詞がこのキャラクターのものになります"
                    ));
                }
                Site::Var { var, .. }
                    if var.namespace.as_deref() == Some(new) && unresolved(new) =>
                {
                    return Err(format!(
                        "本文に、名前空間「{new}」の変数があります。そのままでは、その変数がこのキャラクターのものになります"
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// キャラクターの新しい名前を確かめる：空でなく、20字まで、空白と括弧を含まない（話者として書けるもの。仕様書
/// 6節）。ほかのキャラクターのID・名前・呼び名と重ならない。
fn check_character_name(project: &Project, id: &str, new: &str) -> Result<(), String> {
    let brackets: Vec<char> = project
        .settings
        .brackets
        .iter()
        .flat_map(|b| [b.open, b.close])
        .chain(['（', '）'])
        .collect();
    if new.is_empty()
        || new.chars().count() > 20
        || new
            .chars()
            .any(|c| c.is_whitespace() || brackets.contains(&c))
    {
        return Err(
            "名前は、空白と括弧を含まない20字までで書きます（台詞の話者として書けるもの）"
                .to_owned(),
        );
    }
    let clash = project.characters.iter().any(|c| {
        c.id != id && (c.id == new || c.name == new || c.aliases.iter().any(|a| a == new))
    });
    if clash {
        return Err(format!("{new} はほかのキャラクターのID・名前・呼び名です"));
    }
    if project
        .characters
        .iter()
        .any(|c| c.id == id && c.name == new)
    {
        return Err("同じ名前です".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_keys() {
        let text = "# 変数\nflag.勇気:\n  type: int\n勇気2: 1\n";
        assert_eq!(
            rename_yaml_key(text, None, &["flag.勇気".to_owned()], "flag.覚悟").unwrap(),
            "# 変数\nflag.覚悟:\n  type: int\n勇気2: 1\n"
        );
        let catalog =
            "backgrounds:\n  akari: {}\ncharacters:\n  akari:\n    dir: a/\n  tsumugi: {}\n";
        assert_eq!(
            rename_yaml_key(catalog, Some("characters"), &["akari".to_owned()], "akari2").unwrap(),
            "backgrounds:\n  akari: {}\ncharacters:\n  akari2:\n    dir: a/\n  tsumugi: {}\n"
        );
        assert!(rename_yaml_key(catalog, Some("characters"), &["x".to_owned()], "y").is_none());
    }

    #[test]
    fn start_scene_and_front_id() {
        assert_eq!(
            rename_start_scene(
                "title = \"a\"\nstart_scene = \"s001\"  # 最初\n",
                "s001",
                "s100"
            )
            .unwrap(),
            "title = \"a\"\nstart_scene = \"s100\" # 最初\n"
        );
        assert!(rename_start_scene("start_scene = \"s0012\"\n", "s001", "s100").is_none());
        assert_eq!(
            rename_front_id(
                "---\r\nid: akari\r\nname: 灯\r\n---\r\n本文 id: x\r\n",
                "akari2"
            )
            .unwrap(),
            "---\r\nid: akari2\r\nname: 灯\r\n---\r\n本文 id: x\r\n"
        );
        assert!(rename_front_id("---\nname: 灯\n---\nid: x\n", "y").is_none());
    }
}
