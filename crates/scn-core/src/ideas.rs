//! アイデアメモ（`ideas/*.md`、T2-6、D057）：メモの読み方、昇格先の解決、昇格と新しいメモの中身。
//!
//! - メモは `ideas/` の直下の `.md`（ファイル名の順。隠しファイルと下のフォルダは読まない）。
//! - フロントマターの `status`（未整理・採用・保留・没）、`tags`、`promoted_to`（`scene/<シーンID>` か
//!   `<型ID>/<エントリのID>`）。`status` が無ければ未整理。知らない値も未整理として扱い、知らせる。
//! - 書き換え（項目、本文、昇格）は `workspace` が受け持つ。ここでは書く中身だけを作る。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bible::{EditError, body_of, front_fields, scalar_text, set_field};
use crate::diag::Span;
use crate::ir::Ir;
use crate::project::Project;

/// メモのフォルダ（作品フォルダから）。
pub const IDEAS_DIR: &str = "ideas";
/// メモの状態（この順に並べる）。
pub const STATUSES: [&str; 4] = ["未整理", "採用", "保留", "没"];
/// 状態が無い・知らない値のときの状態。
pub const UNSORTED: &str = "未整理";
/// 昇格したメモの状態。
pub const ADOPTED: &str = "採用";

/// 伏線の型（`bible/types/foreshadow.yaml`）が無いとき、伏線への昇格で作る標準の定義（project-files 5節）。
/// 型のテンプレートの伏線と同じ（T4-9）。
pub const FORESHADOW_TYPE_TEXT: &str = include_str!("../templates/types/foreshadow.yaml");

/// 昇格先（`promoted_to`）を解いたもの。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdeaTarget {
    Scene {
        id: String,
        title: String,
        file: String,
        span: Span,
    },
    Entry {
        type_id: String,
        /// 型の名前（型定義の `name`。無ければ型ID）。
        type_name: String,
        id: String,
        /// エントリの `name`（無ければID）。
        name: String,
        path: String,
    },
    /// 見つからない（書き誤り、消したシーンやエントリ）。
    Missing { value: String },
}

/// アイデアメモ1枚。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Idea {
    /// 作品フォルダからのパス（`ideas/…`）。
    pub file: String,
    /// ファイルの中身（書き換えのときに「知っている中身」として渡す）。UTF-8で読めなければ `None`。
    pub text: Option<String>,
    /// 状態（`STATUSES` のどれか）。
    pub status: String,
    pub tags: Vec<String>,
    pub promoted_to: Option<String>,
    pub target: Option<IdeaTarget>,
    /// フロントマターの後ろの本文。
    pub body: String,
    /// 読み方の知らせ（読めないフロントマター、知らない状態など）。
    pub problems: Vec<String>,
}

/// 昇格のしかた（アプリから受け取る）。伏線は、型 `foreshadow` のエントリ。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Promotion {
    /// 設定DBのエントリを作る（本文はメモの本文）。
    Entry {
        type_id: String,
        id: String,
        name: String,
    },
    /// 章（`story/` の下の `*.scn`）の末尾にスタブのシーンを足す（要約はメモの最初の行）。
    Scene {
        file: String,
        id: String,
        title: String,
    },
}

impl Promotion {
    /// メモの `promoted_to` に書く値。
    pub fn promoted_to(&self) -> String {
        match self {
            Promotion::Entry { type_id, id, .. } => format!("{type_id}/{id}"),
            Promotion::Scene { id, .. } => format!("scene/{id}"),
        }
    }
}

/// `ideas/` の直下の `.md`（ファイル名の順）と、その中身（UTF-8で読めなければ `None`）。
pub fn idea_files(root: &Path) -> Vec<(String, Option<String>)> {
    let Ok(entries) = fs::read_dir(root.join(IDEAS_DIR)) else {
        return Vec::new();
    };
    let mut files: Vec<(String, Option<String>)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.ends_with(".md") && !name.starts_with('.')).then(|| {
                let text = fs::read(e.path())
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok());
                (format!("{IDEAS_DIR}/{name}"), text)
            })
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// メモのパス（`ideas/<名前>.md`）か。
pub fn is_idea_path(file: &str) -> bool {
    match file.split('/').collect::<Vec<_>>().as_slice() {
        [IDEAS_DIR, name] => {
            name.ends_with(".md") && !name.starts_with('.') && !name.contains('\\')
        }
        _ => false,
    }
}

/// 1枚のメモを読む（昇格先は解かない）。
pub fn read_idea(file: &str, text: Option<&str>) -> Idea {
    let mut idea = Idea {
        file: file.to_owned(),
        text: text.map(str::to_owned),
        status: UNSORTED.to_owned(),
        tags: Vec::new(),
        promoted_to: None,
        target: None,
        body: String::new(),
        problems: Vec::new(),
    };
    let Some(text) = text else {
        idea.problems.push("UTF-8で読めません".to_owned());
        return idea;
    };
    body_of(text).clone_into(&mut idea.body);
    let fields = match front_fields(text) {
        Ok(fields) => fields,
        Err(e) => {
            idea.problems
                .push(format!("フロントマターをYAMLとして読めません（{e}）"));
            return idea;
        }
    };
    match fields.get("status") {
        None | Some(Value::Null) => {}
        Some(value) => match scalar_text(value) {
            Some(s) if STATUSES.contains(&s.as_str()) => idea.status = s,
            _ => idea.problems.push(format!(
                "status「{}」は 未整理・採用・保留・没 のどれかで書きます（未整理として扱います）",
                scalar_text(value).unwrap_or_else(|| value.to_string())
            )),
        },
    }
    match fields.get("tags") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            idea.tags = items.iter().filter_map(scalar_text).collect();
        }
        Some(value) => idea.tags.extend(scalar_text(value)),
    }
    idea.promoted_to = fields
        .get("promoted_to")
        .and_then(scalar_text)
        .filter(|s| !s.trim().is_empty());
    idea
}

/// 昇格先（`scene/<シーンID>` か `<型ID>/<エントリのID>`）を解く。
pub fn resolve_target(project: &Project, ir: &Ir, value: &str) -> IdeaTarget {
    let missing = || IdeaTarget::Missing {
        value: value.to_owned(),
    };
    let Some((kind, id)) = value.trim().split_once('/') else {
        return missing();
    };
    if kind == "scene" {
        return ir
            .scenes
            .iter()
            .find(|s| s.id == id)
            .map_or_else(missing, |s| IdeaTarget::Scene {
                id: s.id.clone(),
                title: s.title.clone(),
                file: s.file.clone(),
                span: s.span,
            });
    }
    let bible = &project.bible;
    let Some(entry) = bible
        .entries
        .iter()
        .find(|e| e.type_id == kind && e.id == id)
    else {
        return missing();
    };
    let type_name = bible
        .types
        .iter()
        .find(|t| t.id == kind)
        .map_or_else(|| kind.to_owned(), |t| t.name.clone());
    IdeaTarget::Entry {
        type_id: entry.type_id.clone(),
        type_name,
        id: entry.id.clone(),
        name: entry
            .fields
            .get("name")
            .and_then(scalar_text)
            .unwrap_or_else(|| entry.id.clone()),
        path: entry.path.clone(),
    }
}

/// 作品のアイデアメモ（ファイル名の順。昇格先を解いたもの）。
pub fn ideas(root: &Path, project: &Project, ir: &Ir) -> Vec<Idea> {
    idea_files(root)
        .into_iter()
        .map(|(file, text)| {
            let mut idea = read_idea(&file, text.as_deref());
            idea.target = idea
                .promoted_to
                .as_deref()
                .map(|value| resolve_target(project, ir, value));
            idea
        })
        .collect()
}

/// スタブのシーンの要約：メモの本文の、最初の空でない行（前後の半角空白とタブを除く。全角空白は本文の一部と
/// して残す）。メタ情報の値として読み違えるもの（`[…]` のリスト、`---`）は書かない。
pub fn summary_line(body: &str) -> Option<String> {
    let line = body
        .lines()
        .map(|l| l.trim_matches([' ', '\t']))
        .find(|l| !l.trim_matches('\u{3000}').is_empty())?;
    if (line.starts_with('[') && line.ends_with(']')) || line.trim_end() == "---" {
        return None;
    }
    Some(line.to_owned())
}

/// 章の末尾に足すスタブのシーン（見出しと、要約だけのメタ情報）。前の行と空行で分ける。`text` は今の章の
/// 中身で、改行は章の改行にそろえる。
pub fn stub_scene(text: &str, title: &str, id: &str, summary: Option<&str>) -> String {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        out.push_str(nl);
    }
    // 中身が無いか、もう空行で終わっていれば、空行を足さない。
    let blank_end =
        text.trim_end_matches(['\r', '\n']).is_empty() || text.ends_with(&format!("{nl}{nl}"));
    if !blank_end {
        out.push_str(nl);
    }
    out.push_str(&format!("## {title} #{id}{nl}"));
    if let Some(summary) = summary {
        out.push_str(&format!("---{nl}summary: {summary}{nl}---{nl}"));
    }
    out
}

/// 新しいメモのファイル名：`ideas/idea-NNN.md`（使われていない最初の番号）。
pub fn new_idea_file(root: &Path) -> String {
    let taken: Vec<String> = idea_files(root).into_iter().map(|(f, _)| f).collect();
    let mut n = 1_u32;
    loop {
        let file = format!("{IDEAS_DIR}/idea-{n:03}.md");
        if !taken.contains(&file) && !root.join(&file).exists() {
            return file;
        }
        n += 1;
    }
}

/// 新しいメモの中身：`status: 未整理` と、タグ（あれば）と、本文（終わりに改行）。
pub fn new_idea_text(body: &str, tags: &[String]) -> Result<String, EditError> {
    let mut text = format!("---\nstatus: {UNSORTED}\n---\n");
    if !tags.is_empty() {
        text = set_field(
            &text,
            "tags",
            &Value::Array(tags.iter().cloned().map(Value::String).collect()),
        )?;
    }
    let body = body.trim_end_matches(['\n', '\r', ' ', '\t']);
    text.push_str(body);
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_scene_separates_with_a_blank_line() {
        assert_eq!(stub_scene("", "題", "s1", None), "## 題 #s1\n");
        assert_eq!(
            stub_scene("a\n", "題", "s1", Some("要約")),
            "\n## 題 #s1\n---\nsummary: 要約\n---\n"
        );
        assert_eq!(stub_scene("a", "題", "s1", None), "\n\n## 題 #s1\n");
        assert_eq!(stub_scene("a\n\n", "題", "s1", None), "## 題 #s1\n");
        assert_eq!(stub_scene("a\r\n", "題", "s1", None), "\r\n## 題 #s1\r\n");
    }

    #[test]
    fn summary_skips_list_like_lines() {
        assert_eq!(
            summary_line("\n  一行目 \n二行目"),
            Some("一行目".to_owned())
        );
        assert_eq!(summary_line("[a, b]\n"), None);
        assert_eq!(summary_line("---\n"), None);
        assert_eq!(summary_line(" \n"), None);
        assert_eq!(summary_line("　字下げ\n"), Some("　字下げ".to_owned()));
    }

    #[test]
    fn idea_paths() {
        assert!(is_idea_path("ideas/a.md"));
        assert!(!is_idea_path("ideas/.a.md"));
        assert!(!is_idea_path("ideas/sub/a.md"));
        assert!(!is_idea_path("ideas/a.txt"));
        assert!(!is_idea_path("concept.md"));
    }
}
