//! 文脈の組み立て（T3-6、D067）：タスクの定義の `context` から、LLM に渡す指示（system）と依頼（user）を作る。
//!
//! - 節は「上から下へ」の順に並べる：企画書、文体、ルート・章・シーンの要約、前のシーンまでの要約、キャラクター、
//!   用語、変数、今のシーン。
//! - 文脈の長さ（接続先の `context_tokens` から、返す分を除いた上限）に入らなければ、決めた順に削る
//!   （[`TRIM_ORDER`]）。今のシーンと依頼は削らない。生成と自律では文体（文体見本）を削らない。それでも入らなければ
//!   実行せずに知らせる（`problem`）。
//! - シーンの要約は `.cache/summaries.json` にシーンの本文のハッシュと一緒に置き、本文が変わったら使わない
//!   （メタ情報の `summary`、無ければ題名にする）。

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::client::estimate_tokens;
use super::task::{AiTask, ContextItem, TaskOutput};
use crate::assist::scene_info;
use crate::index::build_index;
use crate::ir::Ir;
use crate::project::{Entry, Project, VarType};

/// シーンの要約のキャッシュ（作品フォルダの中。`.gitignore` で除く）。
pub const SUMMARY_CACHE_FILE: &str = ".cache/summaries.json";

/// シーンの要約のキャッシュ。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryCache {
    #[serde(default)]
    pub scenes: BTreeMap<String, CachedSummary>,
}

/// キャッシュの1つのシーン。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSummary {
    /// 要約を作ったときのシーンの本文のハッシュ（[`source_hash`]）。
    pub hash: String,
    pub summary: String,
}

/// 文脈に使う、本文以外のファイル。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextSources {
    /// `concept.md` の中身。
    pub concept: Option<String>,
    /// `style.md` の中身。
    pub style: Option<String>,
    pub summaries: SummaryCache,
    /// 読めなかったもの（知らせる文）。
    pub problems: Vec<String>,
}

fn read_optional(root: &Path, file: &str, problems: &mut Vec<String>) -> Option<String> {
    match std::fs::read(root.join(file)) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Some(text.trim_start_matches('\u{feff}').to_owned()),
            Err(_) => {
                problems.push(format!(
                    "{file} はUTF-8で書かれていません。文脈に入れません"
                ));
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            problems.push(format!("{file} を読めません：{e}"));
            None
        }
    }
}

/// 文脈に使うファイルを読む。
pub fn read_sources(root: &Path) -> ContextSources {
    let mut problems = Vec::new();
    let concept = read_optional(root, "concept.md", &mut problems);
    let style = read_optional(root, "style.md", &mut problems);
    let summaries = match read_optional(root, SUMMARY_CACHE_FILE, &mut problems) {
        None => SummaryCache::default(),
        Some(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            problems.push(format!(
                "{SUMMARY_CACHE_FILE} を読めません（{e}）。要約のキャッシュを使いません"
            ));
            SummaryCache::default()
        }),
    };
    ContextSources {
        concept,
        style,
        summaries,
        problems,
    }
}

/// シーンの本文のハッシュ（FNV-1a 64 ビットの16進）。
pub fn source_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// シーンの本文（見出しの行から、同じファイルの次のシーンの見出しの手前まで。行IDつき）。
pub fn scene_source<'a>(project: &'a Project, ir: &Ir, scene: &str) -> Option<&'a str> {
    let this = ir.scenes.iter().find(|s| s.id == scene)?;
    let file = project.files.iter().find(|f| f.path == this.file)?;
    let end = ir
        .scenes
        .iter()
        .filter(|s| s.file == this.file && s.span.start > this.span.start)
        .map(|s| s.span.start)
        .min()
        .unwrap_or(file.text.len());
    file.text.get(this.span.start..end)
}

/// 要約をキャッシュに入れて書く（シーン要約のタスクが使う）。
pub fn store_summary(
    root: &Path,
    cache: &mut SummaryCache,
    scene: &str,
    source: &str,
    summary: &str,
) -> Result<(), String> {
    cache.scenes.insert(
        scene.to_owned(),
        CachedSummary {
            hash: source_hash(source),
            summary: summary.trim().to_owned(),
        },
    );
    let text = serde_json::to_string_pretty(cache).map_err(|e| e.to_string())?;
    let path = root.join(SUMMARY_CACHE_FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}：{e}", dir.display()))?;
    }
    std::fs::write(&path, format!("{text}\n")).map_err(|e| format!("{}：{e}", path.display()))
}

/// 要約の出どころ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SummarySource {
    /// 要約のキャッシュ（AI のシーン要約）。
    Cache,
    /// メタ情報の `summary`。
    Meta,
    /// 要約が無く、題名だけ。
    Title,
}

/// シーンの要約。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SceneSummary {
    pub scene: String,
    pub title: String,
    pub file: String,
    pub route: Option<String>,
    /// 要約（無ければ空）。
    pub summary: String,
    pub source: SummarySource,
    /// キャッシュの要約が、本文が変わって使えない（作り直す）。
    pub stale: bool,
}

/// すべてのシーンの要約（IR の順）。
pub fn scene_summaries(project: &Project, ir: &Ir, cache: &SummaryCache) -> Vec<SceneSummary> {
    ir.scenes
        .iter()
        .map(|s| {
            let cached = cache.scenes.get(&s.id);
            let hash = scene_source(project, ir, &s.id).map(source_hash);
            let fresh = cached.filter(|c| Some(&c.hash) == hash.as_ref());
            let (summary, source) = match (fresh, &s.meta.summary) {
                (Some(c), _) => (c.summary.clone(), SummarySource::Cache),
                (None, Some(meta)) => (meta.clone(), SummarySource::Meta),
                (None, None) => (String::new(), SummarySource::Title),
            };
            SceneSummary {
                scene: s.id.clone(),
                title: s.title.clone(),
                file: s.file.clone(),
                route: s.meta.route.clone(),
                summary,
                source,
                stale: cached.is_some() && fresh.is_none(),
            }
        })
        .collect()
}

fn summary_line(s: &SceneSummary) -> String {
    if s.summary.is_empty() {
        format!("- {}「{}」", s.scene, s.title)
    } else {
        format!("- {}「{}」：{}", s.scene, s.title, s.summary)
    }
}

/// 文脈の1つの節。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    /// 項目の名前（`scene`、`characters` など）。
    pub item: &'static str,
    pub title: String,
    pub text: String,
    /// 見積もったトークン数。
    pub tokens: u64,
}

/// 削った節。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trimmed {
    pub item: &'static str,
    pub title: String,
    /// 削った分のトークン数（見積もり）。
    pub tokens: u64,
    /// `removed`（節ごと）か `shortened`（キャラクターを一人称と口調メモだけにした）。
    pub how: &'static str,
}

/// 組み立てた文脈。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assembled {
    pub task: String,
    pub scene: Option<String>,
    /// 接続先の文脈の長さ。
    pub context_tokens: u64,
    /// 文脈に使える上限（返す分を除いたもの。[`context_limit`]）。
    pub limit: u64,
    /// 残した節（並べた順）。
    pub sections: Vec<Section>,
    /// 削った節（削った順）。
    pub trimmed: Vec<Trimmed>,
    /// 知らせ（無い企画書、作り直す要約など）。
    pub notes: Vec<String>,
    pub system: String,
    pub user: String,
    /// `system` と `user` の見積もったトークン数。
    pub tokens: u64,
    /// 実行できない理由。
    pub problem: Option<String>,
}

/// 文脈に使える上限：文脈の長さから、返す分（4分の1、多くて4096）を除く。
pub fn context_limit(context_tokens: u64) -> u64 {
    context_tokens.saturating_sub((context_tokens / 4).min(4096))
}

/// 削る順番。`characters` は1度目で一人称と口調メモだけにし、2度目で節ごと削る。`style` は指摘と提案のタスクだけ。
pub const TRIM_ORDER: [&str; 11] = [
    "previous_summaries",
    "scene_summaries",
    "chapter_summaries",
    "route_summaries",
    "terms",
    "characters",
    "characters",
    "variables",
    "concept",
    "style",
    // 今のシーンは削らない（印として置く）。
    "scene",
];

/// 節を並べる順（上から下へ）。
const SECTION_ORDER: [&str; 10] = [
    "concept",
    "style",
    "route_summaries",
    "chapter_summaries",
    "scene_summaries",
    "previous_summaries",
    "characters",
    "terms",
    "variables",
    "scene",
];

const PREAMBLE: &str = "あなたは、分岐する物語（ADV・ビジュアルノベル）の執筆を手伝う共作者です。
文脈に書かれた設定と本文だけを根拠にし、書かれていないことを事実として扱わないでください。
人が書いた行を直接書き換えず、変更は提案として出してください。

本文は .scn 形式です。`## 題名 #シーンID` がシーンの見出し、`---` で囲んだ部分がシーンのメタ情報、
`話者「台詞」` が台詞、`@` で始まる行が演出や分岐の命令、`-> シーンID` がジャンプ、
それ以外の行が地の文です。行末の `^xxxx` は行IDで、変えてはいけません。";

fn output_instruction(output: TaskOutput) -> &'static str {
    match output {
        TaskOutput::Findings => {
            "指摘は1行に1つの JSON で返してください：{\"line\": \"行ID（^ を除く）\", \"severity\": \"error か warning か info\", \"message\": \"指摘\"}。
行に結びつかない指摘は、line の代わりに \"scene\": \"シーンID\" を書きます。指摘が無ければ何も返さないでください。"
        }
        TaskOutput::Summary => "要約の文だけを返してください。",
        TaskOutput::Diff => {
            "案は1行に1つの JSON で返してください：{\"line\": \"行ID（^ を除く）\", \"before\": \"今の文\", \"after\": \"案\", \"reason\": \"理由\"}。
行IDのある行だけを対象にし、行IDは変えないでください。"
        }
        TaskOutput::Proposals => {
            "提案ごとに `### 提案 1` のような見出しを置き、その下に、足すスタブシーン（見出し、メタ情報の summary、選択肢とジャンプ）を .scn の形で書いてください。"
        }
        TaskOutput::Draft => {
            "書いた本文を .scn の形で返してください。今ある行（行IDのある行）は変えずにそのまま残し、新しい行には行IDを付けないでください。"
        }
    }
}

/// エントリの項目の値を、文脈に入れる文字列にする。空なら `None`。
fn value_text(value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(s) => s.trim().to_owned(),
        Value::Array(items) => items
            .iter()
            .filter_map(value_text)
            .collect::<Vec<_>>()
            .join("、"),
        other => other.to_string(),
    };
    (!text.is_empty()).then_some(text)
}

/// 参照（`ref<…>`）の項目の値を、参照先のエントリの名前にする（`北風高校（kitakaze_high）`）。参照先が無ければそのまま。
fn reference_text(project: &Project, value: &Value) -> Option<String> {
    let name = |id: &str| {
        project
            .bible
            .entries
            .iter()
            .find(|e| e.id == id)
            .map_or_else(|| id.to_owned(), |e| format!("{}（{id}）", entry_name(e)))
    };
    let text = match value {
        Value::String(id) => name(id.trim()),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(|id| name(id.trim()))
            .collect::<Vec<_>>()
            .join("、"),
        other => return value_text(other),
    };
    (!text.is_empty()).then_some(text)
}

/// 項目の型が参照（`ref<…>`・`list<ref<…>>`）か。
fn is_reference(project: &Project, entry: &Entry, key: &str) -> bool {
    project
        .bible
        .types
        .iter()
        .find(|t| t.id == entry.type_id)
        .and_then(|t| t.fields.iter().find(|d| d.key == key))
        .is_some_and(|d| d.ty.starts_with("ref<") || d.ty.starts_with("list<ref<"))
}

/// エントリの項目（型の定義の順）と、そのラベル。`name` は見出しに使うので除く。
fn entry_fields<'a>(project: &'a Project, entry: &'a Entry) -> Vec<(String, &'a Value)> {
    let defs = project
        .bible
        .types
        .iter()
        .find(|t| t.id == entry.type_id)
        .map(|t| t.fields.as_slice())
        .unwrap_or_default();
    let mut out: Vec<(String, &Value)> = defs
        .iter()
        .filter(|d| d.key != "name" && d.key != "id")
        .filter_map(|d| entry.fields.get(&d.key).map(|v| (d.key.clone(), v)))
        .collect();
    // 型に無いキー（`variants` など）も、名前の順で後ろに足す。
    for (key, value) in &entry.fields {
        if key != "name" && key != "id" && !defs.iter().any(|d| &d.key == key) {
            out.push((key.clone(), value));
        }
    }
    out
}

fn field_label(project: &Project, entry: &Entry, key: &str) -> String {
    project
        .bible
        .types
        .iter()
        .find(|t| t.id == entry.type_id)
        .and_then(|t| t.fields.iter().find(|d| d.key == key))
        .map_or_else(
            || match key {
                "first_person" => "一人称".to_owned(),
                "speech" => "口調メモ".to_owned(),
                "body" => "本文".to_owned(),
                _ => key.to_owned(),
            },
            |d| d.label.clone(),
        )
}

fn entry_name(entry: &Entry) -> String {
    entry
        .fields
        .get("name")
        .and_then(Value::as_str)
        .map_or_else(|| entry.id.clone(), str::to_owned)
}

/// エントリの渡し方。
#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryShape<'a> {
    /// 項目と本文を全部。
    Full,
    /// 一人称と口調メモだけ（削ったとき）。書いた項目があれば、そのうちの一人称と口調メモだけ。
    Brief(Option<&'a [String]>),
    /// 書いた項目だけ（`characters.speech` など。`body` は本文）。
    Fields(&'a [String]),
}

/// エントリの渡す部分（ラベルと中身）。
fn entry_parts(project: &Project, entry: &Entry, shape: &EntryShape<'_>) -> Vec<(String, String)> {
    let wanted = |key: &str| match shape {
        EntryShape::Full => true,
        EntryShape::Brief(fields) => {
            (key == "first_person" || key == "speech")
                && fields.is_none_or(|keys| keys.iter().any(|k| k == key))
        }
        EntryShape::Fields(keys) => keys.iter().any(|k| k == key),
    };
    let mut parts: Vec<(String, String)> = entry_fields(project, entry)
        .into_iter()
        .filter(|(key, _)| wanted(key))
        .filter_map(|(key, value)| {
            let text = if is_reference(project, entry, &key) {
                reference_text(project, value)
            } else {
                value_text(value)
            };
            text.map(|t| (field_label(project, entry, &key), t))
        })
        .collect();
    let body = entry.body.trim();
    if !body.is_empty() && (matches!(shape, EntryShape::Full) || wanted("body")) {
        parts.push(("本文".to_owned(), body.to_owned()));
    }
    parts
}

fn render_entry(heading: &str, parts: &[(String, String)]) -> String {
    let mut out = format!("### {heading}");
    for (label, text) in parts {
        if label == "本文" {
            out.push_str(&format!("\n{text}"));
        } else {
            out.push_str(&format!("\n- {label}：{text}"));
        }
    }
    out
}

/// 変数の宣言を1行にする。
fn variable_line(project: &Project, key: &str) -> String {
    let Some(decl) = project.variables.iter().find(|v| v.key == key) else {
        return format!("- {key}（宣言なし）");
    };
    let mut facts = vec![decl.ty.name().to_owned()];
    if let VarType::Enum(values) = &decl.ty {
        facts.push(format!("値 {}", values.join("・")));
    }
    if let Some(default) = &decl.default {
        facts.push(format!("既定 {default}"));
    }
    if let Some((min, max)) = decl.range {
        facts.push(format!("範囲 {min}〜{max}"));
    }
    if let Some(scope) = &decl.scope {
        facts.push(scope.clone());
    }
    let desc = decl
        .desc
        .as_deref()
        .map(|d| format!("：{d}"))
        .unwrap_or_default();
    format!("- {key}（{}）{desc}", facts.join("、"))
}

/// 開始シーンから `scene` までの、シーンの辺の上の最短の道（`scene` を含まない）。着けなければ `None`。
fn previous_scenes(ir: &Ir, scene: &str) -> Option<Vec<String>> {
    let start = ir
        .start_scene
        .as_deref()
        .filter(|s| ir.scenes.iter().any(|x| x.id == *s))
        .or_else(|| ir.scenes.first().map(|s| s.id.as_str()))?;
    let mut came_from: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(here) = queue.pop_front() {
        if here == scene {
            let mut path = Vec::new();
            let mut at = here;
            while let Some(prev) = came_from.get(at) {
                path.push((*prev).to_owned());
                at = prev;
            }
            path.reverse();
            return Some(path);
        }
        for edge in ir.edges.iter().filter(|e| e.from == here) {
            if seen.insert(edge.to.as_str()) {
                came_from.insert(edge.to.as_str(), here);
                queue.push_back(edge.to.as_str());
            }
        }
    }
    None
}

/// 文字列の中に、語のどれかがあるか。
fn mentions(text: &str, words: &[String]) -> bool {
    words
        .iter()
        .any(|w| !w.is_empty() && text.contains(w.as_str()))
}

fn entry_words(entry: &Entry) -> Vec<String> {
    let mut words = vec![entry_name(entry)];
    if let Some(Value::Array(aliases)) = entry.fields.get("aliases") {
        words.extend(aliases.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    words
}

/// 節を作る材料。
struct Builder<'a> {
    project: &'a Project,
    ir: &'a Ir,
    sources: &'a ContextSources,
    task: &'a AiTask,
    scene: Option<&'a str>,
    notes: Vec<String>,
}

impl Builder<'_> {
    fn characters_fields(&self) -> Option<Option<&[String]>> {
        self.task.context.iter().find_map(|i| match i {
            ContextItem::Characters { fields } => Some(fields.as_deref()),
            _ => None,
        })
    }

    /// 文脈に入れる項目か。生成と自律では、`context` に書いていなくても文体を入れる（文体見本は必ず入れる）。
    fn has(&self, name: &str) -> bool {
        self.task.context.iter().any(|i| i.name() == name)
            || (name == "style" && self.task.level.writes())
    }

    /// 登場キャラ（シーンがあれば登場順、無ければ全員）。話者も `@立ち絵` も無いシーン（スタブなど）では、
    /// シーンの本文（メタ情報の summary を含む）に名前か呼び名が出るキャラクターを、出た順に。
    fn character_entries(&self) -> Vec<&Entry> {
        match self.scene {
            Some(scene) => {
                let index = build_index(self.project, self.ir);
                let appearing: Vec<&Entry> = scene_info(self.project, &index, scene)
                    .characters
                    .iter()
                    .filter_map(|c| self.project.character_entry(&c.id))
                    .collect();
                if !appearing.is_empty() {
                    return appearing;
                }
                let text = scene_source(self.project, self.ir, scene).unwrap_or_default();
                let mut mentioned: Vec<(usize, &Entry)> = self
                    .project
                    .characters
                    .iter()
                    .filter_map(|c| {
                        let first = std::iter::once(&c.name)
                            .chain(&c.aliases)
                            .filter(|w| !w.is_empty())
                            .filter_map(|w| text.find(w.as_str()))
                            .min()?;
                        Some((first, self.project.character_entry(&c.id)?))
                    })
                    .collect();
                mentioned.sort_by_key(|(at, _)| *at);
                mentioned.into_iter().map(|(_, e)| e).collect()
            }
            None => self
                .project
                .characters
                .iter()
                .filter_map(|c| self.project.character_entry(&c.id))
                .collect(),
        }
    }

    fn characters(&self, brief: bool) -> Option<Section> {
        let fields = self.characters_fields()?;
        let shape = match (brief, fields) {
            (true, keys) => EntryShape::Brief(keys),
            (false, None) => EntryShape::Full,
            (false, Some(keys)) => EntryShape::Fields(keys),
        };
        let blocks: Vec<String> = self
            .character_entries()
            .into_iter()
            .map(|e| {
                render_entry(
                    &format!("{}（{}）", entry_name(e), e.id),
                    &entry_parts(self.project, e, &shape),
                )
            })
            .collect();
        if blocks.is_empty() {
            return None;
        }
        Some(section(
            "characters",
            "キャラクター",
            format!("## キャラクター\n\n{}", blocks.join("\n\n")),
        ))
    }

    fn summaries(&self) -> Vec<SceneSummary> {
        scene_summaries(self.project, self.ir, &self.sources.summaries)
    }

    fn build(&mut self, name: &str) -> Option<Section> {
        match name {
            "concept" => {
                let text = self.sources.concept.as_deref().map(str::trim);
                if text.is_none() {
                    self.notes
                        .push("企画書（concept.md）がありません".to_owned());
                }
                Some(section(
                    "concept",
                    "企画書",
                    format!("## 企画書（concept.md）\n\n{}", text?),
                ))
            }
            "style" => {
                let text = self.sources.style.as_deref().map(str::trim);
                if text.is_none() {
                    self.notes.push("文体（style.md）がありません".to_owned());
                }
                Some(section(
                    "style",
                    "文体",
                    format!("## 文体（style.md）\n\n{}", text?),
                ))
            }
            "route_summaries" | "chapter_summaries" => {
                let by_route = name == "route_summaries";
                let summaries = self.summaries();
                let mut groups: Vec<(String, Vec<String>)> = Vec::new();
                for s in &summaries {
                    let key = if by_route {
                        s.route
                            .clone()
                            .unwrap_or_else(|| "（ルートなし）".to_owned())
                    } else {
                        s.file.clone()
                    };
                    match groups.iter_mut().find(|(k, _)| *k == key) {
                        Some((_, lines)) => lines.push(summary_line(s)),
                        None => groups.push((key, vec![summary_line(s)])),
                    }
                }
                if groups.is_empty() {
                    return None;
                }
                let (item, title) = if by_route {
                    ("route_summaries", "ルートの要約")
                } else {
                    ("chapter_summaries", "章の要約")
                };
                let body: Vec<String> = groups
                    .iter()
                    .map(|(k, lines)| format!("### {k}\n{}", lines.join("\n")))
                    .collect();
                Some(section(
                    item,
                    title,
                    format!("## {title}\n\n{}", body.join("\n\n")),
                ))
            }
            "scene_summaries" => {
                let lines: Vec<String> = self.summaries().iter().map(summary_line).collect();
                if lines.is_empty() {
                    return None;
                }
                Some(section(
                    "scene_summaries",
                    "シーンの要約",
                    format!("## シーンの要約\n\n{}", lines.join("\n")),
                ))
            }
            "previous_summaries" => {
                let Some(scene) = self.scene else {
                    self.notes.push(
                        "シーンを選んでいないので、前のシーンまでの要約を入れません".to_owned(),
                    );
                    return None;
                };
                let Some(path) = previous_scenes(self.ir, scene) else {
                    self.notes.push(format!(
                        "開始シーンから {scene} への道が見つからないので、前のシーンまでの要約を入れません"
                    ));
                    return None;
                };
                if path.is_empty() {
                    return None;
                }
                let summaries = self.summaries();
                let lines: Vec<String> = path
                    .iter()
                    .filter_map(|id| summaries.iter().find(|s| &s.scene == id))
                    .map(summary_line)
                    .collect();
                Some(section(
                    "previous_summaries",
                    "前のシーンまでの要約",
                    format!(
                        "## 前のシーンまでの要約（開始シーンからの道）\n\n{}",
                        lines.join("\n")
                    ),
                ))
            }
            "characters" => self.characters(false),
            "terms" => {
                let character_type = self.project.bible.character_type();
                let source = self
                    .scene
                    .and_then(|s| scene_source(self.project, self.ir, s));
                let blocks: Vec<String> = self
                    .project
                    .bible
                    .entries
                    .iter()
                    .filter(|e| e.type_id != character_type)
                    .filter(|e| source.is_none_or(|text| mentions(text, &entry_words(e))))
                    .map(|e| {
                        let type_name = self
                            .project
                            .bible
                            .types
                            .iter()
                            .find(|t| t.id == e.type_id)
                            .map_or(e.type_id.as_str(), |t| t.name.as_str());
                        render_entry(
                            &format!("{}（{type_name}）", entry_name(e)),
                            &entry_parts(self.project, e, &EntryShape::Full),
                        )
                    })
                    .collect();
                if blocks.is_empty() {
                    return None;
                }
                Some(section(
                    "terms",
                    "用語",
                    format!("## 用語\n\n{}", blocks.join("\n\n")),
                ))
            }
            "variables" => {
                let keys: Vec<String> = match self.scene {
                    Some(scene) => {
                        let index = build_index(self.project, self.ir);
                        scene_info(self.project, &index, scene)
                            .variables
                            .into_iter()
                            .map(|v| v.key)
                            .collect()
                    }
                    None => self
                        .project
                        .variables
                        .iter()
                        .map(|v| v.key.clone())
                        .collect(),
                };
                if keys.is_empty() {
                    return None;
                }
                let lines: Vec<String> = keys
                    .iter()
                    .map(|k| variable_line(self.project, k))
                    .collect();
                Some(section(
                    "variables",
                    "変数",
                    format!("## 変数\n\n{}", lines.join("\n")),
                ))
            }
            "scene" => {
                let scene = self.scene?;
                let this = self.ir.scenes.iter().find(|s| s.id == scene)?;
                let text = scene_source(self.project, self.ir, scene)?;
                Some(section(
                    "scene",
                    "今のシーン",
                    format!(
                        "## シーン {}「{}」（{}）\n\n```scn\n{}\n```",
                        this.id,
                        this.title,
                        this.file,
                        text.trim_end()
                    ),
                ))
            }
            _ => None,
        }
    }
}

fn section(item: &'static str, title: &str, text: String) -> Section {
    Section {
        item,
        title: title.to_owned(),
        tokens: estimate_tokens(&text),
        text,
    }
}

fn user_text(sections: &[Section], prompt: &str, target: Option<&str>) -> String {
    let mut out = String::from("# 文脈\n\n");
    let bodies: Vec<&str> = sections.iter().map(|s| s.text.as_str()).collect();
    out.push_str(&bodies.join("\n\n"));
    out.push_str("\n\n# 依頼\n\n");
    out.push_str(prompt.trim_end());
    if let Some(target) = target {
        out.push_str(&format!("\n\n対象：{target}"));
    }
    out
}

/// タスクの文脈を組み立てる。`scene` は今のシーン（`context` に `scene`・`previous_summaries` があるタスクで使う）。
/// `context_tokens` は接続先の文脈の長さ。
pub fn assemble(
    project: &Project,
    ir: &Ir,
    sources: &ContextSources,
    task: &AiTask,
    scene: Option<&str>,
    context_tokens: u64,
) -> Assembled {
    let limit = context_limit(context_tokens);
    let mut out = Assembled {
        task: task.id.clone(),
        scene: scene.map(str::to_owned),
        context_tokens,
        limit,
        sections: Vec::new(),
        trimmed: Vec::new(),
        notes: sources.problems.clone(),
        system: String::new(),
        user: String::new(),
        tokens: 0,
        problem: None,
    };
    let Some(prompt) = task.prompt.as_deref() else {
        out.problem = Some(format!("{} に prompt がありません", task.path));
        return out;
    };
    let this = match scene {
        None => None,
        Some(id) => match ir.scenes.iter().find(|s| s.id == id) {
            Some(s) => Some(s),
            None => {
                out.problem = Some(format!("シーン {id} がありません"));
                return out;
            }
        },
    };
    if this.is_none() && task.context.contains(&ContextItem::Scene) {
        out.problem = Some(format!(
            "{} は今のシーンを文脈に入れるタスクです。シーンを選びます",
            task.name
        ));
        return out;
    }
    let mut builder = Builder {
        project,
        ir,
        sources,
        task,
        scene,
        notes: Vec::new(),
    };
    let mut sections: Vec<Section> = Vec::new();
    for name in SECTION_ORDER {
        if builder.has(name)
            && let Some(built) = builder.build(name)
        {
            sections.push(built);
        }
    }
    out.notes.append(&mut builder.notes);
    let stale: Vec<String> = scene_summaries(project, ir, &sources.summaries)
        .into_iter()
        .filter(|s| s.stale)
        .map(|s| s.scene)
        .collect();
    let uses_summaries = [
        "previous_summaries",
        "scene_summaries",
        "chapter_summaries",
        "route_summaries",
    ]
    .iter()
    .any(|n| builder.has(n));
    if uses_summaries && !stale.is_empty() {
        out.notes.push(format!(
            "本文が変わって要約を作り直すシーン：{}（メタ情報の summary を使いました）",
            stale.join("、")
        ));
    }
    let target = this.map(|s| format!("シーン {}「{}」", s.id, s.title));
    let system = format!("{PREAMBLE}\n\n{}", output_instruction(task.output));
    let measure = |sections: &[Section]| {
        estimate_tokens(&system) + estimate_tokens(&user_text(sections, prompt, target.as_deref()))
    };
    let mut shortened = false;
    for step in TRIM_ORDER {
        if measure(&sections) <= limit {
            break;
        }
        if step == "scene" || (step == "style" && task.level.writes()) {
            continue;
        }
        let Some(at) = sections.iter().position(|s| s.item == step) else {
            continue;
        };
        let Some(old) = sections.get(at).cloned() else {
            continue;
        };
        if step == "characters" && !shortened {
            shortened = true;
            if let Some(brief) = builder.characters(true).filter(|b| b.tokens < old.tokens) {
                out.trimmed.push(Trimmed {
                    item: old.item,
                    title: old.title.clone(),
                    tokens: old.tokens.saturating_sub(brief.tokens),
                    how: "shortened",
                });
                if let Some(slot) = sections.get_mut(at) {
                    *slot = brief;
                }
                continue;
            }
        }
        sections.remove(at);
        out.trimmed.push(Trimmed {
            item: old.item,
            title: old.title,
            tokens: old.tokens,
            how: "removed",
        });
    }
    out.tokens = measure(&sections);
    if task.level.writes() && !sections.iter().any(|s| s.item == "style") {
        out.problem = Some(
            "文体（style.md）がありません。生成と自律のタスクには、文体の方針と文体見本が要ります"
                .to_owned(),
        );
    } else if out.tokens > limit {
        let style_kept = task.level.writes() && sections.iter().any(|s| s.item == "style");
        out.problem = Some(if style_kept {
            format!(
                "文脈が上限（{limit}）に入りきりません（見積もり {}）。生成のタスクでは文体（文体見本）を削りません。接続先の context_tokens を増やすか、style.md を短くします",
                out.tokens
            )
        } else {
            format!(
                "文脈が上限（{limit}）に入りきりません（見積もり {}）。接続先の context_tokens を増やすか、短いシーンで使います",
                out.tokens
            )
        });
    }
    out.user = user_text(&sections, prompt, target.as_deref());
    out.system = system;
    out.sections = sections;
    out
}

/// 設定DBのエントリが渡る部分（項目のラベルと字数）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryPart {
    pub label: String,
    pub chars: usize,
}

/// エントリを文脈に入れるタスク。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryTask {
    pub id: String,
    pub name: String,
    /// 渡す部分のラベル。
    pub labels: Vec<String>,
    /// 渡す字数。
    pub chars: usize,
}

/// 設定DBの画面の「AIに渡す内容」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryContext {
    /// キャラクターとして渡る（登場するシーンで）。いいえなら用語として（言及されたシーンで）。
    pub character: bool,
    /// 全部を渡すときの部分。
    pub parts: Vec<EntryPart>,
    /// このエントリを文脈に入れるタスク。
    pub tasks: Vec<EntryTask>,
}

fn parts_of(parts: &[(String, String)]) -> Vec<EntryPart> {
    parts
        .iter()
        .map(|(label, text)| EntryPart {
            label: label.clone(),
            chars: text.chars().count(),
        })
        .collect()
}

/// エントリ（`path` はプロジェクトの根からの相対パス）が、どのタスクにどの部分で渡るか。
pub fn entry_context(project: &Project, path: &str) -> Option<EntryContext> {
    let entry = project.bible.entries.iter().find(|e| e.path == path)?;
    let character = entry.type_id == project.bible.character_type();
    let full = entry_parts(project, entry, &EntryShape::Full);
    let tasks = project
        .ai_tasks
        .iter()
        .filter_map(|task| {
            let parts = if character {
                let fields = task.context.iter().find_map(|i| match i {
                    ContextItem::Characters { fields } => Some(fields.as_deref()),
                    _ => None,
                })?;
                match fields {
                    None => full.clone(),
                    Some(keys) => entry_parts(project, entry, &EntryShape::Fields(keys)),
                }
            } else {
                task.context
                    .contains(&ContextItem::Terms)
                    .then(|| full.clone())?
            };
            Some(EntryTask {
                id: task.id.clone(),
                name: task.name.clone(),
                labels: parts.iter().map(|(l, _)| l.clone()).collect(),
                chars: parts.iter().map(|(_, t)| t.chars().count()).sum(),
            })
        })
        .collect();
    Some(EntryContext {
        character,
        parts: parts_of(&full),
        tasks,
    })
}
