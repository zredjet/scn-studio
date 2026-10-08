//! 変数の宣言のフォームでの編集（T2-10、D061）：`variables.yaml` の1つの宣言の1つの項目を差し替える・足す・消す、
//! 宣言を足す。直した行のほかは変えない（`bible::set_field` と同じ考え）。書き換えた後に読み直して、その項目の
//! ほかは何も変わっていないことを確かめる。書けない書き方（1行の `{ … }` の宣言、複数行の値、行末のコメントの
//! 途中にある値など）なら、書かずに `EditError::Unsupported`（ソースで直す）。

use serde_json::Value;

use crate::bible::{EditError, comment_start, key_prefix, newline_of, scalar_inline};

/// 項目のキー（`type`・`values`・`default`・`range`・`scope`・`desc`）。
pub const DECL_FIELDS: [&str; 6] = ["type", "values", "default", "range", "scope", "desc"];

/// `variables.yaml` を読んだ対応表（変数名ごとの宣言）。読めなければ誤りの文。
fn parse(text: &str) -> Result<serde_json::Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    let value: yaml_serde::Value = yaml_serde::from_str(text).map_err(|e| e.to_string())?;
    match serde_json::to_value(&value).map_err(|e| e.to_string())? {
        Value::Null => Ok(serde_json::Map::new()),
        Value::Object(map) => Ok(map),
        _ => Err("変数名をキーにした対応表ではありません".to_owned()),
    }
}

/// 宣言の書かれたキー（`flag.勇気` は `勇気` とも書ける）。
fn written_key(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    if map.contains_key(key) {
        return Some(key.to_owned());
    }
    let bare = key.strip_prefix("flag.")?;
    map.contains_key(bare).then(|| bare.to_owned())
}

/// 値の1行の書き方（素のまま読めなければ JSON）。
fn inline(value: &Value) -> String {
    match value {
        Value::String(s) => scalar_inline(s, false),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| match item {
                    Value::String(s) => scalar_inline(s, true),
                    other => serde_json::to_string(other).unwrap_or_default(),
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn blank_or_comment(line: &str) -> bool {
    let body = line.trim_matches([' ', '\t', '\r', '\n']);
    body.is_empty() || body.starts_with('#')
}

/// 宣言 `key` の項目 `field` を `value` にする（`null` なら項目を消す）。ほかの行は変えない。
pub fn set_decl_field(
    text: &str,
    key: &str,
    field: &str,
    value: &Value,
) -> Result<String, EditError> {
    // 先頭の BOM は除いて書き換え、戻す（T2-10）。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return set_decl_field(rest, key, field, value).map(|next| format!("\u{feff}{next}"));
    }
    if !DECL_FIELDS.contains(&field) {
        return Err(EditError::ReadOnly(field.to_owned()));
    }
    let before = parse(text).map_err(EditError::Unreadable)?;
    let Some(written) = written_key(&before, key) else {
        return Err(EditError::Unsupported);
    };
    let nl = newline_of(text);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    // 宣言の行（字下げの無い `キー:` で、値が後ろの行にあるもの）。
    let Some(head) = lines.iter().position(|line| {
        indent(line) == 0
            && key_prefix(line, &written).is_some_and(|len| {
                let rest = line.get(len + 1..).unwrap_or_default();
                let rest = rest.trim_end_matches(['\r', '\n']);
                let value =
                    comment_start(rest).map_or(rest, |at| rest.get(..at).unwrap_or_default());
                value.trim().is_empty()
            })
    }) else {
        return Err(EditError::Unsupported);
    };
    // 宣言の中身の行（次の字下げの無い行まで）。
    let mut end = head + 1;
    while let Some(line) = lines.get(end) {
        if !blank_or_comment(line) && indent(line) == 0 {
            break;
        }
        end += 1;
    }
    let children: Vec<usize> = (head + 1..end)
        .filter(|&i| lines.get(i).is_some_and(|l| !blank_or_comment(l)))
        .collect();
    let child_indent = children
        .first()
        .and_then(|&i| lines.get(i))
        .map_or(2, |l| indent(l));
    let pad = children
        .first()
        .and_then(|&i| lines.get(i))
        .and_then(|l| l.get(..child_indent))
        .unwrap_or("  ")
        .to_owned();
    let found = children.iter().copied().find(|&i| {
        lines.get(i).is_some_and(|l| {
            indent(l) == child_indent
                && key_prefix(l.get(child_indent..).unwrap_or_default(), field).is_some()
        })
    });
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    match found {
        Some(i) => {
            // 値が次の行に続く（深い字下げの行がある）なら書き換えない。
            if let Some(&next) = children.iter().find(|&&j| j > i)
                && lines.get(next).is_some_and(|l| indent(l) > child_indent)
            {
                return Err(EditError::Unsupported);
            }
            if value.is_null() {
                out.remove(i);
            } else {
                let line = lines.get(i).copied().unwrap_or_default();
                let body = line.trim_end_matches(['\r', '\n']);
                // キーを引用符で書いた行（`"default": 0`）でも、`:` の後ろから行末のコメントを探す。
                let key_len = key_prefix(body.get(child_indent..).unwrap_or_default(), field)
                    .unwrap_or(field.len());
                let after_key = body.get(child_indent + key_len + 1..).unwrap_or_default();
                let comment = comment_start(after_key)
                    .map_or("", |at| after_key.get(at..).unwrap_or_default());
                let ending = line.get(body.len()..).unwrap_or_default();
                if let Some(slot) = out.get_mut(i) {
                    *slot = format!("{pad}{field}: {}{comment}{ending}", inline(value));
                }
            }
        }
        None if value.is_null() => return Ok(text.to_owned()),
        None => {
            // 最後の中身の行の後ろに足す（無ければ見出しの行の後ろ）。
            let at = children.last().copied().unwrap_or(head);
            let line = lines.get(at).copied().unwrap_or_default();
            if !line.ends_with('\n')
                && let Some(slot) = out.get_mut(at)
            {
                slot.push_str(nl);
            }
            out.insert(at + 1, format!("{pad}{field}: {}{nl}", inline(value)));
        }
    }
    let next: String = out.concat();
    let was_valid = valid_decl(before.get(&written));
    let mut expected = before;
    if let Some(Value::Object(decl)) = expected.get_mut(&written) {
        if value.is_null() {
            decl.remove(field);
        } else {
            decl.insert(field.to_owned(), value.clone());
        }
    }
    // 読み込みで読み飛ばされる宣言（型が無い・読めない、値の無い列挙）にはしない（フォームで戻せなくなる）。
    if was_valid && !valid_decl(expected.get(&written)) {
        let to_enum = value.as_str() == Some("enum");
        return Err(EditError::Invalid(if field == "type" && !to_enum {
            "型は bool・int・string・enum のどれかにします（消せません）".to_owned()
        } else if field == "type" {
            "列挙（enum）の変数には値（values）が要ります。先に値を入れてから型を enum にします"
                .to_owned()
        } else {
            "列挙（enum）の値は1つ以上要ります（値を入れ替えるときは、新しい値を足してから古い値を消します）"
                .to_owned()
        }));
    }
    if parse(&next).ok() == Some(expected) {
        Ok(next)
    } else {
        Err(EditError::Unsupported)
    }
}

/// 読み込みで読み飛ばされない宣言か（`type` が4つのどれかで、`enum` なら空でない `values` がある。
/// `project` の読み込みと同じ）。
fn valid_decl(decl: Option<&Value>) -> bool {
    let Some(Value::Object(decl)) = decl else {
        return false;
    };
    match decl.get("type").and_then(Value::as_str) {
        Some("bool" | "int" | "string") => true,
        Some("enum") => decl
            .get("values")
            .and_then(Value::as_array)
            .is_some_and(|v| {
                !v.is_empty()
                    && v.iter()
                        .all(|x| !x.is_object() && !x.is_array() && !x.is_null())
            }),
        _ => false,
    }
}

/// 宣言を足す：`key`（`名前空間.名前` か `名前`）の宣言を、ファイルの終わりに空行を挟んで足す。もうあれば断る。
/// `values` は列挙のときの値。
pub fn add_decl(text: &str, key: &str, ty: &str, values: &[String]) -> Result<String, String> {
    // 先頭の BOM は除いて書き換え、戻す（T2-10）。
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return add_decl(rest, key, ty, values).map(|next| format!("\u{feff}{next}"));
    }
    let before = parse(text)?;
    let key = key.trim();
    let normalized = if key.contains('.') {
        key.to_owned()
    } else {
        format!("flag.{key}")
    };
    let valid = match normalized.split_once('.') {
        Some((ns, name)) => !ns.is_empty() && !name.is_empty() && !name.contains('.'),
        None => false,
    };
    if !valid || key.contains(char::is_whitespace) {
        return Err("変数名は「名前空間.名前」か「名前」で、空白を含まずに書きます".to_owned());
    }
    if written_key(&before, &normalized).is_some() {
        return Err(format!("変数 {normalized} はもう宣言されています"));
    }
    if !matches!(ty, "bool" | "int" | "string" | "enum") {
        return Err("型は bool・int・string・enum のどれかです".to_owned());
    }
    if ty == "enum" && values.is_empty() {
        return Err("列挙の変数には値（values）が要ります".to_owned());
    }
    let nl = newline_of(text);
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(nl);
    }
    if !out.trim().is_empty() && !out.ends_with(&format!("{nl}{nl}")) {
        out.push_str(nl);
    }
    out.push_str(&format!(
        "{}:{nl}  type: {ty}{nl}",
        scalar_inline(&normalized, false)
    ));
    if ty == "enum" {
        let list = Value::Array(values.iter().cloned().map(Value::String).collect());
        out.push_str(&format!("  values: {}{nl}", inline(&list)));
    }
    let after = parse(&out)?;
    if after.len() != before.len() + 1 {
        return Err("宣言を足せません（ソースで直してください）".to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEXT: &str = "# 変数\nflag.勇気:\n  type: int\n  default: 0   # 最初\n  desc: 勇気\n\n恋:\n  type: bool\n";

    #[test]
    fn replaces_only_that_line() {
        let next = set_decl_field(TEXT, "flag.勇気", "default", &json!(3)).unwrap();
        assert_eq!(
            next,
            TEXT.replace("default: 0   # 最初", "default: 3   # 最初")
        );
        // 名前空間を省いて書いた宣言にも足す（最後の中身の行の後ろ）。
        let next = set_decl_field(TEXT, "flag.恋", "default", &json!(true)).unwrap();
        assert_eq!(next, format!("{TEXT}  default: true\n"));
        // 足す（空行の前）。
        let next = set_decl_field(TEXT, "flag.勇気", "range", &json!([0, 10])).unwrap();
        assert!(
            next.contains("  desc: 勇気\n  range: [0, 10]\n\n恋:"),
            "{next}"
        );
        // 消す。
        let next = set_decl_field(TEXT, "flag.勇気", "desc", &Value::Null).unwrap();
        assert_eq!(next, TEXT.replace("  desc: 勇気\n", ""));
        // 文字列は読み違えないように書く。
        let next = set_decl_field(TEXT, "flag.勇気", "desc", &json!("12")).unwrap();
        assert!(next.contains("  desc: \"12\"\n"), "{next}");
    }

    #[test]
    fn refuses_unsupported_shapes() {
        let flow = "flag.a: { type: int, default: 0 }\n";
        assert_eq!(
            set_decl_field(flow, "flag.a", "default", &json!(1)),
            Err(EditError::Unsupported)
        );
        let block = "flag.a:\n  type: enum\n  values:\n    - 甲\n    - 乙\n";
        assert_eq!(
            set_decl_field(block, "flag.a", "values", &json!(["甲"])),
            Err(EditError::Unsupported)
        );
        assert!(set_decl_field(TEXT, "flag.無い", "default", &json!(1)).is_err());
        assert!(set_decl_field(TEXT, "flag.勇気", "name", &json!(1)).is_err());
        // 読み込みで読み飛ばされる宣言にはしない：型を消す・読めない型、値の無い列挙。
        let invalid = |r: Result<String, EditError>| matches!(r, Err(EditError::Invalid(_)));
        assert!(invalid(set_decl_field(
            TEXT,
            "flag.勇気",
            "type",
            &Value::Null
        )));
        assert!(invalid(set_decl_field(
            TEXT,
            "flag.勇気",
            "type",
            &json!("float")
        )));
        assert!(invalid(set_decl_field(
            TEXT,
            "flag.勇気",
            "type",
            &json!("enum")
        )));
        let en = "flag.a:\n  type: enum\n  values: [甲]\n";
        assert!(invalid(set_decl_field(en, "flag.a", "values", &json!([]))));
        assert!(invalid(set_decl_field(
            en,
            "flag.a",
            "values",
            &Value::Null
        )));
        // 先に値を入れれば、enum にできる。
        let next = set_decl_field(TEXT, "flag.勇気", "values", &json!(["甲", "乙"])).unwrap();
        let next = set_decl_field(&next, "flag.勇気", "type", &json!("enum")).unwrap();
        assert!(next.contains("  type: enum\n"), "{next}");
        // キーを引用符で書いた行でも、行末のコメントを残す。
        let quoted = "flag.a:\n  type: int\n  \"default\": 0  # c\n";
        let next = set_decl_field(quoted, "flag.a", "default", &json!(2)).unwrap();
        assert_eq!(next, "flag.a:\n  type: int\n  default: 2  # c\n");
    }

    #[test]
    fn adds_declarations() {
        let next = add_decl(TEXT, "akari.信頼", "int", &[]).unwrap();
        assert_eq!(next, format!("{TEXT}\nakari.信頼:\n  type: int\n"));
        let next = add_decl("", "ルート", "enum", &["甲".to_owned()]).unwrap();
        assert_eq!(next, "flag.ルート:\n  type: enum\n  values: [甲]\n");
        assert!(add_decl(TEXT, "勇気", "int", &[]).is_err());
        assert!(add_decl(TEXT, "a b", "int", &[]).is_err());
        assert!(add_decl(TEXT, "x", "float", &[]).is_err());
        assert!(add_decl(TEXT, "x", "enum", &[]).is_err());
        let crlf = TEXT.replace('\n', "\r\n");
        let next = add_decl(&crlf, "x", "bool", &[]).unwrap();
        assert!(next.ends_with("\r\n\r\nflag.x:\r\n  type: bool\r\n"));
    }
}
