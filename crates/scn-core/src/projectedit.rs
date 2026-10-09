//! 環境設定の「この作品」の頁（T4-25、N6）：`project.toml` のキーを、コメントと並びを残して直す（`toml_edit`）。
//!
//! - 直せるのは決まったキーだけ（作品名、開始シーン、台詞の括弧、画面とメッセージウィンドウの大きさ、`[editor]`）。
//!   ほかのキー（`format_version`・`[ai.*]` など）は変えない。
//! - 値の形を確かめてから書く（正しくない値は書かない）。値を差し替えるときは、行末のコメントを残す。
//! - 書いた後の中身を読み直し（[`crate::project::read_settings`]）、直したキーが書いた値で読めることと、新しい
//!   診断（E802 など）が出ないことを確かめる。確かめられなければ書かない。

use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table, TableLike};

use crate::project::{Settings, read_settings};
use crate::syntax::ParseOptions;

/// 1つのキーの直し方。`value` が `None`（JSON の null）なら消す。
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SettingEdit {
    /// キーの道（`["title"]`、`["screen", "width"]`、`["dialogue", "brackets", "「」"]` など）。
    pub path: Vec<String>,
    pub value: Option<Value>,
}

/// 値の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 空でない文字列。
    Text,
    /// 正の整数。
    Positive,
    Bool,
    /// 表ごと消すだけ（`[screen]`・`[message_window]`）。
    RemoveTable,
}

/// 直せるキーと、値の種類。台詞の括弧（`["dialogue", "brackets", 括弧]`）は別に扱う。
fn kind_of(path: &[&str]) -> Option<Kind> {
    Some(match path {
        ["title"] | ["start_scene"] => Kind::Text,
        ["screen", "width" | "height"] | ["message_window", "lines" | "chars_per_line"] => {
            Kind::Positive
        }
        ["editor", "normalize_ellipsis" | "indent_narration"] => Kind::Bool,
        ["screen"] | ["message_window"] => Kind::RemoveTable,
        _ => return None,
    })
}

/// `project.toml` の中身 `text` に、`edits` をまとめて当てた中身。直せないキー、正しくない値、読み直して確かめ
/// られないときは、誤りの文。
pub fn edit_project_toml(text: &str, edits: &[SettingEdit]) -> Result<String, String> {
    // 先頭の BOM は外して直し、書くときに戻す。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return edit_project_toml(rest, edits).map(|after| format!("\u{feff}{after}"));
    }
    let mut doc: DocumentMut = text.parse().map_err(|e| {
        format!(
            "project.toml を TOML として読めないので、ここでは直せません（ファイルを直します）：{e}"
        )
    })?;
    for edit in edits {
        apply(&mut doc, edit)?;
    }
    // `toml_edit` は改行を `\n` で書くので、`\r\n` のファイルは `\r\n` にそろえて戻す。
    let after = if text.contains("\r\n") {
        doc.to_string().replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        doc.to_string()
    };
    verify(text, &after, edits)?;
    Ok(after)
}

fn apply(doc: &mut DocumentMut, edit: &SettingEdit) -> Result<(), String> {
    let path: Vec<&str> = edit.path.iter().map(String::as_str).collect();
    if let ["dialogue", "brackets", pair] = path.as_slice() {
        return bracket(doc, pair, edit.value.as_ref());
    }
    let kind = kind_of(&path).ok_or_else(|| format!("{} はここでは直せません", path.join(".")))?;
    let (tables, key) = match path.split_last() {
        Some((key, tables)) => (tables, *key),
        None => return Err("キーがありません".to_owned()),
    };
    let Some(value) = &edit.value else {
        // 消す。
        match tables {
            [] => {
                doc.remove(key);
            }
            [name] => {
                if let Some(table) = table_mut(doc, name, false)? {
                    table.remove(key);
                    if table.is_empty() {
                        doc.remove(name);
                    }
                }
            }
            _ => return Err(format!("{} はここでは直せません", path.join("."))),
        }
        return Ok(());
    };
    let new = match (kind, value) {
        (Kind::Text, Value::String(s)) if !s.trim().is_empty() => toml_edit::Value::from(s.trim()),
        (Kind::Text, _) => return Err(format!("{key} は空でない文字で書きます")),
        (Kind::Positive, Value::Number(n))
            if n.as_u64()
                .is_some_and(|n| n > 0 && n <= u64::from(u32::MAX)) =>
        {
            toml_edit::Value::from(n.as_i64().unwrap_or_default())
        }
        (Kind::Positive, _) => return Err(format!("{key} は正の整数で書きます")),
        (Kind::Bool, Value::Bool(b)) => toml_edit::Value::from(*b),
        (Kind::Bool, _) => return Err(format!("{key} は true か false で書きます")),
        (Kind::RemoveTable, _) => return Err(format!("{key} は消すことだけができます")),
    };
    match tables {
        [] => set(doc.as_table_mut(), key, new),
        [name] => match table_mut(doc, name, true)? {
            Some(table) => set(table, key, new),
            None => Err(format!("{name} の表を作れません")),
        },
        _ => Err(format!("{} はここでは直せません", path.join("."))),
    }
}

/// 根の表の `name`（無ければ、`create` のときだけ作る）。表でなければ誤り。
fn table_mut<'d>(
    doc: &'d mut DocumentMut,
    name: &str,
    create: bool,
) -> Result<Option<&'d mut dyn TableLike>, String> {
    if !doc.contains_key(name) {
        if !create {
            return Ok(None);
        }
        doc.insert(name, Item::Table(Table::new()));
    }
    match doc.get_mut(name).and_then(Item::as_table_like_mut) {
        Some(table) => Ok(Some(table)),
        None => Err(format!(
            "{name} が表ではないので、ここでは直せません（ファイルを直します）"
        )),
    }
}

/// 値を入れる。今ある値は、行末のコメントなど飾りを残して差し替える。
fn set(table: &mut dyn TableLike, key: &str, mut new: toml_edit::Value) -> Result<(), String> {
    match table.get_mut(key) {
        Some(item) => match item.as_value_mut() {
            Some(old) => {
                *new.decor_mut() = old.decor().clone();
                *old = new;
                Ok(())
            }
            None => Err(format!(
                "{key} が値ではない（表など）ので、ここでは直せません（ファイルを直します）"
            )),
        },
        None => {
            table.insert(key, Item::Value(new));
            Ok(())
        }
    }
}

/// 台詞の括弧の組を、足す・表示スタイルを変える・消す。
fn bracket(doc: &mut DocumentMut, pair: &str, style: Option<&Value>) -> Result<(), String> {
    if pair.chars().count() != 2 {
        return Err(format!(
            "括弧「{pair}」は、開き括弧と閉じ括弧の2文字で書きます"
        ));
    }
    if !doc.contains_key("dialogue") {
        if style.is_none() {
            return Ok(());
        }
        doc.insert("dialogue", Item::Table(implicit_table()));
    }
    let dialogue = doc
        .get_mut("dialogue")
        .and_then(Item::as_table_like_mut)
        .ok_or("dialogue が表ではないので、ここでは直せません（ファイルを直します）")?;
    if dialogue.get("brackets").is_none() {
        if style.is_none() {
            return Ok(());
        }
        dialogue.insert("brackets", Item::Table(Table::new()));
    }
    let brackets = dialogue
        .get_mut("brackets")
        .and_then(Item::as_table_like_mut)
        .ok_or("[dialogue.brackets] が表ではないので、ここでは直せません（ファイルを直します）")?;
    // 表が無いか空なら既定の「」が効いているので、先に書いてから足す（「」の台詞を地の文にしないため）。
    if brackets.is_empty() && style.is_some() {
        for b in ParseOptions::default().brackets {
            brackets.insert(
                &format!("{}{}", b.open, b.close),
                Item::Value(toml_edit::Value::from(b.style)),
            );
        }
    }
    match style {
        None => {
            if brackets.len() <= 1 && brackets.contains_key(pair) {
                return Err("台詞の括弧は、1組は残します".to_owned());
            }
            brackets.remove(pair);
            Ok(())
        }
        Some(Value::String(s)) if !s.trim().is_empty() => {
            set(brackets, pair, toml_edit::Value::from(s.trim()))
        }
        Some(_) => Err("表示スタイルは英字の名前で書きます（normal・thought など）".to_owned()),
    }
}

/// 見出しを書かない表（`[dialogue.brackets]` だけを書くため）。
fn implicit_table() -> Table {
    let mut table = Table::new();
    table.set_implicit(true);
    table
}

/// 書いた後の中身を読み直し、直したキーが書いた値で読めて、新しい診断が出ないことを確かめる。
fn verify(before: &str, after: &str, edits: &[SettingEdit]) -> Result<(), String> {
    let (_, old) = read_settings(before);
    let (settings, new) = read_settings(after);
    let seen: Vec<(String, String)> = old
        .iter()
        .map(|d| (d.diag.code.to_string(), d.diag.message.clone()))
        .collect();
    if let Some(d) = new
        .iter()
        .find(|d| !seen.contains(&(d.diag.code.to_string(), d.diag.message.clone())))
    {
        return Err(format!(
            "書くと {} になるので、書きませんでした：{}",
            d.diag.code, d.diag.message
        ));
    }
    for edit in edits {
        if !reads_back(&settings, edit) {
            return Err(format!(
                "{} を書いた値で読めなかったので、書きませんでした",
                edit.path.join(".")
            ));
        }
    }
    let (earlier, _) = read_settings(before);
    if let Some(key) = untouched_change(&earlier, &settings, edits) {
        return Err(format!(
            "直していない {key} の読み方が変わるので、書きませんでした（ファイルを直します）"
        ));
    }
    Ok(())
}

/// 直していないキーで、読み方が変わったもの（無ければ `None`）。
fn untouched_change(before: &Settings, after: &Settings, edits: &[SettingEdit]) -> Option<String> {
    let edited = |prefix: &[&str]| {
        edits
            .iter()
            .any(|e| e.path.len() >= prefix.len() && e.path.iter().zip(prefix).all(|(a, b)| a == b))
    };
    let checks: [(&[&str], bool); 7] = [
        (
            &["format_version"],
            before.format_version != after.format_version,
        ),
        (&["title"], before.title != after.title),
        (&["start_scene"], before.start_scene != after.start_scene),
        (&["screen"], before.screen != after.screen),
        (
            &["message_window"],
            before.message_window != after.message_window,
        ),
        (
            &["editor", "normalize_ellipsis"],
            before.editor.normalize_ellipsis != after.editor.normalize_ellipsis,
        ),
        (
            &["editor", "indent_narration"],
            before.editor.indent_narration != after.editor.indent_narration,
        ),
    ];
    for (key, changed) in checks {
        if changed && !edited(key) {
            return Some(key.join("."));
        }
    }
    if before.ai != after.ai {
        return Some("ai".to_owned());
    }
    // 括弧は、直した組のほかが同じか。
    let others = |settings: &Settings| {
        let mut pairs: Vec<(char, char, String)> = settings
            .brackets
            .iter()
            .filter(|b| !edited(&["dialogue", "brackets", &format!("{}{}", b.open, b.close)]))
            .map(|b| (b.open, b.close, b.style.clone()))
            .collect();
        pairs.sort();
        pairs
    };
    (others(before) != others(after)).then(|| "dialogue.brackets".to_owned())
}

/// 直したキーが、書いた値で読めるか。
fn reads_back(settings: &Settings, edit: &SettingEdit) -> bool {
    let text = edit.value.as_ref().and_then(Value::as_str).map(str::trim);
    let number = edit
        .value
        .as_ref()
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    let flag = edit.value.as_ref().and_then(Value::as_bool);
    let path: Vec<&str> = edit.path.iter().map(String::as_str).collect();
    match path.as_slice() {
        ["title"] => settings.title.as_deref() == text,
        ["start_scene"] => settings.start_scene.as_deref() == text,
        ["screen"] => settings.screen.is_none(),
        ["message_window"] => settings.message_window.is_none(),
        ["screen", "width"] => settings.screen.map(|s| s.width) == number,
        ["screen", "height"] => settings.screen.map(|s| s.height) == number,
        ["message_window", "lines"] => settings.message_window.map(|m| m.lines) == number,
        ["message_window", "chars_per_line"] => {
            settings.message_window.map(|m| m.chars_per_line) == number
        }
        ["editor", "normalize_ellipsis"] => {
            settings.editor.normalize_ellipsis == flag.unwrap_or(false)
        }
        ["editor", "indent_narration"] => settings.editor.indent_narration == flag.unwrap_or(false),
        ["dialogue", "brackets", pair] => {
            let mut chars = pair.chars();
            let (open, close) = (chars.next(), chars.next());
            let found = settings
                .brackets
                .iter()
                .find(|b| Some(b.open) == open && Some(b.close) == close);
            match text {
                Some(style) => found.is_some_and(|b| b.style == style),
                None => found.is_none(),
            }
        }
        _ => false,
    }
}
