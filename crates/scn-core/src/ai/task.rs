//! AIタスクの定義（`ai/*.yaml`。T3-6、D067）：形を読み、誤りは設定ファイルの診断（E802）にして続ける。
//! 接続先の名前（`model`）と `checks` の名前は、使うときに確かめる。

use serde::Serialize;
use yaml_serde::Value;

/// 関わり方の段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskLevel {
    Review,
    Propose,
    Generate,
    Auto,
}

impl TaskLevel {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "review" => Some(Self::Review),
            "propose" => Some(Self::Propose),
            "generate" => Some(Self::Generate),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    /// 本文に書き込む段階（生成と自律）。
    pub fn writes(self) -> bool {
        matches!(self, Self::Generate | Self::Auto)
    }

    /// 段階の既定の出力。
    fn default_output(self) -> TaskOutput {
        match self {
            Self::Review => TaskOutput::Findings,
            Self::Propose => TaskOutput::Diff,
            Self::Generate | Self::Auto => TaskOutput::Draft,
        }
    }
}

/// 出力の形。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskOutput {
    /// 行IDつきの指摘（指摘の段階）。
    Findings,
    /// シーンの要約（指摘の段階。`.cache/` に入れる）。
    Summary,
    /// 行IDつきの差分（提案の段階）。
    Diff,
    /// 提案ノード（スタブシーンの案。提案の段階）。
    Proposals,
    /// 本文への書き込み（生成と自律）。
    Draft,
    /// アイデアメモの分類案（指摘の段階。T3-10）。
    Ideas,
    /// 企画書の下書き（生成の段階。空いている項目にだけ書き、値のある項目は提案。T4-4）。
    Concept,
    /// 設定の下書き（生成の段階。足りないエントリを作り、空いている項目に書く。T4-5）。
    Entries,
    /// アイデアの候補（提案の段階。人が選んだものだけをアイデアメモとして足す。T4-29）。
    #[serde(rename = "idea_candidates")]
    IdeaCandidates,
}

impl TaskOutput {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "findings" => Some(Self::Findings),
            "summary" => Some(Self::Summary),
            "diff" => Some(Self::Diff),
            "proposals" => Some(Self::Proposals),
            "draft" => Some(Self::Draft),
            "ideas" => Some(Self::Ideas),
            "concept" => Some(Self::Concept),
            "entries" => Some(Self::Entries),
            "idea_candidates" => Some(Self::IdeaCandidates),
            _ => None,
        }
    }

    fn fits(self, level: TaskLevel) -> bool {
        match level {
            TaskLevel::Review => matches!(self, Self::Findings | Self::Summary | Self::Ideas),
            TaskLevel::Propose => {
                matches!(self, Self::Diff | Self::Proposals | Self::IdeaCandidates)
            }
            TaskLevel::Generate => matches!(self, Self::Draft | Self::Concept | Self::Entries),
            TaskLevel::Auto => self == Self::Draft,
        }
    }
}

/// 文脈の項目（`context` の1つ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextItem {
    /// 今のシーンの本文（行IDつき）。
    Scene,
    /// 登場キャラのエントリ。`fields` が無ければ全部（項目と本文）、あれば書いた項目だけ（`characters.speech` など）。
    Characters { fields: Option<Vec<String>> },
    /// 言及された用語（キャラクター以外のエントリ）。
    Terms,
    /// 関係する変数の定義。
    Variables,
    /// 前のシーンまでの要約。
    PreviousSummaries,
    /// 企画書（`concept.md`）。
    Concept,
    /// 文体（`style.md`）。
    Style,
    /// ルートごとの要約。
    RouteSummaries,
    /// 章ごとの要約。
    ChapterSummaries,
    /// シーンごとの要約。
    SceneSummaries,
    /// コアが計算した構造の検証の結果（ルートの分量、エンディングへの到達、結果が同じになる選択肢、流れと伏線の
    /// 診断。T3-10）。
    Structure,
    /// アイデアメモ（`ideas/*.md`）の一覧（T3-10）。
    Ideas,
    /// 設定DBの型と項目（T4-5）。
    Types,
    /// 足りない設定と、空いている項目のあるエントリ（T4-5。コアが求める）。
    MissingEntries,
}

impl ContextItem {
    /// 項目の名前（`context` に書く名前）。
    pub fn name(&self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Characters { .. } => "characters",
            Self::Terms => "terms",
            Self::Variables => "variables",
            Self::PreviousSummaries => "previous_summaries",
            Self::Concept => "concept",
            Self::Style => "style",
            Self::RouteSummaries => "route_summaries",
            Self::ChapterSummaries => "chapter_summaries",
            Self::SceneSummaries => "scene_summaries",
            Self::Structure => "structure",
            Self::Ideas => "ideas",
            Self::Types => "types",
            Self::MissingEntries => "missing_entries",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "scene" => Self::Scene,
            "characters" => Self::Characters { fields: None },
            "terms" => Self::Terms,
            "variables" => Self::Variables,
            "previous_summaries" => Self::PreviousSummaries,
            "concept" => Self::Concept,
            "style" => Self::Style,
            "route_summaries" => Self::RouteSummaries,
            "chapter_summaries" => Self::ChapterSummaries,
            "scene_summaries" => Self::SceneSummaries,
            "structure" => Self::Structure,
            "ideas" => Self::Ideas,
            "types" => Self::Types,
            "missing_entries" => Self::MissingEntries,
            _ => return None,
        })
    }
}

/// 書き込んでよい範囲（生成と自律）。
pub const SCOPE_EMPTY_ONLY: &str = "empty_only";

/// AIタスクの定義。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiTask {
    /// ファイル名から拡張子を除いたもの（出自の記録の `task`、`checks` が指す名前）。
    pub id: String,
    /// プロジェクトの根からの相対パス。
    pub path: String,
    /// 表示の名前（`name`。無ければ `id`）。
    pub name: String,
    pub level: TaskLevel,
    /// 接続先の名前（`[ai.<名前>]`）。無ければ既定の接続先。
    pub model: Option<String>,
    /// 文脈の項目（書いた順。同じ項目は1つにまとめる）。
    pub context: Vec<ContextItem>,
    pub output: TaskOutput,
    /// 書き込んでよい範囲（生成と自律だけ。今は `empty_only`）。
    pub scope: Option<String>,
    /// 大きな実行のブランチ名のひな形（`ai/{task}-{date}`）。
    pub branch: Option<String>,
    /// 自律で通す点検（`check` と、ほかのタスクの名前）。
    pub checks: Vec<String>,
    pub max_iterations: Option<u32>,
    /// 1回の実行のトークン数の上限（`budget.tokens`）。
    pub budget_tokens: Option<u64>,
    /// 依頼の文。無ければ実行できない（E802）。
    pub prompt: Option<String>,
}

/// 読んだ定義と、誤り（キーの名前と、知らせる文）。
pub fn parse_task(id: &str, path: &str, root: &Value) -> (AiTask, Vec<(&'static str, String)>) {
    let mut problems: Vec<(&'static str, String)> = Vec::new();
    let text = |key: &'static str, problems: &mut Vec<(&'static str, String)>| match root.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            problems.push((
                key,
                format!("{path} の {key} は文字列で書きます。読み飛ばします"),
            ));
            None
        }
    };
    let name = text("name", &mut problems).unwrap_or_else(|| id.to_owned());
    let level = match text("level", &mut problems) {
        None => TaskLevel::Review,
        Some(level) => TaskLevel::parse(&level).unwrap_or_else(|| {
            problems.push((
                "level",
                format!(
                    "{path} の level「{level}」は review・propose・generate・auto のどれかで書きます。review として扱います"
                ),
            ));
            TaskLevel::Review
        }),
    };
    let model = text("model", &mut problems);
    let output = match text("output", &mut problems) {
        None => level.default_output(),
        Some(output) => match TaskOutput::parse(&output) {
            Some(o) if o.fits(level) => o,
            Some(_) => {
                problems.push((
                    "output",
                    format!(
                        "{path} の output「{output}」は、level に合いません（review は findings・summary、propose は diff・proposals、generate と auto は draft）。既定の形にします"
                    ),
                ));
                level.default_output()
            }
            None => {
                problems.push((
                    "output",
                    format!(
                        "{path} の output「{output}」は findings・summary・diff・proposals・draft のどれかで書きます。既定の形にします"
                    ),
                ));
                level.default_output()
            }
        },
    };
    let context = match root.get("context") {
        None | Some(Value::Null) => vec![ContextItem::Scene],
        Some(Value::Sequence(items)) => context_items(path, items, &mut problems),
        Some(_) => {
            problems.push((
                "context",
                format!("{path} の context は項目の名前のリストで書きます。scene だけを使います"),
            ));
            vec![ContextItem::Scene]
        }
    };
    let scope = match text("scope", &mut problems) {
        Some(scope) if scope == SCOPE_EMPTY_ONLY => Some(scope),
        Some(scope) => {
            problems.push((
                "scope",
                format!(
                    "{path} の scope「{scope}」には対応していません（{SCOPE_EMPTY_ONLY}）。{SCOPE_EMPTY_ONLY} として扱います"
                ),
            ));
            Some(SCOPE_EMPTY_ONLY.to_owned())
        }
        None => None,
    };
    let scope = if level.writes() {
        scope.or_else(|| Some(SCOPE_EMPTY_ONLY.to_owned()))
    } else {
        scope
    };
    let branch = text("branch", &mut problems);
    let checks = match root.get("checks") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Sequence(items)) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => problems.push((
                        "checks",
                        format!("{path} の checks は名前の文字列で書きます。読み飛ばします"),
                    )),
                }
            }
            out
        }
        Some(_) => {
            problems.push((
                "checks",
                format!("{path} の checks は名前のリストで書きます。読み飛ばします"),
            ));
            Vec::new()
        }
    };
    let max_iterations = match root.get("max_iterations") {
        None | Some(Value::Null) => None,
        Some(v) => match v
            .as_u64()
            .filter(|n| *n > 0)
            .and_then(|n| u32::try_from(n).ok())
        {
            Some(n) => Some(n),
            None => {
                problems.push((
                    "max_iterations",
                    format!("{path} の max_iterations は正の整数で書きます。読み飛ばします"),
                ));
                None
            }
        },
    };
    let budget_tokens = match root.get("budget") {
        None | Some(Value::Null) => None,
        Some(Value::Mapping(map)) => match map.get("tokens") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64().filter(|n| *n > 0) {
                Some(n) => Some(n),
                None => {
                    problems.push((
                        "budget",
                        format!("{path} の budget.tokens は正の整数で書きます。読み飛ばします"),
                    ));
                    None
                }
            },
        },
        Some(_) => {
            problems.push((
                "budget",
                format!("{path} の budget は {{ tokens: 数 }} の形で書きます。読み飛ばします"),
            ));
            None
        }
    };
    let prompt = text("prompt", &mut problems).filter(|p| !p.trim().is_empty());
    if prompt.is_none() && !problems.iter().any(|(key, _)| *key == "prompt") {
        problems.push((
            "prompt",
            format!("{path} に prompt がありません。このタスクは実行できません"),
        ));
    }
    (
        AiTask {
            id: id.to_owned(),
            path: path.to_owned(),
            name,
            level,
            model,
            context,
            output,
            scope,
            branch,
            checks,
            max_iterations,
            budget_tokens,
            prompt,
        },
        problems,
    )
}

/// `context` の項目を読む。`characters.<項目>` は1つの `characters` にまとめる（`characters` があれば全部）。
fn context_items(
    path: &str,
    items: &[Value],
    problems: &mut Vec<(&'static str, String)>,
) -> Vec<ContextItem> {
    let mut out: Vec<ContextItem> = Vec::new();
    for item in items {
        let Value::String(name) = item else {
            problems.push((
                "context",
                format!("{path} の context の項目は名前の文字列で書きます。読み飛ばします"),
            ));
            continue;
        };
        let parsed = match name.split_once('.') {
            Some(("characters", field)) if !field.is_empty() => Some(ContextItem::Characters {
                fields: Some(vec![field.to_owned()]),
            }),
            _ => ContextItem::parse(name),
        };
        let Some(parsed) = parsed else {
            problems.push((
                "context",
                format!(
                    "{path} の context の「{name}」は知らない項目です（scene・characters・characters.<項目>・terms・variables・previous_summaries・concept・style・route_summaries・chapter_summaries・scene_summaries）。読み飛ばします"
                ),
            ));
            continue;
        };
        match (&parsed, out.iter_mut().find(|i| i.name() == parsed.name())) {
            (_, None) => out.push(parsed),
            (
                ContextItem::Characters { fields: new },
                Some(ContextItem::Characters { fields: old }),
            ) => match (old.as_mut(), new) {
                (Some(old), Some(new)) => {
                    for field in new {
                        if !old.contains(field) {
                            old.push(field.clone());
                        }
                    }
                }
                // どちらかが全部なら全部。
                _ => *old = None,
            },
            _ => {}
        }
    }
    out
}
