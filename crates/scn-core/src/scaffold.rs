//! 作品フォルダのひな形（`scn new`、`scn format-doc`、T1-5、D044）。
//!
//! 新しい作品フォルダに置くファイルは `templates/` にあり、ビルドのときに埋め込む。
//! `FORMAT.md` は、`.scn` の仕様書（1〜16節）と作品フォルダの規約から作る。
//! `SCN.md` は、`scn` コマンドの使い方の説明（`docs/guide/scn.md`。T4-8、D082）から作る。

use std::fs;
use std::io;
use std::path::Path;

use crate::project::{ProjectError, write_source};

/// `.scn` の仕様書。`FORMAT.md` には、1〜16節（IRの形と、決定の経緯の節を除く）を入れる。
const SPEC: &str = include_str!("../../../docs/spec/scn-format-v0.md");
/// 作品フォルダの規約。
const PROJECT_FILES: &str = include_str!("../../../docs/spec/project-files.md");
/// `scn` コマンドの使い方の説明（T4-8、D082）。
const GUIDE: &str = include_str!("../../../docs/guide/scn.md");

/// ツールが書く説明のファイル（`scn new` が置き、`scn format-doc` が今の版で書き直す。手で直さない）。
pub const GENERATED_DOCS: [&str; 2] = ["FORMAT.md", "SCN.md"];

/// 作品フォルダの中に置くファイル（パスと中身）。`{{title}}` は作品名にする。
const TEMPLATES: [(&str, &str); 24] = [
    ("project.toml", include_str!("../templates/project.toml")),
    ("CLAUDE.md", include_str!("../templates/CLAUDE.md")),
    ("concept.md", include_str!("../templates/concept.md")),
    ("style.md", include_str!("../templates/style.md")),
    (
        "variables.yaml",
        include_str!("../templates/variables.yaml"),
    ),
    (
        "story/chapter01.scn",
        include_str!("../templates/chapter01.scn"),
    ),
    (
        "bible/types/character.yaml",
        include_str!("../templates/character.yaml"),
    ),
    ("ai/provenance.jsonl", ""),
    // 組み込みの AI タスク（T3-6、D067。T3-10 で分岐案・問い返し・メモの分類案、T4-4・T4-5 で企画書と設定の下書きを足した）。
    (
        "ai/branch_ideas.yaml",
        include_str!("../templates/ai/branch_ideas.yaml"),
    ),
    (
        "ai/concept_questions.yaml",
        include_str!("../templates/ai/concept_questions.yaml"),
    ),
    (
        "ai/draft_chapter.yaml",
        include_str!("../templates/ai/draft_chapter.yaml"),
    ),
    (
        "ai/draft_concept.yaml",
        include_str!("../templates/ai/draft_concept.yaml"),
    ),
    (
        "ai/draft_entries.yaml",
        include_str!("../templates/ai/draft_entries.yaml"),
    ),
    (
        "ai/draft_stub.yaml",
        include_str!("../templates/ai/draft_stub.yaml"),
    ),
    (
        "ai/idea_sort.yaml",
        include_str!("../templates/ai/idea_sort.yaml"),
    ),
    (
        "ai/rephrase.yaml",
        include_str!("../templates/ai/rephrase.yaml"),
    ),
    (
        "ai/route_skeleton.yaml",
        include_str!("../templates/ai/route_skeleton.yaml"),
    ),
    (
        "ai/scene_summary.yaml",
        include_str!("../templates/ai/scene_summary.yaml"),
    ),
    (
        "ai/setting_check.yaml",
        include_str!("../templates/ai/setting_check.yaml"),
    ),
    (
        "ai/thin_branches.yaml",
        include_str!("../templates/ai/thin_branches.yaml"),
    ),
    (
        "ai/tone_check.yaml",
        include_str!("../templates/ai/tone_check.yaml"),
    ),
    (
        "ai/wording_check.yaml",
        include_str!("../templates/ai/wording_check.yaml"),
    ),
    (".gitignore", include_str!("../templates/gitignore")),
    (".gitattributes", include_str!("../templates/gitattributes")),
];

/// 企画書のひな形（アプリの「企画書を作る」。`scn new` と同じ中身）。
pub const CONCEPT_TEMPLATE: &str = include_str!("../templates/concept.md");

/// 中身の無いまま作るフォルダ。
const EMPTY_DIRS: [&str; 1] = ["bible/entries/character"];

/// 共通の `CLAUDE.md` のひな形を、作品名で埋めたもの。
pub fn claude_md(title: &str) -> String {
    include_str!("../templates/CLAUDE.md").replace("{{title}}", &one_line(title))
}

/// `FORMAT.md` の中身。
pub fn format_doc() -> String {
    let spec_body = SPEC
        .find("\n## 1. ")
        .map(|start| {
            let rest = SPEC.get(start + 1..).unwrap_or_default();
            match rest.find("\n## 17. ") {
                Some(end) => rest.get(..end).unwrap_or(rest),
                None => rest,
            }
        })
        .unwrap_or(SPEC)
        .trim_end();
    format!(
        "# 形式と作品フォルダの規約（FORMAT.md）\n\n\
         このファイルは scn {version} が書いた。手で直さず、ツールの版が上がったら `scn format-doc` で書き直す。\n\
         中身は、scn-studio の `docs/spec/scn-format-v0.md`（`.scn` の形式。1〜16節）と \
         `docs/spec/project-files.md`（作品フォルダの規約）。文中の `Dxxx` は scn-studio の決定記録の番号、\
         `docs/…` は scn-studio のリポジトリの文書を指す。\n\n\
         # 第1部 .scn の形式\n\n{spec_body}\n\n---\n\n{project_files}",
        version = env!("CARGO_PKG_VERSION"),
        project_files = PROJECT_FILES.trim_start(),
    )
}

/// `SCN.md` の中身：説明の初めの見出しを、作品フォルダのファイルの見出しと、書いた版の知らせにする。
pub fn scn_doc() -> String {
    let body = GUIDE
        .split_once('\n')
        .map_or(GUIDE, |(_, rest)| rest)
        .trim_start();
    format!(
        "# scn コマンドの使い方（SCN.md）\n\n\
         このファイルは scn {version} が書いた。手で直さず、ツールの版が上がったら `scn format-doc` で書き直す。\n\
         中身は、scn-studio の `docs/guide/scn.md`。\n\n{body}",
        version = env!("CARGO_PKG_VERSION"),
    )
}

/// ツールが書く説明のファイルの、今の版の中身（`GENERATED_DOCS` の名前で）。
fn generated_doc(name: &str) -> Option<String> {
    match name {
        "FORMAT.md" => Some(format_doc()),
        "SCN.md" => Some(scn_doc()),
        _ => None,
    }
}

/// 作品名を1行にする（改行や制御文字を空白にする）。
fn one_line(title: &str) -> String {
    title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// 新しい作品フォルダを作る（D044）。`dir` が無ければ作る。あっても空なら使う。
/// 空でなければ、何も書かずにエラーにする。
pub fn create_project(dir: &Path, title: &str) -> Result<(), ProjectError> {
    let error = |path: &Path, source: io::Error| ProjectError::Write {
        path: path.to_owned(),
        source,
    };
    match fs::read_dir(dir) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                return Err(ProjectError::NotEmpty {
                    path: dir.to_owned(),
                });
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(dir).map_err(|e| error(dir, e))?;
        }
        Err(e) => return Err(error(dir, e)),
    }
    let title = one_line(title);
    let toml_title = toml::Value::String(title.clone()).to_string();
    for (path, template) in TEMPLATES {
        let text = match path {
            "project.toml" => template.replace("{{title}}", &toml_title),
            _ => template.replace("{{title}}", &title),
        };
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| error(parent, e))?;
        }
        write_source(dir, path, &text)?;
    }
    for path in EMPTY_DIRS {
        let full = dir.join(path);
        fs::create_dir_all(&full).map_err(|e| error(&full, e))?;
    }
    for name in GENERATED_DOCS {
        if let Some(text) = generated_doc(name) {
            write_source(dir, name, &text)?;
        }
    }
    Ok(())
}

/// `FORMAT.md` と `SCN.md` を、今のツールの版で書き直す。書き直したファイルの名前を返す（同じなら書かない）。
pub fn write_generated_docs(dir: &Path) -> Result<Vec<&'static str>, ProjectError> {
    let mut written = Vec::new();
    for name in stale_docs(dir) {
        if let Some(text) = generated_doc(name) {
            write_source(dir, name, &text)?;
            written.push(name);
        }
    }
    Ok(written)
}

/// `FORMAT.md`・`SCN.md` のうち、無いか今のツールの版と違うもの。改行の違い（Gitが `\r\n` にして取り出した
/// ときなど）は見ない。
pub fn stale_docs(dir: &Path) -> Vec<&'static str> {
    GENERATED_DOCS
        .into_iter()
        .filter(|name| {
            fs::read_to_string(dir.join(name)).map_or(true, |text| {
                Some(text.replace("\r\n", "\n")) != generated_doc(name)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_doc_has_sections_1_to_16_only() {
        let doc = format_doc();
        assert!(doc.contains("\n## 1. 基本方針"));
        assert!(doc.contains("\n## 16. 診断の一覧"));
        assert!(!doc.contains("\n## 17. "));
        assert!(!doc.contains("\n## 18. "));
        assert!(doc.contains("# 作品フォルダの規約"));
        assert!(doc.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn scn_doc_has_the_guide() {
        let doc = scn_doc();
        assert!(doc.starts_with("# scn コマンドの使い方（SCN.md）\n"));
        assert!(doc.contains(env!("CARGO_PKG_VERSION")));
        assert!(doc.contains("\n## 1. 入れ方"));
        assert!(doc.contains("\n## 12. つまずいたとき"));
        // 説明の初めの見出しは、作品フォルダのファイルの見出しに替える。
        assert!(!doc.contains("\n# scn コマンドの使い方\n"));
    }

    #[test]
    fn titles_are_one_line() {
        assert_eq!(one_line("放課後の\n灯"), "放課後の 灯");
        assert!(claude_md("a\"b").starts_with("# a\"b（作品フォルダ）"));
    }
}
