//! 設定DBの型定義のフォームでの編集（T2-10、D061）：`bible/types/<型>.yaml` の `fields` の項目を足す・直す
//! （名前・型・選択肢）・並べ替える・消す。項目は1行の `- { key: …, label: …, type: … }` で書かれたものだけを
//! 扱い、直した項目の行のほかは変えない。書き換えた後に読み直して、`fields` のほかは何も変わっていないことを
//! 確かめる。複数行で書いた項目、1行の `fields: [ … ]` などは書かずに `EditError::Unsupported`（ソースで直す）。
//! 項目の `key` は変えない（エントリが使っているため）。直す・動かす・消す項目は `key` で指す（アプリの項目の
//! 一覧は、読めない項目を読み飛ばしているので、順番では指さない）。

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::bible::{EditError, FieldType, newline_of, scalar_inline};

/// 型定義の編集。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeEdit {
    /// 項目を足す（最後に）。
    Add {
        key: String,
        label: String,
        #[serde(rename = "type")]
        ty: String,
        #[serde(default)]
        options: Vec<String>,
    },
    /// 項目の名前・型・選択肢を直す。
    Set {
        key: String,
        label: String,
        #[serde(rename = "type")]
        ty: String,
        #[serde(default)]
        options: Vec<String>,
    },
    /// 項目を動かす（`delta` が負なら前へ。読めない項目も1つと数える）。
    Move { key: String, delta: i64 },
    /// 項目を消す（エントリの値は消さない）。
    Remove { key: String },
}

fn parse(text: &str) -> Result<Map<String, Value>, String> {
    let value: yaml_serde::Value = yaml_serde::from_str(text).map_err(|e| e.to_string())?;
    match serde_json::to_value(&value).map_err(|e| e.to_string())? {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        _ => Err("型定義は対応表で書きます".to_owned()),
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn blank_or_comment(line: &str) -> bool {
    let body = line.trim_matches([' ', '\t', '\r', '\n']);
    body.is_empty() || body.starts_with('#')
}

/// 項目の1行の書き方（`- { key: …, label: …, type: … }`）。
fn item_line(pad: &str, key: &str, label: &str, ty: &str, options: &[String], nl: &str) -> String {
    let mut parts = vec![
        format!("key: {}", scalar_inline(key, true)),
        format!("label: {}", scalar_inline(label, true)),
        format!("type: {}", scalar_inline(ty, true)),
    ];
    if !options.is_empty() {
        let list: Vec<String> = options.iter().map(|o| scalar_inline(o, true)).collect();
        parts.push(format!("options: [{}]", list.join(", ")));
    }
    format!("{pad}- {{ {} }}{nl}", parts.join(", "))
}

/// 項目の値（`{key, label, type, options}`）。
fn item_value(key: &str, label: &str, ty: &str, options: &[String]) -> Value {
    let mut map = Map::new();
    map.insert("key".to_owned(), Value::String(key.to_owned()));
    map.insert("label".to_owned(), Value::String(label.to_owned()));
    map.insert("type".to_owned(), Value::String(ty.to_owned()));
    if !options.is_empty() {
        map.insert(
            "options".to_owned(),
            Value::Array(options.iter().cloned().map(Value::String).collect()),
        );
    }
    Value::Object(map)
}

/// 1行の `- { … }` の項目の、閉じの `}` の位置。引用符の中の括弧と、入れ子の `{ }`・`[ ]` は数えない
/// （行末のコメントに `}` があっても、項目の閉じを取り違えない）。
fn closing_brace(body: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    // 引用符は、値の始まり（`{`・`[`・`,`・`:` の後ろ）でだけ開く（`it's` の `'` は引用符ではない）。
    let mut prev = ' ';
    for (i, c) in body.char_indices() {
        match quote {
            Some('"') => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    quote = None;
                }
            }
            Some(_) => {
                if c == '\'' {
                    quote = None;
                }
            }
            None => {
                match c {
                    '"' | '\'' if matches!(prev, '{' | '[' | ',' | ':') => quote = Some(c),
                    '{' | '[' => depth += 1,
                    '}' | ']' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 && c == '}' {
                            return Some(i);
                        }
                    }
                    '#' if depth == 0 => return None,
                    _ => {}
                }
                if !c.is_whitespace() {
                    prev = c;
                }
            }
        }
    }
    None
}

/// 項目の検査：名前は空でない1行、型は読める型、選択肢の要る型（`select`・`multiselect`）には選択肢がある。
fn check_item(label: &str, ty: &str, options: &[String]) -> Result<(), EditError> {
    if label.trim().is_empty() || label.contains(['\n', '\r']) {
        return Err(EditError::Invalid("項目の名前を1行で書きます".to_owned()));
    }
    match FieldType::parse(ty) {
        None => Err(EditError::Invalid(format!("型「{ty}」を読めません"))),
        Some(FieldType::Select | FieldType::Multiselect) if options.is_empty() => Err(
            EditError::Invalid("select・multiselect には選択肢（options）が要ります".to_owned()),
        ),
        Some(_) => Ok(()),
    }
}

/// 型定義を編集する。直した行のほかは変えない。
pub fn edit_type(text: &str, edit: &TypeEdit) -> Result<String, EditError> {
    // 先頭の BOM は除いて書き換え、戻す（T2-10）。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return edit_type(rest, edit).map(|next| format!("\u{feff}{next}"));
    }
    let before = parse(text).map_err(EditError::Unreadable)?;
    let fields: Vec<Value> = match before.get("fields") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(EditError::Unsupported),
    };
    let nl = newline_of(text);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    // `fields:` の行（字下げ無し）。
    let head = lines.iter().position(|l| {
        indent(l) == 0 && {
            let body = l.trim_end_matches(['\r', '\n']);
            body.strip_prefix("fields:").is_some_and(|rest| {
                let rest = rest.split(" #").next().unwrap_or_default().trim();
                rest.is_empty() || rest == "[]"
            })
        }
    });
    // 項目の行（`fields:` の後ろの、字下げした `- ` の行）。どれも1行の対応表であること。
    let mut items: Vec<usize> = Vec::new();
    let mut end = head.map_or(lines.len(), |h| h + 1);
    if let Some(h) = head {
        let mut i = h + 1;
        while let Some(line) = lines.get(i) {
            if blank_or_comment(line) {
                i += 1;
                continue;
            }
            if indent(line) == 0 && !line.trim_start().starts_with('-') {
                break;
            }
            let body = line
                .trim_start_matches([' ', '\t'])
                .trim_end_matches(['\r', '\n']);
            if !body.starts_with("- {")
                || !body
                    .split(" #")
                    .next()
                    .unwrap_or(body)
                    .trim_end()
                    .ends_with('}')
            {
                return Err(EditError::Unsupported);
            }
            items.push(i);
            i += 1;
            end = i;
        }
    }
    if items.len() != fields.len() {
        return Err(EditError::Unsupported);
    }
    let pad = items
        .first()
        .and_then(|&i| lines.get(i))
        .map_or("  ".to_owned(), |l| {
            l.get(..indent(l)).unwrap_or("  ").to_owned()
        });
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    let mut expected = fields.clone();
    // `key` の項目の順番（1つだけのとき）。
    let position = |key: &str| -> Result<usize, EditError> {
        let mut found = fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.get("key").and_then(Value::as_str) == Some(key));
        match (found.next(), found.next()) {
            (Some((i, _)), None) => Ok(i),
            (None, _) => Err(EditError::Invalid(format!("項目 {key} がありません"))),
            (Some(_), Some(_)) => Err(EditError::Invalid(format!(
                "項目 {key} が2つあります。ソースで直してください"
            ))),
        }
    };
    match edit {
        TypeEdit::Add {
            key,
            label,
            ty,
            options,
        } => {
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(EditError::Invalid(
                    "項目のキーは英数字と _ で書きます".to_owned(),
                ));
            }
            if ["id", "name"].contains(&key.as_str())
                || fields
                    .iter()
                    .any(|f| f.get("key").and_then(Value::as_str) == Some(key))
            {
                return Err(EditError::Invalid(format!(
                    "キー {key} はもう使われています"
                )));
            }
            check_item(label, ty, options)?;
            let line = item_line(&pad, key, label, ty, options, nl);
            match head {
                Some(h) => {
                    // `fields: []` は `fields:` にしてから足す。
                    if let Some(slot) = out.get_mut(h)
                        && slot.contains("[]")
                    {
                        *slot = format!("fields:{nl}");
                    }
                    let at = items.last().map_or(h + 1, |&i| i + 1).min(end).max(h + 1);
                    // 前の行が改行で終わっていなければ（ファイルの終わり）、改行を足してから足す。
                    if let Some(prev) = at.checked_sub(1).and_then(|p| out.get_mut(p))
                        && !prev.ends_with('\n')
                    {
                        prev.push_str(nl);
                    }
                    out.insert(at, line);
                }
                None => {
                    let mut tail = String::new();
                    if !text.is_empty() && !text.ends_with('\n') {
                        tail.push_str(nl);
                    }
                    tail.push_str(&format!("fields:{nl}{line}"));
                    out.push(tail);
                }
            }
            expected.push(item_value(key, label, ty, options));
        }
        TypeEdit::Set {
            key,
            label,
            ty,
            options,
        } => {
            let index = position(key)?;
            let (Some(&line_at), Some(old)) = (items.get(index), fields.get(index)) else {
                return Err(EditError::Invalid(format!("項目 {key} がありません")));
            };
            check_item(label, ty, options)?;
            // 項目にあった、ほかのキー（name・label・type・options 以外）は書き直せないので断る。
            let extra = old.as_object().is_some_and(|m| {
                m.keys()
                    .any(|k| !["key", "label", "type", "options"].contains(&k.as_str()))
            });
            if extra {
                return Err(EditError::Unsupported);
            }
            let line = lines.get(line_at).copied().unwrap_or_default();
            let ending = if line.ends_with('\n') { nl } else { "" };
            let item_pad = line.get(..indent(line)).unwrap_or_default();
            // `}` の後ろの行末のコメントは残す。
            let body = line.trim_end_matches(['\r', '\n']);
            let comment = closing_brace(body)
                .and_then(|close| body.get(close + 1..))
                .filter(|rest| rest.trim_start().starts_with('#'))
                .unwrap_or_default();
            if let Some(slot) = out.get_mut(line_at) {
                *slot = format!(
                    "{}{comment}{ending}",
                    item_line(item_pad, key, label, ty, options, "")
                );
            }
            if let Some(slot) = expected.get_mut(index) {
                *slot = item_value(key, label, ty, options);
            }
        }
        TypeEdit::Move { key, delta } => {
            let index = position(key)?;
            // 読めない項目（key か type が無い・読めない。アプリの一覧に出ない）は飛ばして、隣の読める項目と
            // 入れ替える。
            let readable: Vec<usize> = fields
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    // 読み込みと同じく、key は文字列・数・真偽で読める。
                    f.get("key")
                        .is_some_and(|k| k.is_string() || k.is_number() || k.is_boolean())
                        && f.get("type")
                            .and_then(Value::as_str)
                            .and_then(FieldType::parse)
                            .is_some()
                })
                .map(|(i, _)| i)
                .collect();
            let to = readable
                .iter()
                .position(|&i| i == index)
                .and_then(|at| i64::try_from(at).ok())
                .and_then(|at| at.checked_add(*delta))
                .and_then(|at| usize::try_from(at).ok())
                .and_then(|at| readable.get(at).copied());
            let Some(to) = to else {
                return Err(EditError::Invalid("動かせません".to_owned()));
            };
            let (Some(&a), Some(&b)) = (items.get(index), items.get(to)) else {
                return Err(EditError::Invalid("動かせません".to_owned()));
            };
            let line_a = lines.get(a).copied().unwrap_or_default();
            let line_b = lines.get(b).copied().unwrap_or_default();
            // 終わりの改行の無い最後の行と入れ替えるときは、改行をそろえる。
            let fix = |line: &str, last: bool| -> String {
                let body = line.trim_end_matches(['\r', '\n']);
                if last {
                    body.to_owned()
                } else {
                    format!("{body}{nl}")
                }
            };
            let last = |i: usize| !lines.get(i).is_some_and(|l| l.ends_with('\n'));
            if let Some(slot) = out.get_mut(a) {
                *slot = fix(line_b, last(a));
            }
            if let Some(slot) = out.get_mut(b) {
                *slot = fix(line_a, last(b));
            }
            expected.swap(index, to);
        }
        TypeEdit::Remove { key } => {
            let index = position(key)?;
            let Some(&line_at) = items.get(index) else {
                return Err(EditError::Invalid(format!("項目 {key} がありません")));
            };
            out.remove(line_at);
            expected.remove(index);
        }
    }
    let next: String = out.concat();
    let mut want = before;
    want.insert("fields".to_owned(), Value::Array(expected));
    // 書き換えた結果を読めなければ、ほかの書き方（ソースで直す）にしてもらう。
    let mut got = parse(&next).map_err(|_| EditError::Unsupported)?;
    // 項目が無くなったら `fields:` は空（null）になる。
    if got.get("fields") == Some(&Value::Null) {
        got.insert("fields".to_owned(), Value::Array(Vec::new()));
    }
    if got == want {
        Ok(next)
    } else {
        Err(EditError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "name: 伏線\n# 項目\nfields:\n  - { key: setup,  label: 仕込み, type: list<ref<scene>> }\n  - { key: payoff, label: 回収,   type: ref<scene> }   # 回収\n";

    fn set(key: &str, label: &str, ty: &str) -> TypeEdit {
        TypeEdit::Set {
            key: key.to_owned(),
            label: label.to_owned(),
            ty: ty.to_owned(),
            options: Vec::new(),
        }
    }

    #[test]
    fn edits_only_that_line() {
        let next = edit_type(TEXT, &set("payoff", "回収のシーン", "ref<scene>")).unwrap();
        assert_eq!(
            next,
            TEXT.replace(
                "  - { key: payoff, label: 回収,   type: ref<scene> }   # 回収\n",
                "  - { key: payoff, label: 回収のシーン, type: ref<scene> }   # 回収\n"
            )
        );
        let next = edit_type(
            TEXT,
            &TypeEdit::Add {
                key: "note".to_owned(),
                label: "メモ".to_owned(),
                ty: "select".to_owned(),
                options: vec!["甲".to_owned(), "a, b".to_owned()],
            },
        )
        .unwrap();
        assert_eq!(
            next,
            format!(
                "{TEXT}  - {{ key: note, label: メモ, type: select, options: [甲, \"a, b\"] }}\n"
            )
        );
        let next = edit_type(
            TEXT,
            &TypeEdit::Move {
                key: "payoff".to_owned(),
                delta: -1,
            },
        )
        .unwrap();
        let lines: Vec<&str> = next.lines().collect();
        assert!(lines[3].contains("payoff") && lines[4].contains("setup"));
        let next = edit_type(
            TEXT,
            &TypeEdit::Remove {
                key: "setup".to_owned(),
            },
        )
        .unwrap();
        assert!(!next.contains("setup") && next.contains("payoff"));
        // 項目の無い型と、`fields: []` に足す。
        for text in [
            "name: 場所\n",
            "name: 場所\nfields: []\n",
            "name: 場所\nfields:\n",
        ] {
            let next = edit_type(
                text,
                &TypeEdit::Add {
                    key: "a".to_owned(),
                    label: "A".to_owned(),
                    ty: "text".to_owned(),
                    options: Vec::new(),
                },
            )
            .unwrap();
            assert!(
                next.ends_with("fields:\n  - { key: a, label: A, type: text }\n"),
                "{next}"
            );
        }
    }

    #[test]
    fn refuses() {
        let block = "name: x\nfields:\n  - key: a\n    label: A\n    type: text\n";
        assert_eq!(
            edit_type(block, &set("a", "B", "text")),
            Err(EditError::Unsupported)
        );
        let flow = "name: x\nfields: [{ key: a, label: A, type: text }]\n";
        assert_eq!(
            edit_type(flow, &set("a", "B", "text")),
            Err(EditError::Unsupported)
        );
        assert!(edit_type(TEXT, &set("setup", "", "text")).is_err());
        assert!(edit_type(TEXT, &set("setup", "A", "txt")).is_err());
        assert!(edit_type(TEXT, &set("setup", "A", "select")).is_err());
        assert!(edit_type(TEXT, &set("nothing", "A", "text")).is_err());
        assert!(
            edit_type(
                TEXT,
                &TypeEdit::Move {
                    key: "setup".to_owned(),
                    delta: -1
                }
            )
            .is_err()
        );
        // 読めない項目（key の無いもの）があっても、key で指した項目を直す。
        let skipped = "name: x\nfields:\n  - { label: 壊れ, type: text }\n  - { key: b, label: B, type: text }\n";
        let next = edit_type(skipped, &set("b", "Bの名前", "text")).unwrap();
        assert!(
            next.ends_with("  - { key: b, label: Bの名前, type: text }\n"),
            "{next}"
        );
        let dup = "name: x\nfields:\n  - { key: b, label: B, type: text }\n  - { key: b, label: C, type: text }\n";
        assert!(edit_type(dup, &set("b", "D", "text")).is_err());
        // 読めない項目は飛ばして、隣の読める項目と入れ替える。
        let between = "name: x\nfields:\n  - { key: a, label: A, type: text }\n  - { key: z, label: Z, type: txt }\n  - { key: b, label: B, type: text }\n";
        let next = edit_type(
            between,
            &TypeEdit::Move {
                key: "b".to_owned(),
                delta: -1,
            },
        )
        .unwrap();
        assert_eq!(
            next,
            "name: x\nfields:\n  - { key: b, label: B, type: text }\n  - { key: z, label: Z, type: txt }\n  - { key: a, label: A, type: text }\n"
        );
        assert!(
            edit_type(
                between,
                &TypeEdit::Move {
                    key: "a".to_owned(),
                    delta: i64::MAX
                }
            )
            .is_err()
        );
        // 行末のコメントに `}` があっても、コメントを残す。引用符の中の `}` も閉じと取り違えない。
        let commented =
            "name: x\nfields:\n  - { key: a, label: \"A}\", type: text }  # memo {x} }\n";
        let next = edit_type(commented, &set("a", "B", "text")).unwrap();
        assert_eq!(
            next,
            "name: x\nfields:\n  - { key: a, label: B, type: text }  # memo {x} }\n"
        );
        // 終わりの改行の無いファイルにも足す。
        for text in [
            "name: x\nfields:\n  - { key: a, label: A, type: text }",
            "name: x\nfields:",
        ] {
            let next = edit_type(
                text,
                &TypeEdit::Add {
                    key: "n".to_owned(),
                    label: "A".to_owned(),
                    ty: "text".to_owned(),
                    options: Vec::new(),
                },
            )
            .unwrap();
            assert!(
                next.ends_with("\n  - { key: n, label: A, type: text }\n"),
                "{next}"
            );
        }
        let add = |key: &str| TypeEdit::Add {
            key: key.to_owned(),
            label: "A".to_owned(),
            ty: "text".to_owned(),
            options: Vec::new(),
        };
        assert!(edit_type(TEXT, &add("setup")).is_err());
        assert!(edit_type(TEXT, &add("name")).is_err());
        assert!(edit_type(TEXT, &add("a-b")).is_err());
    }
}
