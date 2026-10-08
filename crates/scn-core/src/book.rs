//! EPUB と縦書きPDFの書き出し（T3-13、D074）：通読（`route::read`）の本文を、小説としての配布用の EPUB 3 と、
//! 縦書きPDFを組むための印刷用の HTML（外部の組版の道具に渡す中間のファイル）にする。
//!
//! - 道（共通・ルート・エンディング）ごとに1冊（プレーンテキストと同じ道。`scn export text` と同じ選び方）。
//! - 縦書き（既定。`writing-mode: vertical-rl`、右から左へのページ送り）と横書きを選べる（`export/book.yaml` の
//!   `writing`、CLI の `--horizontal`）。
//! - 話者名は書かず、台詞は表示スタイルの括弧で囲む。シーンの見出し、段落の区切り、注記（選んだ選択肢と
//!   エンディング）はプレーンテキストと同じ。ルビは `<ruby>`、傍点は `text-emphasis`、縦書きでは半角の数字の
//!   3文字までと `!!`・`!?`・`?!`・`??` を縦中横にする。
//! - EPUB の ZIP は台本と同じ書き手（`mimetype` を先頭に、圧縮しないで置く）。
//! - 縦書きPDF はコアで組まない（D005：厳密な組版は書き出しで行う）。CSS の印刷の決まり（`@page`）を書いた
//!   HTML を書き出し、外部の組版の道具（Vivliostyle など。T3-14 の外部コマンドでも呼べる）で PDF にする。

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use yaml_serde::Value as Yaml;

use crate::ir::IrInline;
use crate::project::{Project, ProjectError, without_bom, write_bytes};
use crate::renpy::{yaml_string, yaml_text};
use crate::route::{Reading, ReadingItem};
use crate::xlsx::Zip;

/// 出力プロファイルの置き場所（作品フォルダから）。
pub const BOOK_PROFILE: &str = "export/book.yaml";

/// EPUB の書き出し先（作品フォルダから）。
pub const EPUB_DIR: &str = "export/epub";

/// 印刷用の HTML の書き出し先（作品フォルダから）。
pub const PRINT_DIR: &str = "export/pdf";

/// EPUB と印刷用の HTML の出力プロファイル。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BookProfile {
    /// 縦書きか。
    pub vertical: bool,
    /// 著者（無ければ書かない）。
    pub author: Option<String>,
    /// 本文の書体（CSS の `font-family`。無ければ明朝の総称）。
    pub font: Option<String>,
    /// 本文の文字の大きさ（CSS の長さ。印刷用）。
    pub font_size: String,
    /// 紙の大きさ（CSS の `@page` の `size`。印刷用）。
    pub page_size: String,
    /// 紙の余白（CSS の長さ。印刷用）。
    pub page_margin: String,
    /// 注記（選んだ選択肢、エンディング）を書くか。
    pub notes: bool,
}

impl Default for BookProfile {
    fn default() -> Self {
        Self {
            vertical: true,
            author: None,
            font: None,
            font_size: "9pt".to_owned(),
            page_size: "A5".to_owned(),
            page_margin: "15mm".to_owned(),
            notes: true,
        }
    }
}

/// CSS の値として書いてよいか（`;`・`{`・`}`・`<`・`>`・`"`・`\`・注釈の `/*` と改行を含まない）。
fn css_value(text: &str) -> bool {
    !text.is_empty()
        && !text.contains([';', '{', '}', '<', '>', '\n', '\r', '"', '\\'])
        && !text.contains("/*")
}

/// 単位の無い数（`0` を除く）を含むか（`font_size: 9` は CSS では効かない）。
fn unitless(text: &str) -> bool {
    text.split_whitespace()
        .any(|word| word.parse::<f64>().is_ok_and(|n| n != 0.0))
}

/// 出力プロファイルを読む。無ければ既定。値の誤りは読み飛ばして既定を使い、何を読み飛ばしたかを返す。
pub fn read_profile(root: &Path) -> (BookProfile, Vec<String>) {
    let mut profile = BookProfile::default();
    let mut problems = Vec::new();
    let text = match fs::read_to_string(root.join(BOOK_PROFILE)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (profile, problems),
        Err(e) => {
            problems.push(format!("読めません：{e}。既定の設定で書き出します"));
            return (profile, problems);
        }
    };
    let yaml: Yaml = match yaml_serde::from_str(without_bom(&text)) {
        Ok(value) => value,
        Err(e) => {
            problems.push(format!(
                "YAMLとして読めません：{e}。既定の設定で書き出します"
            ));
            return (profile, problems);
        }
    };
    if !yaml.is_mapping() {
        if !yaml.is_null() {
            problems.push("対応表で書きます。既定の設定で書き出します".to_owned());
        }
        return (profile, problems);
    }
    match yaml_string(&yaml, "writing", &mut problems).as_deref() {
        None => {}
        Some("vertical" | "縦書き") => profile.vertical = true,
        Some("horizontal" | "横書き") => profile.vertical = false,
        Some(other) => problems.push(format!(
            "writing は vertical か horizontal で書きます（{other}）"
        )),
    }
    profile.author = yaml_string(&yaml, "author", &mut problems).filter(|a| !a.trim().is_empty());
    for (key, slot) in [
        ("font", None),
        ("font_size", Some(0)),
        ("page_size", Some(1)),
        ("page_margin", Some(2)),
    ] {
        let Some(value) = yaml_string(&yaml, key, &mut problems) else {
            continue;
        };
        if !css_value(&value) {
            problems.push(format!(
                "{key} に使えない文字（; {{ }} < > \" \\ /* と改行）があります（{value}）"
            ));
            continue;
        }
        if matches!(key, "font_size" | "page_margin") && unitless(&value) {
            problems.push(format!(
                "{key} は単位を付けて書きます（9pt、15mm など。{value}）"
            ));
            continue;
        }
        match slot {
            None => profile.font = Some(value),
            Some(0) => profile.font_size = value,
            Some(1) => profile.page_size = value,
            _ => profile.page_margin = value,
        }
    }
    match yaml.get("notes") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::Bool(b)) => profile.notes = *b,
        Some(other) => problems.push(format!(
            "notes は true か false で書きます（{}）",
            yaml_text(other).unwrap_or_default()
        )),
    }
    (profile, problems)
}

/// XML の文字（`&`・`<`・`>`・`"`。XML 1.0 で書けない制御文字は落とす）。
fn xml(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// 縦中横にする並び（半角の数字の3文字までと、`!!`・`!?`・`?!`・`??`）を `<span class="tcy">` で囲む。
fn tate_chu_yoko(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c.is_ascii_digit() {
            let start = i;
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            let run: String = chars.get(start..i).unwrap_or_default().iter().collect();
            // 前後が半角の英字なら、英単語の一部なので囲まない。
            let joined = start
                .checked_sub(1)
                .and_then(|p| chars.get(p))
                .is_some_and(char::is_ascii_alphabetic)
                || chars.get(i).is_some_and(char::is_ascii_alphabetic);
            if run.chars().count() <= 3 && !joined {
                let _ = write!(out, "<span class=\"tcy\">{run}</span>");
            } else {
                out.push_str(&run);
            }
            continue;
        }
        if matches!(c, '!' | '?')
            && chars.get(i + 1).is_some_and(|n| matches!(n, '!' | '?'))
            && !chars.get(i + 2).is_some_and(|n| matches!(n, '!' | '?'))
            && !i
                .checked_sub(1)
                .and_then(|p| chars.get(p))
                .is_some_and(|p| matches!(p, '!' | '?'))
        {
            let next = chars.get(i + 1).copied().unwrap_or(c);
            let _ = write!(out, "<span class=\"tcy\">{c}{next}</span>");
            i += 2;
            continue;
        }
        out.push_str(&xml(&c.to_string()));
        i += 1;
    }
    out
}

/// 本文の文字（縦書きでは縦中横も）。
fn body_text(text: &str, vertical: bool) -> String {
    if vertical {
        tate_chu_yoko(text)
    } else {
        xml(text)
    }
}

/// 文中の記法を XHTML にする。
fn inline_html(text: &[IrInline], vertical: bool) -> String {
    let mut out = String::new();
    for inline in text {
        match inline {
            IrInline::Text { text } => out.push_str(&body_text(text, vertical)),
            IrInline::Ruby { base, reading } => {
                let _ = write!(out, "<ruby>{}<rt>{}</rt></ruby>", xml(base), xml(reading));
            }
            IrInline::Emphasis { text } => {
                let _ = write!(
                    out,
                    "<em class=\"sesame\">{}</em>",
                    body_text(text, vertical)
                );
            }
            IrInline::Interpolation { expr_text, .. } => {
                out.push_str(&xml(&format!("{{={expr_text}}}")));
            }
            IrInline::LineBreak => out.push_str("<br/>"),
            IrInline::Wait { .. }
            | IrInline::PageBreak
            | IrInline::Se { .. }
            | IrInline::Speed { .. } => {}
        }
    }
    out
}

/// 本文（道の項目）を、見出しと段落の XHTML にする。目次の項目（見出しの id と題名）も返す。
fn body_html(
    project: &Project,
    reading: &Reading,
    profile: &BookProfile,
) -> (String, Vec<(String, String)>) {
    let brackets = |style: &str| {
        project
            .settings
            .brackets
            .iter()
            .find(|b| b.style == style)
            .map_or(('「', '」'), |b| (b.open, b.close))
    };
    let vertical = profile.vertical;
    let mut out = String::new();
    let mut toc = Vec::new();
    // 段落の区切りの後の段落に印を付ける（空行の代わりに、前を空ける）。
    let mut gap = false;
    let paragraph = |out: &mut String, class: Option<&str>, html: String, gap: &mut bool| {
        let mut classes: Vec<&str> = Vec::new();
        if let Some(class) = class {
            classes.push(class);
        }
        if std::mem::take(gap) {
            classes.push("gap");
        }
        if classes.is_empty() {
            let _ = writeln!(out, "<p>{html}</p>");
        } else {
            let _ = writeln!(out, "<p class=\"{}\">{html}</p>", classes.join(" "));
        }
    };
    for item in &reading.items {
        match item {
            ReadingItem::Scene { id, title, .. } => {
                let anchor = format!("sec{}", toc.len() + 1);
                let shown = if title.is_empty() { id } else { title };
                let _ = writeln!(
                    out,
                    "<h2 id=\"{anchor}\">{}</h2>",
                    body_text(shown, vertical)
                );
                toc.push((anchor, shown.clone()));
                gap = false;
            }
            ReadingItem::Say { style, text, .. } => {
                let (open, close) = brackets(style);
                let html = format!(
                    "{}{}{}",
                    xml(&open.to_string()),
                    inline_html(text, vertical),
                    xml(&close.to_string())
                );
                paragraph(&mut out, None, html, &mut gap);
            }
            ReadingItem::Narrate { text, .. } => {
                paragraph(&mut out, None, inline_html(text, vertical), &mut gap);
            }
            ReadingItem::Break => gap = true,
            ReadingItem::Choice {
                options, chosen, ..
            } => {
                if profile.notes
                    && let Some(option) = options.get(*chosen)
                {
                    let html = format!("〔選択：{}〕", inline_html(&option.text, vertical));
                    paragraph(&mut out, Some("note"), html, &mut gap);
                }
            }
            ReadingItem::Ending { id } => {
                if profile.notes {
                    gap = true;
                    let html = format!(
                        "〔エンディング：{}〕",
                        body_text(id.as_deref().unwrap_or(""), vertical)
                    );
                    paragraph(&mut out, Some("note"), html, &mut gap);
                }
            }
        }
    }
    (out, toc)
}

/// 本の題名（`作品名（道の名前）`）。
fn book_title(project: &Project, label: &str) -> String {
    match project.settings.title.as_deref().filter(|t| !t.is_empty()) {
        Some(title) => format!("{title}（{label}）"),
        None => label.to_owned(),
    }
}

/// 本文の CSS（EPUB と印刷用で共通の部分）。
fn base_css(profile: &BookProfile) -> String {
    let font = profile
        .font
        .as_deref()
        .map_or_else(|| "serif".to_owned(), |f| format!("{f}, serif"));
    let writing = if profile.vertical {
        "html {\n  writing-mode: vertical-rl;\n  -epub-writing-mode: vertical-rl;\n  -webkit-writing-mode: vertical-rl;\n}\n"
    } else {
        "html {\n  writing-mode: horizontal-tb;\n  -epub-writing-mode: horizontal-tb;\n}\n"
    };
    format!(
        "{writing}body {{\n  font-family: {font};\n  line-height: 1.75;\n}}\n\
         h2 {{\n  font-size: 1.2em;\n  margin-block-end: 1em;\n  break-before: page;\n  page-break-before: always;\n}}\n\
         p {{\n  margin: 0;\n}}\n\
         p.gap {{\n  margin-block-start: 1em;\n}}\n\
         p.note {{\n  font-size: 0.9em;\n}}\n\
         em.sesame {{\n  font-style: normal;\n  text-emphasis-style: sesame;\n  -epub-text-emphasis-style: sesame;\n  -webkit-text-emphasis-style: sesame;\n}}\n\
         span.tcy {{\n  text-combine-upright: all;\n  -epub-text-combine: horizontal;\n  -webkit-text-combine: horizontal;\n}}\n"
    )
}

/// 道の EPUB の中のファイル（パスと中身。`mimetype` が先頭）。`modified` は更新日時（`2026-10-09T00:00:00Z`）。
pub fn epub_files(
    project: &Project,
    reading: &Reading,
    label: &str,
    profile: &BookProfile,
    modified: &str,
) -> Vec<(String, String)> {
    let title = book_title(project, label);
    let (body, toc) = body_html(project, reading, profile);
    let direction = if profile.vertical { "rtl" } else { "ltr" };
    // 作品名と道の名前から決まる識別子（同じ本は同じ識別子）。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let identifier = format!("urn:scn-studio:{hash:016x}");
    let author = profile
        .author
        .as_deref()
        .map(|a| format!("\n    <dc:creator>{}</dc:creator>", xml(a)))
        .unwrap_or_default();
    let writing_meta = if profile.vertical {
        "\n    <meta name=\"primary-writing-mode\" content=\"vertical-rl\"/>"
    } else {
        ""
    };
    let package = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id" xml:lang="ja">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">{identifier}</dc:identifier>
    <dc:title>{}</dc:title>
    <dc:language>ja</dc:language>{author}
    <meta property="dcterms:modified">{}</meta>{writing_meta}
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="style" href="style.css" media-type="text/css"/>
    <item id="text" href="text.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine page-progression-direction="{direction}">
    <itemref idref="text"/>
  </spine>
</package>
"#,
        xml(&title),
        xml(modified)
    );
    let toc_items: String = toc
        .iter()
        .map(|(anchor, shown)| {
            format!(
                "      <li><a href=\"text.xhtml#{anchor}\">{}</a></li>\n",
                xml(shown)
            )
        })
        .collect();
    let toc_list = if toc_items.is_empty() {
        "      <li><a href=\"text.xhtml\">本文</a></li>\n".to_owned()
    } else {
        toc_items
    };
    let nav = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="ja" lang="ja">
<head>
  <meta charset="UTF-8"/>
  <title>{}</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
  <nav epub:type="toc" id="toc">
    <h1>目次</h1>
    <ol>
{toc_list}    </ol>
  </nav>
</body>
</html>
"#,
        xml(&title)
    );
    let text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="ja" lang="ja">
<head>
  <meta charset="UTF-8"/>
  <title>{t}</title>
  <link rel="stylesheet" type="text/css" href="style.css"/>
</head>
<body>
<h1>{t}</h1>
{body}</body>
</html>
"#,
        t = xml(&title)
    );
    let container = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;
    vec![
        ("mimetype".to_owned(), "application/epub+zip".to_owned()),
        ("META-INF/container.xml".to_owned(), container.to_owned()),
        ("OEBPS/package.opf".to_owned(), package),
        ("OEBPS/nav.xhtml".to_owned(), nav),
        (
            "OEBPS/style.css".to_owned(),
            format!("@charset \"UTF-8\";\n{}", base_css(profile)),
        ),
        ("OEBPS/text.xhtml".to_owned(), text),
    ]
}

/// EPUB が大きすぎて ZIP にできない。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("本が大きすぎて EPUB にできません（4GB まで）")]
pub struct TooLarge;

/// 道の EPUB のバイト列（ZIP。`mimetype` を先頭に、圧縮しないで置く）。
pub fn epub(
    project: &Project,
    reading: &Reading,
    label: &str,
    profile: &BookProfile,
    modified: &str,
) -> Result<Vec<u8>, TooLarge> {
    let mut zip = Zip::new();
    for (path, text) in epub_files(project, reading, label, profile, modified) {
        zip.add(&path, text.as_bytes()).map_err(|_| TooLarge)?;
    }
    zip.finish().map_err(|_| TooLarge)
}

/// 縦書きPDFを組むための、印刷用の HTML（1つのファイル。CSS の `@page` に紙の大きさと余白、柱は書かない）。
/// Vivliostyle などの CSS 組版の道具で PDF にする。
pub fn print_html(
    project: &Project,
    reading: &Reading,
    label: &str,
    profile: &BookProfile,
) -> String {
    let title = book_title(project, label);
    let (body, _) = body_html(project, reading, profile);
    let page = format!(
        "@page {{\n  size: {};\n  margin: {};\n}}\nhtml {{\n  font-size: {};\n}}\n",
        profile.page_size, profile.page_margin, profile.font_size
    );
    let author = profile
        .author
        .as_deref()
        .map(|a| format!("<p class=\"author\">{}</p>\n", xml(a)))
        .unwrap_or_default();
    format!(
        "<!DOCTYPE html>\n<html lang=\"ja\">\n<head>\n<meta charset=\"UTF-8\">\n<title>{t}</title>\n<style>\n{page}{}p.author {{\n  margin-block-end: 2em;\n}}\n</style>\n</head>\n<body>\n<h1>{t}</h1>\n{author}{body}</body>\n</html>\n",
        base_css(profile),
        t = xml(&title)
    )
}

/// ファイル名に使えない文字を `_` にする（プレーンテキストと同じ）。
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
        "book".to_owned()
    } else {
        name
    }
}

/// 道の名前から、作品フォルダの中の書き出し先（`export/epub/<道の名前>.epub` など）。
pub fn book_path(root: &Path, dir: &str, label: &str, extension: &str) -> PathBuf {
    root.join(dir)
        .join(format!("{}.{extension}", file_name(label)))
}

/// ファイルを書く（フォルダが無ければ作る。本文と同じく、一時ファイルから置き換える）。
pub fn write_book(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let dir = path.parent().unwrap_or(Path::new(""));
    if !dir.as_os_str().is_empty() {
        fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
            path: dir.to_owned(),
            source,
        })?;
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_bytes(dir, &name, bytes)
}
