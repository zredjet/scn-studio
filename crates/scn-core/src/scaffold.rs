//! 作品フォルダのひな形（`scn new`、`scn format-doc`、T1-5、D044）。
//!
//! 新しい作品フォルダに置くファイルは `templates/` にあり、ビルドのときに埋め込む。
//! `FORMAT.md` は、`.scn` の仕様書（1〜16節）と作品フォルダの規約から作る。
//! `SCN.md` は、`scn` コマンドの使い方の説明（`docs/guide/scn.md`。T4-8、D082）から作る。
//! 設定DBの型のテンプレート（`templates/types/`。T4-9、D082）は、`scn new` が作品フォルダに置き（汎用を除く）、
//! アプリの「＋ 型」が型を作る元にする。

use std::fs;
use std::io;
use std::path::Path;

use crate::project::{ProjectError, parse_entry_type, write_source};

/// `.scn` の仕様書。`FORMAT.md` には、1〜16節（IRの形と、決定の経緯の節を除く）を入れる。
const SPEC: &str = include_str!("../../../docs/spec/scn-format-v0.md");
/// 作品フォルダの規約。
const PROJECT_FILES: &str = include_str!("../../../docs/spec/project-files.md");
/// `scn` コマンドの使い方の説明（T4-8、D082）。
const GUIDE: &str = include_str!("../../../docs/guide/scn.md");

/// ツールが書く説明のファイル（`scn new` が置き、`scn format-doc` が今の版で書き直す。手で直さない）。
pub const GENERATED_DOCS: [&str; 2] = ["FORMAT.md", "SCN.md"];

/// 作品フォルダの中に置くファイル（パスと中身）。`{{title}}` は作品名にする。
const TEMPLATES: [(&str, &str); 23] = [
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

/// 設定DBの型のテンプレート（T4-9、D082）。型の名前と項目は `text`（型定義のファイルの中身）に書く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeTemplate {
    /// 型IDの既定（`scn new` が置くファイルの名前）。
    pub id: &'static str,
    /// 何に使う型か（アプリのテンプレートの選択に見せる）。
    pub description: &'static str,
    /// 型定義のファイルの中身。
    pub text: &'static str,
    /// `scn new` が作品フォルダに置くか（置かない汎用の型は、アプリの「＋ 型」からだけ使う）。
    pub placed: bool,
}

/// 設定DBの型のテンプレート集（T4-9、D082）。`scn new` は `placed` の型を `bible/types/<id>.yaml` に置く
/// （要らなければ消してよい）。
pub const TYPE_TEMPLATES: [TypeTemplate; 14] = [
    TypeTemplate {
        id: "character",
        description: "物語に出る人物。台詞の話者になる（組み込みの型）。役割、口調、外見、性格、欲求と葛藤、秘密、声優など",
        text: include_str!("../templates/types/character.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "place",
        description: "舞台になる場所。雰囲気、様子、音や匂い、いる人物、背景の素材、出てくるシーン",
        text: include_str!("../templates/types/place.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "organization",
        description: "学校、部活、会社、一族、国などの集まり。目的、長と主な人、拠点、決まり、成り立ち",
        text: include_str!("../templates/types/organization.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "item",
        description: "物語で意味を持つ物（形見、鍵、手紙、武器など）。はたらき、持ち主、由来、物語での意味",
        text: include_str!("../templates/types/item.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "term",
        description: "作中の用語、固有名詞、造語。読み、意味、使い方、避ける書き方（表記ゆれの検査に使う）",
        text: include_str!("../templates/types/term.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "event",
        description: "過去と本編の出来事（年表）。いつ、並び順、場所、関わった人物と組織、きっかけと結果",
        text: include_str!("../templates/types/event.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "world_rule",
        description: "世界観の決まり（魔法の仕組み、社会の制度など）。できないこと・代償、例外、物語への影響",
        text: include_str!("../templates/types/world_rule.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "relationship",
        description: "2人の人物の関係。関係の種類、互いの見方、始まりと変化、隠していること",
        text: include_str!("../templates/types/relationship.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "foreshadow",
        description: "伏線。仕込みのシーンと回収のシーン（回収の無い伏線は検証が知らせる）、仕込む中身と明かすこと",
        text: include_str!("../templates/types/foreshadow.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "species",
        description: "人間でない種族。見た目、寿命、性質、力と弱み、住むところ、文化",
        text: include_str!("../templates/types/species.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "ability",
        description: "魔法、超能力、技、技能など。効果、代償・条件、限界、使い手、演出の素材",
        text: include_str!("../templates/types/ability.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "culture",
        description: "行事、祭り、風習、信仰、食など。中身、時期、場所、担う人々、物語での意味",
        text: include_str!("../templates/types/culture.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "motif",
        description: "繰り返し現れる象徴（花、雨、歌など）。象徴するもの、現れ方、物語での変化、素材",
        text: include_str!("../templates/types/motif.yaml"),
        placed: true,
    },
    TypeTemplate {
        id: "generic",
        description: "どれにも当てはまらない設定のための汎用の型。概要、詳細、関わるもの、タグ",
        text: include_str!("../templates/types/generic.yaml"),
        placed: false,
    },
];

/// 型のテンプレートを `id` で探す。
pub fn type_template(id: &str) -> Option<&'static TypeTemplate> {
    TYPE_TEMPLATES.iter().find(|t| t.id == id)
}

/// アプリの「＋ 型」に見せる型のテンプレート（名前と項目の名前は、テンプレートの中身を読んだもの）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TypeTemplateRow {
    pub id: &'static str,
    pub name: String,
    pub description: &'static str,
    /// 項目の名前（並びのとおり）。
    pub labels: Vec<String>,
    pub placed: bool,
}

/// 型のテンプレート集の一覧（並びは `TYPE_TEMPLATES` のとおり）。
pub fn type_template_rows() -> Vec<TypeTemplateRow> {
    TYPE_TEMPLATES
        .iter()
        .map(|t| {
            let parsed = parse_entry_type(t.id, t.text).0;
            TypeTemplateRow {
                id: t.id,
                name: parsed
                    .as_ref()
                    .map_or_else(|| t.id.to_owned(), |p| p.name.clone()),
                description: t.description,
                labels: parsed
                    .map(|p| p.fields.into_iter().map(|f| f.label).collect())
                    .unwrap_or_default(),
                placed: t.placed,
            }
        })
        .collect()
}

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
    for template in TYPE_TEMPLATES.iter().filter(|t| t.placed) {
        let path = format!("bible/types/{}.yaml", template.id);
        let full = dir.join(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| error(parent, e))?;
        }
        write_source(dir, &path, template.text)?;
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

    /// 型のテンプレートは、どれも作品の読み込みと同じ読み方で誤りなく読め、型定義のフォームで直せる書き方
    /// （1行の `- { … }`）で書いてある（T4-9）。
    #[test]
    fn type_templates_are_readable_and_editable() {
        use crate::typedef::{TypeEdit, edit_type};
        let mut ids = Vec::new();
        for template in TYPE_TEMPLATES {
            assert!(!ids.contains(&template.id), "{}", template.id);
            ids.push(template.id);
            let (parsed, diags) = parse_entry_type(template.id, template.text);
            assert!(diags.is_empty(), "{}：{diags:?}", template.id);
            let parsed = parsed.unwrap();
            assert!(!parsed.name.is_empty());
            assert!(!parsed.fields.is_empty(), "{}", template.id);
            let mut keys = Vec::new();
            for field in &parsed.fields {
                assert!(!keys.contains(&field.key), "{} {}", template.id, field.key);
                assert!(!matches!(field.key.as_str(), "id" | "name" | "status"));
                assert!(
                    field
                        .key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                    "{}",
                    field.key
                );
                if matches!(field.ty.as_str(), "select" | "multiselect") {
                    assert!(!field.options.is_empty(), "{}", field.key);
                }
                keys.push(field.key.clone());
            }
            assert_eq!(
                parsed.builtin.as_deref(),
                (template.id == "character").then_some("character")
            );
            // どの項目も、フォームで直せる（1行の書き方）。
            for field in &parsed.fields {
                let edit = TypeEdit::Set {
                    key: field.key.clone(),
                    label: format!("{}（直した）", field.label),
                    ty: field.ty.clone(),
                    options: field.options.clone(),
                };
                assert!(
                    edit_type(template.text, &edit).is_ok(),
                    "{} {}",
                    template.id,
                    field.key
                );
            }
        }
        assert_eq!(
            TYPE_TEMPLATES.iter().filter(|t| !t.placed).count(),
            1,
            "置かないのは汎用だけ"
        );
        // コードが読むキャラクターの項目（話者、一人称の表記ゆれ、AI の文脈、企画書の主要キャラ、声優）。
        let character = parse_entry_type("character", TYPE_TEMPLATES[0].text)
            .0
            .unwrap();
        for key in [
            "aliases",
            "first_person",
            "speech",
            "want",
            "conflict",
            "voice",
            "role",
        ] {
            assert!(character.fields.iter().any(|f| f.key == key), "{key}");
        }
        // 伏線のテンプレートは、検証の読む仕込みと回収を持つ（アイデアメモの昇格でも同じ型を置く）。
        let foreshadow = parse_entry_type("foreshadow", type_template("foreshadow").unwrap().text)
            .0
            .unwrap();
        assert_eq!(foreshadow.fields[0].key, "setup");
        assert_eq!(foreshadow.fields[1].key, "payoff");
        let rows = type_template_rows();
        assert_eq!(rows.len(), TYPE_TEMPLATES.len());
        assert_eq!(rows[0].name, "キャラクター");
        assert_eq!(rows[0].labels[0], "役割");
    }

    #[test]
    fn titles_are_one_line() {
        assert_eq!(one_line("放課後の\n灯"), "放課後の 灯");
        assert!(claude_md("a\"b").starts_with("# a\"b（作品フォルダ）"));
    }
}
