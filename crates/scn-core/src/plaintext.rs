//! プレーンテキストの書き出し（T2-5、D056）：通読（`route::read`）の本文を、ルート単位の通読用のテキストにする。
//!
//! - 小説の形にする：話者名は書かず、台詞は表示スタイルの括弧で囲む。地の文は書かれたとおり（段落頭の全角空白を
//!   含む）。シーンの見出しは `◆ 題名`。
//! - ルビはなろう・カクヨムの記法（`｜親文字《読み》`）、傍点はカクヨムの記法（`《《傍点》》`）。`{w}`・`{p}`・
//!   `{speed}`・文中の効果音は書かない。値の決まらない変数の埋め込みは `{=式}` のまま。本文の `《` は、ルビと
//!   読まれないように `｜《` と書く（なろう・カクヨムの記法）。
//! - 注記（`notes`）：選んだ選択肢を `〔選択：表示文〕`、エンディングを `〔エンディング：ID〕` と書く。
//! - 文字コードは UTF-8（BOM なし）、改行は LF。

use std::fs;
use std::path::{Path, PathBuf};

use crate::ir::IrInline;
use crate::project::{Project, ProjectError, write_source};
use crate::route::{Reading, ReadingItem};

/// プレーンテキストの書き出し先（作品フォルダから）。
pub const TEXT_DIR: &str = "export/text";

/// 本文の `《` を、ルビの記法と読まれないように `｜《` にする。
fn escape(text: &str) -> String {
    text.replace('《', "｜《")
}

/// 文中の記法を、通読用の文字にする。
fn inline_text(text: &[IrInline]) -> String {
    let mut out = String::new();
    for inline in text {
        match inline {
            IrInline::Text { text } => out.push_str(&escape(text)),
            IrInline::Ruby { base, reading } => {
                out.push('｜');
                out.push_str(base);
                out.push('《');
                out.push_str(reading);
                out.push('》');
            }
            IrInline::Emphasis { text } => {
                out.push_str("《《");
                out.push_str(text);
                out.push_str("》》");
            }
            IrInline::Interpolation { expr_text, .. } => {
                out.push_str("{=");
                out.push_str(expr_text);
                out.push('}');
            }
            IrInline::LineBreak => out.push('\n'),
            IrInline::Wait { .. }
            | IrInline::PageBreak
            | IrInline::Se { .. }
            | IrInline::Speed { .. } => {}
        }
    }
    out
}

/// 通読の本文を、プレーンテキストにする。`label` は道の名前（見出しに書く）。
pub fn plain_text(project: &Project, reading: &Reading, label: &str, notes: bool) -> String {
    let brackets = |style: &str| {
        project
            .settings
            .brackets
            .iter()
            .find(|b| b.style == style)
            .map_or(('「', '」'), |b| (b.open, b.close))
    };
    let mut lines: Vec<String> = Vec::new();
    let title = project.settings.title.as_deref().unwrap_or("");
    lines.push(if title.is_empty() {
        label.to_owned()
    } else {
        format!("{title}（{label}）")
    });
    let blank = |lines: &mut Vec<String>| {
        if lines.last().is_some_and(|l| !l.is_empty()) {
            lines.push(String::new());
        }
    };
    for item in &reading.items {
        match item {
            ReadingItem::Scene { id, title, .. } => {
                blank(&mut lines);
                lines.push(format!("◆ {}", if title.is_empty() { id } else { title }));
                lines.push(String::new());
            }
            ReadingItem::Say { style, text, .. } => {
                let (open, close) = brackets(style);
                lines.push(format!("{open}{}{close}", inline_text(text)));
            }
            ReadingItem::Narrate { text, .. } => lines.push(inline_text(text)),
            ReadingItem::Break => blank(&mut lines),
            ReadingItem::Choice {
                options, chosen, ..
            } => {
                if notes && let Some(option) = options.get(*chosen) {
                    lines.push(format!("〔選択：{}〕", inline_text(&option.text)));
                }
            }
            ReadingItem::Ending { id } => {
                if notes {
                    blank(&mut lines);
                    lines.push(format!("〔エンディング：{}〕", id.as_deref().unwrap_or("")));
                }
            }
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// ファイル名に使えない文字を `_` にする。
fn file_name(label: &str) -> String {
    let name: String = label
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim().trim_start_matches('.').to_owned();
    if name.is_empty() {
        "text".to_owned()
    } else {
        name
    }
}

/// プレーンテキストを作品フォルダの `export/text/<道の名前>.txt` に書く。書いたパスを返す。
pub fn write_plain_text(root: &Path, label: &str, text: &str) -> Result<PathBuf, ProjectError> {
    let relative = format!("{TEXT_DIR}/{}.txt", file_name(label));
    let path = root.join(&relative);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    write_source(root, &relative, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_double_angle_bracket() {
        let text = [
            IrInline::Text {
                text: "本《題名》".to_owned(),
            },
            IrInline::Ruby {
                base: "灯".to_owned(),
                reading: "あかり".to_owned(),
            },
        ];
        assert_eq!(inline_text(&text), "本｜《題名》｜灯《あかり》");
    }
}
