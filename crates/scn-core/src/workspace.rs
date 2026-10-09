//! 開いている作品フォルダ（アプリ、T1-6、D045）：保存していない本文と、ディスクの中身を持つ。
//!
//! アプリはこれを通して本文を保存し、ファイルの監視で知った外部の変更を取り込む。外部の変更は、
//! 保存していない変更が無ければ読み込み直し、あれば「読み込み直す」か「自分の変更を残す」かを選ばせる
//! （マージはフェーズ2）。自分で書いたファイルの知らせは、中身がディスクの中身と同じなので無視する。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::assets::{CATALOG_FILE, CatalogEditError, add_entry, assets_relative, set_file};
use crate::bible::{FORESHADOW_TYPE, body_of, front_fields, set_body, set_field, yaml_scalar};
use crate::concept::CONCEPT_FILE;
use crate::flowedit::{FlowEdge, insert_edge};
use crate::ideas::{
    ADOPTED, FORESHADOW_TYPE_TEXT, IDEAS_DIR, Promotion, is_idea_path, new_idea_file,
    new_idea_text, stub_scene, summary_line,
};
use crate::layout::{LAYOUT_FILE, LayoutError, Point, rename_layout_key, update_layout};
use crate::line_id::{AssignedLineId, LineIdGenerator, assign_line_ids};
use crate::merge::{Choice, Merge, merge_scn, merge_text};
use crate::project::{
    Project, ProjectError, SourceFile, load_project, scn_paths, without_bom, write_source,
};
use crate::provenance::{
    AI, PROVENANCE_FILE, Record, append_provenance, edited_ai_fields, edited_ai_lines,
    is_field_file, moved_field_records, now as provenance_now, read_provenance,
    reset_field_records,
};
use crate::rename::{RenameTarget, plan_rename};
use crate::scaffold::CONCEPT_TEMPLATE;
use crate::typedef::{TypeEdit, edit_type};
use crate::vardecl::{add_decl, set_decl_field};

/// 開いている作品フォルダ。
pub struct Workspace {
    /// 開いたときに渡されたパス。
    root: PathBuf,
    /// 実際のパス（シンボリックリンクを解いたもの）。ファイルの監視の知らせのパスと照らす。
    canonical: PathBuf,
    /// 本文のファイルは、保存していない編集を含む今の中身を持つ。
    project: Project,
    /// 本文のファイルの、ディスクの中身（読み込んだか、保存したか、取り込みを決めたときのもの）。
    /// `None` はディスクに無い（外で消された）。
    disk: BTreeMap<String, Option<String>>,
    /// 保存していない変更があるのに外で変わったファイルの、外の中身（まだ取り込みを決めていない）。
    pending: BTreeMap<String, Option<String>>,
    /// 外で消されて閉じたファイル。アプリにまだ送っていない編集があれば、「自分の変更を残す」で開き直す。
    removed: BTreeSet<String>,
    /// 最後に自分で書いた `layout.json` の中身。監視の知らせで、ディスクの中身がこれと同じなら無視する。
    layout_written: Option<String>,
    /// 自分で書いた設定ファイル（設定DBのエントリ、`variables.yaml`）の中身。書いたときに設定を読み直して
    /// いるので、監視の知らせで、ディスクの中身がこれと同じなら無視する。
    settings_written: BTreeMap<String, String>,
    /// 名前の変更で、自分で移したエントリの元のパス（監視の知らせを無視する。T2-8）。
    moved_away: BTreeSet<String>,
    /// 本文のファイルの、人が直し始めた元の中身（読んだか、書いたか、読み込み直したときの中身）。外の変更と
    /// ぶつかって「自分の変更を残す」・マージをしても変えない。出自の記録で、人が直した AI の行を見分ける
    /// （T3-4、D065）。
    base: BTreeMap<String, String>,
    /// 企画書・エントリの項目の出自を書けなかったわけ（書いた後に呼ぶ側が受け取る。T4-3）。
    field_provenance_problem: Option<String>,
}

/// 名前の変更の結果（T2-8、D059）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Renamed {
    /// 書いた本文のファイル（アプリはエディタの中身を差し替える）。
    pub story: Vec<StoryWritten>,
    /// 書いた設定ファイル（ファイル名を変えたものは新しいパス）。
    pub settings: Vec<String>,
    /// 本文の置き換えの数。
    pub story_edits: usize,
    /// 知らせ（変えないもの、変わってしまうもの）。
    pub notes: Vec<String>,
    /// 途中で書けなかったとき、その理由（それまでに書いたものは `story` と `settings` にある）。
    pub problem: Option<String>,
    /// 保存していない変更ごと書いた章で、人が直した AI の行として `ai-edited` を書き足した行ID（T3-4）。
    pub provenance: Vec<String>,
}

/// マージを当てた結果（T2-7、D058）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeApplied {
    /// マージした本文（保存していない変更として開く）。
    pub text: String,
    /// ディスクの中身（外の中身）。
    pub disk: Option<String>,
}

/// 外部の変更を取り込んだ結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// 何もしない（自分で書いた変更、同じ中身、本文でも設定でもないファイル）。
    None,
    /// 保存していない変更が無いので、読み込み直した。
    Reloaded { file: String, text: String },
    /// 本文のファイルが外で足された。
    Added { file: String, text: String },
    /// 本文のファイルが外で消された（保存していない変更は無かった）。
    Removed { file: String },
    /// 保存していない変更があるのに、外で変わった（`deleted` なら消された）。選ばせる。
    Conflict { file: String, deleted: bool },
    /// 本文のファイルが外で変わったが、読めない（UTF-8でない、書いている途中など）。取り込まずに知らせる。
    Unreadable { file: String },
    /// フロー図の配置（`layout.json`）が変わった（`git pull` など）。アプリは読み直す（D048）。
    Layout,
    /// 設定ファイル（`project.toml`、変数、素材カタログ、設定DB、企画書）が変わったので、設定を読み直した。
    Settings,
    /// アイデアメモ（`ideas/*.md`）が外で変わった。アプリはメモの一覧を読み直す（T2-6）。
    Ideas,
    /// 出自の記録（`ai/provenance.jsonl`）が変わった（外部のエージェントが書き足した、`git pull` など）。
    /// アプリは AI の行の印を取り直す（T3-4）。
    Provenance,
    /// 外部のAIエージェントの提案（`ai/proposals/*.md`）が変わった。アプリは提案の一覧を読み直す（T3-8）。
    Proposals,
}

/// 書いた本文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// 書いた本文（付けた行IDを含む）。
    pub text: String,
    /// 保存のときに付けた行ID（D045）。
    pub assigned: Vec<AssignedLineId>,
    /// 人が直した AI の行として、出自を `ai-edited` に書き足した行ID（T3-4、D065）。
    pub provenance: Vec<String>,
    /// 出自の記録を読めなかった・書けなかったわけ（保存は済んでいる）。
    pub provenance_problem: Option<String>,
}

/// 提案を採用した結果（T3-8、D069）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adopted {
    /// 保存した（採用した行が1つ以上あった）。
    pub saved: Option<Saved>,
    /// まだ取り込んでいない外部の変更があったので、採用しなかった（`true` なら外で消された）。
    pub conflict: Option<bool>,
    /// 置き換えた行ID。
    pub applied: Vec<String>,
    /// 採用しなかった行IDと理由。
    pub skipped: Vec<(String, String)>,
}

/// 下書きを書き込んだ結果（T3-9、D070）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftWritten {
    /// 保存した。
    pub saved: Option<Saved>,
    /// まだ取り込んでいない外部の変更があったので、書き込まなかった（`true` なら外で消された）。
    pub conflict: Option<bool>,
    /// 書き込まなかったわけ（スタブでない、差し込む行が無い、流れが変わる）。
    pub refused: Option<String>,
    /// 差し込んだ行に付けた行ID。
    pub ids: Vec<String>,
}

/// 提案ノードの採用とルートの骨格を書いた結果（T3-10、D071）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotWritten {
    /// 書いた章（採用で章が決まらなければ空）。
    pub file: String,
    pub written: DraftWritten,
    /// 書いた後の検証（足したシーンと、選択肢を足したシーンの誤りと警告）。
    pub checks: Vec<String>,
}

/// 保存の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(Saved),
    /// まだ取り込んでいない外部の変更がある（監視の知らせが届く前、または選ぶ前）ので、書かなかった
    /// （`deleted` なら外で消された）。外の中身を退避し、「読み込み直す」か「自分の変更を残す」かを選ばせる。
    Conflict {
        deleted: bool,
    },
}

/// 設定DBと `variables.yaml` を書いた結果（T1-10、D049）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryEdit {
    /// 書いた。`text` は書いた中身。
    Saved { text: String },
    /// 外で変わっていた（直した項目や本文も外で変わっていた）ので、書かなかった。`text` は今のディスクの中身。
    Conflict { text: String },
    /// フォームでは書き換えられない書き方なので、書かなかった（ソース表示で直す）。
    Unsupported { message: String },
}

/// アイデアメモを昇格した結果（T2-6、D057）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Promoted {
    /// 昇格先を作り、メモを書いた。`text` は書いたメモ、`target` は `promoted_to` の値。スタブのシーンを
    /// 足したら、その章の書いた中身（`scene`）。
    Saved {
        text: String,
        target: String,
        scene: Option<StoryWritten>,
    },
    /// 昇格先は作ったが、メモを書けなかった（書く直前に読み取り専用になった、ディスクがいっぱいなど）。
    /// アプリは作ったもの（`scene`）を画面に入れ、`message` を知らせる。
    MemoUnwritten {
        target: String,
        scene: Option<StoryWritten>,
        message: String,
    },
    /// メモが外で変わっていたので、何も作らなかった。`text` は今のメモの中身。
    Conflict { text: String },
    /// メモのフロントマターを書き換えられない書き方なので、何も作らなかった（ソースで直す）。
    Unsupported { message: String },
}

/// 書いた本文のファイル（アプリはエディタの中身を差し替える）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoryWritten {
    pub file: String,
    pub text: String,
}

/// 素材カタログを書いた結果（T2-3、D054）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatalogEdit {
    /// 書いた。`text` は書いた中身。
    Saved { text: String },
    /// 変わらない（もうある、同じファイル）ので、書かなかった。
    Unchanged,
    /// 画面からは書き換えられない書き方なので、書かなかった（テキストで直す）。
    Unsupported { message: String },
}

/// 素材を指すもの（種類、立ち絵のキャラクターID、意味名）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AssetKey {
    /// `background`・`expression`・`bgm`・`se`。
    pub kind: String,
    pub character: Option<String>,
    pub name: String,
}

/// 変数の宣言のファイル。
const VARIABLES_FILE: &str = "variables.yaml";

/// `text` の改行を、`current`（今のファイル）の改行にそろえる。画面の入力欄は `\n` で返すため。
fn same_newlines(current: &str, text: &str) -> String {
    let crlf = current
        .find('\n')
        .is_some_and(|at| at > 0 && current.as_bytes().get(at - 1) == Some(&b'\r'));
    if crlf {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_owned()
    }
}

/// 章のファイル名（`story/` から）：前後の空白を除き、`.scn` が無ければ補う。`/` で区切ったどの部分も空でなく、
/// `.` で始まらず、`\` を含まない。合わなければ `None`。
fn chapter_name(name: &str) -> Option<String> {
    let name = name.trim();
    let name = if name.ends_with(".scn") {
        name.to_owned()
    } else {
        format!("{name}.scn")
    };
    let ok = name
        .trim_end_matches(".scn")
        .split('/')
        .all(|part| !part.is_empty() && !part.starts_with('.') && !part.contains('\\'))
        && !name.trim_end_matches(".scn").is_empty();
    ok.then_some(name)
}

/// 設定ファイル（変われば設定を読み直す）。`bible/` の中はすべて。
const SETTINGS_FILES: [&str; 4] = [
    "project.toml",
    "variables.yaml",
    "assets/catalog.yaml",
    "concept.md",
];

fn error(path: &Path, kind: io::ErrorKind, message: &str) -> ProjectError {
    ProjectError::Write {
        path: path.to_owned(),
        source: io::Error::new(kind, message.to_owned()),
    }
}

fn not_open(path: &Path) -> ProjectError {
    error(path, io::ErrorKind::NotFound, "開いていないファイルです")
}

/// ディスクの本文のファイルを読んだ結果。
enum Disk {
    Text(String),
    /// 無い。
    Missing,
    /// あるが読めない（UTF-8でない、読む権限が無い、書いている途中など）。消されたとは扱わない。
    Unreadable,
}

fn read_disk(path: &Path) -> Disk {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_or(Disk::Unreadable, Disk::Text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Disk::Missing,
        Err(_) => Disk::Unreadable,
    }
}

impl Workspace {
    /// 作品フォルダを開く。
    pub fn open(root: &Path) -> Result<Self, ProjectError> {
        let project = load_project(root)?;
        let disk = project
            .files
            .iter()
            .map(|f| (f.path.clone(), Some(f.text.clone())))
            .collect();
        let base = project
            .files
            .iter()
            .map(|f| (f.path.clone(), f.text.clone()))
            .collect();
        Ok(Self {
            base,
            root: root.to_owned(),
            canonical: fs::canonicalize(root).unwrap_or_else(|_| root.to_owned()),
            project,
            disk,
            pending: BTreeMap::new(),
            removed: BTreeSet::new(),
            layout_written: None,
            settings_written: BTreeMap::new(),
            moved_away: BTreeSet::new(),
            field_provenance_problem: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 今の中身（保存していない編集を含む）。
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// 本文のファイルの今の中身。
    pub fn text(&self, file: &str) -> Option<&str> {
        self.file(file).map(|f| f.text.as_str())
    }

    fn file(&self, file: &str) -> Option<&SourceFile> {
        self.project.files.iter().find(|f| f.path == file)
    }

    /// 保存していない編集を受け取る。知らないファイルなら `false`。
    pub fn set_buffer(&mut self, file: &str, text: &str) -> bool {
        match self.project.files.iter_mut().find(|f| f.path == file) {
            Some(existing) => {
                text.clone_into(&mut existing.text);
                true
            }
            None => false,
        }
    }

    /// 知っているディスクの中身（外で消されたか、開いていなければ `None`）。
    pub fn disk_text(&self, file: &str) -> Option<&str> {
        self.disk.get(file).and_then(|d| d.as_deref())
    }

    /// 保存していない変更があるか（今の中身がディスクの中身と違うか）。
    pub fn is_dirty(&self, file: &str) -> bool {
        match (self.text(file), self.disk.get(file)) {
            (Some(text), Some(Some(disk))) => text != disk,
            (Some(_), _) => true,
            (None, _) => false,
        }
    }

    /// 本文のファイルを保存する。行IDの無い行に行IDを付けてから書く（D045）。本文の読み方に関わる設定を
    /// 読めないとき（[`Project::has_reading_errors`]）は付けない。足した ` ^ID` のほかは、今の中身のまま書く。
    ///
    /// ディスクの中身が、知っている中身と違えば（まだ取り込んでいない外部の変更があれば）、上に書かずに
    /// 衝突を返す。選ぶ前の外の中身（`pending`）が残っているときも同じ。「自分の変更を残す」の後なら書ける。
    /// `seen` は、呼び手（アプリの画面）が知っているディスクの中身（消されていれば `None`）。コアが取り込んだ
    /// 外部の変更を、呼び手がまだ受け取っていなければ（知らせの途中）、それとも違うので衝突を返す。
    pub fn save(
        &mut self,
        file: &str,
        seen: Option<&str>,
        generator: &mut LineIdGenerator,
    ) -> Result<SaveOutcome, ProjectError> {
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        if let Some(known) = self.disk.get(file)
            && known.as_deref() != seen
        {
            return Ok(SaveOutcome::Conflict {
                deleted: known.is_none(),
            });
        }
        if let Some(on_disk) = self.pending.get(file) {
            return Ok(SaveOutcome::Conflict {
                deleted: on_disk.is_none(),
            });
        }
        let on_disk = match read_disk(&path) {
            Disk::Text(text) => Some(text),
            Disk::Missing => None,
            Disk::Unreadable => {
                return Err(error(
                    &path,
                    io::ErrorKind::InvalidData,
                    "ディスクの中身がUTF-8で読めない中身に変わっているので、保存していません",
                ));
            }
        };
        if self.disk.get(file) != Some(&on_disk) {
            let deleted = on_disk.is_none();
            self.pending.insert(file.to_owned(), on_disk);
            return Ok(SaveOutcome::Conflict { deleted });
        }
        let (text, assigned) = if self.project.has_reading_errors() {
            (current, Vec::new())
        } else {
            match assign_line_ids(&self.project, generator)
                .into_iter()
                .find(|r| r.path == file)
            {
                Some(result) => (result.text, result.assigned),
                None => (current, Vec::new()),
            }
        };
        write_source(&self.root, file, &text)?;
        self.set_buffer(file, &text);
        // アプリで直して保存した AI の行は、人が直した（`ai-edited`）と記録する（T3-4、D065）。比べるのは、
        // 人が直し始めた元の中身と、外の中身（自分の変更を残したとき）。
        let before = self.base.get(file).cloned().or_else(|| on_disk.clone());
        let (provenance, provenance_problem) = match &before {
            Some(before) => self.record_edited_ai_lines(before, on_disk.as_deref(), &text),
            None => (Vec::new(), None),
        };
        self.synced(file, &text);
        Ok(SaveOutcome::Saved(Saved {
            text,
            assigned,
            provenance,
            provenance_problem,
        }))
    }

    /// 人が直し始めた元を覚え直す（アプリが覚えていた、知らせの途中の競合の元。コアは読み込み直した後の
    /// 中身しか知らないので、そのまま比べると、外で変わった行を人が直した行と数えてしまう。T3-4）。
    pub fn remember_base(&mut self, file: &str, base: &str) {
        if self.file(file).is_some() {
            self.base.insert(file.to_owned(), base.to_owned());
        }
    }

    /// ディスクと本文がそろった（読んだ、書いた、読み込み直した）。人が直し始める元も、その中身にする。
    fn synced(&mut self, file: &str, text: &str) {
        self.disk.insert(file.to_owned(), Some(text.to_owned()));
        self.base.insert(file.to_owned(), text.to_owned());
    }

    /// 人が直した AI の行（今の出自が `ai`。行をつないだときは残った行）を、`ai-edited` として出自の記録に
    /// 書き足す。`before` は人が直し始めた元、`external` は今のディスクの中身、`after` は書く本文。
    fn record_edited_ai_lines(
        &self,
        before: &str,
        external: Option<&str>,
        after: &str,
    ) -> (Vec<String>, Option<String>) {
        let provenance = match read_provenance(&self.root) {
            Ok(p) => p,
            Err(e) => return (Vec::new(), Some(e.to_string())),
        };
        if provenance.records.is_empty() {
            return (Vec::new(), None);
        }
        let records = edited_ai_lines(
            before,
            external,
            after,
            &self.project.parse_options(),
            &provenance,
            &provenance_now(),
        );
        let ids: Vec<String> = records.iter().map(|r| r.line.clone()).collect();
        match append_provenance(&self.root, &records) {
            Ok(()) => (ids, None),
            Err(e) => (
                Vec::new(),
                Some(format!("{e}（記録できなかった行ID：{}）", ids.join("、"))),
            ),
        }
    }

    /// 作品フォルダの根からの相対パス（区切りは `/`）。作品フォルダの外なら `None`。
    pub fn relative(&self, path: &Path) -> Option<String> {
        // 消されたファイルは解けないので、親のフォルダを解いてから名前をつなぐ。
        let resolved = path
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name));
        let rel = [Some(path.to_owned()), resolved]
            .into_iter()
            .flatten()
            .find_map(|p| {
                p.strip_prefix(&self.canonical)
                    .or_else(|_| p.strip_prefix(&self.root))
                    .ok()
                    .map(Path::to_path_buf)
            })?;
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        Some(parts.join("/"))
    }

    /// 外部の変更（ファイルとフォルダの監視の知らせ）を取り込む。`story/` の下のフォルダの知らせ（Finder で
    /// フォルダごと動かした、消した、持ち込んだなど。中のファイルの知らせが来ないことがある）では、そのフォルダの
    /// 下の、知っている章とディスクの章をすべて調べる（T2-10、D061）。
    pub fn external_changes(&mut self, path: &Path) -> Vec<Change> {
        if let Some(rel) = self.relative(path) {
            let parts: Vec<&str> = rel.split('/').collect();
            let folder = parts.first() == Some(&"story")
                && !rel.ends_with(".scn")
                && !parts.iter().any(|p| p.starts_with('.'))
                && !path.is_file();
            if folder {
                let prefix = format!("{rel}/");
                let mut files: BTreeSet<String> = self
                    .project
                    .files
                    .iter()
                    .map(|f| f.path.clone())
                    .filter(|p| p.starts_with(&prefix))
                    .collect();
                if path.is_dir() {
                    let (found, _) = scn_paths(path);
                    files.extend(found.iter().filter_map(|p| self.relative(p)));
                }
                return files
                    .iter()
                    .map(|file| self.story_change(file))
                    .filter(|change| *change != Change::None)
                    .collect();
            }
        }
        match self.external_change(path) {
            Change::None => Vec::new(),
            change => vec![change],
        }
    }

    /// 提案を採用する（T3-8、D069）：今の本文に当て（採用できる行だけを、字下げと行IDを残して置き換える）、保存し、
    /// 置き換えた行の出自を `ai`（タスクとモデル）として書き足す。人の行を直接書き換えないために、採用は人の操作
    /// （アプリのボタン）からだけ呼ぶ。外部の変更とぶつかっていれば、本文を戻して採用しない。
    pub fn adopt_proposals(
        &mut self,
        file: &str,
        proposals: &[crate::ai::Proposal],
        task: &str,
        model: &str,
        seen: Option<&str>,
        generator: &mut LineIdGenerator,
    ) -> Result<Adopted, ProjectError> {
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        // 本文全体で見て採用できない行ID（ほかの章との行IDの重なり、複数行の台詞）。
        let ids: Vec<&str> = proposals.iter().map(|p| p.line.as_str()).collect();
        let blocked =
            crate::ai::project_blocks(&self.project, &crate::analyze(&self.project), &ids);
        let options = self.project.parse_options();
        let applied = crate::ai::apply_proposals(&current, proposals, &blocked, &options);
        if applied.applied.is_empty() {
            return Ok(Adopted {
                saved: None,
                conflict: None,
                applied: Vec::new(),
                skipped: applied.skipped,
            });
        }
        self.set_buffer(file, &applied.text);
        // 書けなかったときも、採用した本文をワークスペースに残さない（エディタは元のまま）。
        let outcome = match self.save(file, seen, generator) {
            Ok(outcome) => outcome,
            Err(e) => {
                self.set_buffer(file, &current);
                return Err(e);
            }
        };
        match outcome {
            SaveOutcome::Conflict { deleted } => {
                self.set_buffer(file, &current);
                Ok(Adopted {
                    saved: None,
                    conflict: Some(deleted),
                    applied: Vec::new(),
                    skipped: applied.skipped,
                })
            }
            SaveOutcome::Saved(mut saved) => {
                let at = provenance_now();
                let records: Vec<Record> = applied
                    .applied
                    .iter()
                    .map(|line| Record {
                        line: line.clone(),
                        origin: AI.to_owned(),
                        task: task.to_owned(),
                        model: model.to_owned(),
                        at: at.clone(),
                        ..Record::default()
                    })
                    .collect();
                if let Err(e) = append_provenance(&self.root, &records) {
                    let ids: Vec<String> =
                        applied.applied.iter().map(|id| format!("^{id}")).collect();
                    saved.provenance_problem = Some(format!(
                        "{e}（採用した行 {} を記録できませんでした）",
                        ids.join("、")
                    ));
                }
                Ok(Adopted {
                    saved: Some(saved),
                    conflict: None,
                    applied: applied.applied,
                    skipped: applied.skipped,
                })
            }
        }
    }

    /// 生成の下書きをスタブのシーンに書き込む（T3-9、D070）：本文の行を差し込み（今ある行は変えない）、メタ情報の
    /// `status` を `ai-draft` にし、差し込んだ行にだけ行IDを付けて保存し、その行の出自を `ai` として書き足す。
    /// スタブでない（台詞か地の文がある）シーン、差し込むとシーンの流れ（ジャンプ・選択肢・条件）が変わる下書きは
    /// 書き込まない。外部の変更とぶつかったとき・書けなかったときは本文を戻す。
    #[allow(clippy::too_many_arguments)]
    pub fn write_draft(
        &mut self,
        file: &str,
        scene: &str,
        body: &[String],
        task: &str,
        model: &str,
        seen: Option<&str>,
        generator: &mut LineIdGenerator,
    ) -> Result<DraftWritten, ProjectError> {
        use crate::ai::{draft_into_scene, is_stub, line_ids_in, scene_range, with_line_ids};
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        let refuse = |reason: String| DraftWritten {
            saved: None,
            conflict: None,
            refused: Some(reason),
            ids: Vec::new(),
        };
        let ir = crate::analyze(&self.project);
        let Some((scene_file, start, end)) = scene_range(&self.project, &ir, scene) else {
            return Ok(refuse(format!("シーン {scene} がありません")));
        };
        if scene_file != file {
            return Ok(refuse(format!("シーン {scene} は {file} にありません")));
        }
        if !is_stub(&ir, scene) {
            return Ok(refuse(format!(
                "シーン {scene} には台詞か地の文があるので、書き込みません（スタブにだけ書き込みます）"
            )));
        }
        if self.project.has_reading_errors() {
            // 本文の読み方の設定を読めないときは行IDを付けない（D042、D045）。下書きの出自を記録できないので書かない。
            return Ok(refuse(
                "project.toml の本文の読み方の設定を読めないので、行IDを付けられず、書き込みません"
                    .to_owned(),
            ));
        }
        let drafted = match current
            .get(start..end)
            .ok_or_else(|| "シーンの範囲を読めません".to_owned())
            .and_then(|scene_text| draft_into_scene(scene_text, body))
        {
            Ok(drafted) => drafted,
            Err(reason) => return Ok(refuse(reason)),
        };
        let new_text = format!(
            "{}{}{}",
            current.get(..start).unwrap_or_default(),
            drafted.text,
            current.get(end..).unwrap_or_default()
        );
        let range = (start + drafted.inserted.0, start + drafted.inserted.1);
        if line_ids_in(&new_text) != line_ids_in(&current) {
            return Ok(refuse("下書きに今ある行IDが入っています".to_owned()));
        }
        // 差し込んでもシーンの流れ（辺）が変わらないことを確かめる（閉じていない括弧が後ろの行を飲み込むなど）。
        let edges = |ir: &crate::Ir| {
            ir.edges
                .iter()
                .filter(|e| e.from == scene)
                .map(|e| {
                    (
                        e.to.clone(),
                        e.kind,
                        e.via,
                        e.label.clone(),
                        e.cond_text.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let mut trial = self.project.clone();
        if let Some(f) = trial.files.iter_mut().find(|f| f.path == file) {
            f.text = new_text.clone();
        }
        if edges(&crate::analyze(&trial)) != edges(&ir) {
            return Ok(refuse(
                "下書きを差し込むとシーンの流れ（ジャンプ・選択肢・条件）が変わるので、書き込みません".to_owned(),
            ));
        }
        let (with_ids, ids) = with_line_ids(&self.project, file, &new_text, range, generator);
        self.set_buffer(file, &with_ids);
        let outcome = match self.save(file, seen, generator) {
            Ok(outcome) => outcome,
            Err(e) => {
                self.set_buffer(file, &current);
                return Err(e);
            }
        };
        match outcome {
            SaveOutcome::Conflict { deleted } => {
                self.set_buffer(file, &current);
                Ok(DraftWritten {
                    saved: None,
                    conflict: Some(deleted),
                    refused: None,
                    ids: Vec::new(),
                })
            }
            SaveOutcome::Saved(mut saved) => {
                let at = provenance_now();
                let records: Vec<Record> = ids
                    .iter()
                    .map(|line| Record {
                        line: line.clone(),
                        origin: AI.to_owned(),
                        task: task.to_owned(),
                        model: model.to_owned(),
                        at: at.clone(),
                        ..Record::default()
                    })
                    .collect();
                if let Err(e) = append_provenance(&self.root, &records) {
                    saved.provenance_problem = Some(format!(
                        "{e}（下書きの {}行を記録できませんでした）",
                        ids.len()
                    ));
                }
                Ok(DraftWritten {
                    saved: Some(saved),
                    conflict: None,
                    refused: None,
                    ids,
                })
            }
        }
    }

    /// 書き足した本文を保存し、AI が書いた行（`ids`）の出自を `ai` と記録する（T3-10）。外部の変更とぶつかった
    /// とき・書けなかったときは本文を戻す。
    #[allow(clippy::too_many_arguments)]
    fn save_ai_text(
        &mut self,
        file: &str,
        current: &str,
        text: &str,
        ids: Vec<String>,
        task: &str,
        model: &str,
        seen: Option<&str>,
        generator: &mut LineIdGenerator,
    ) -> Result<DraftWritten, ProjectError> {
        self.set_buffer(file, text);
        let outcome = match self.save(file, seen, generator) {
            Ok(outcome) => outcome,
            Err(e) => {
                self.set_buffer(file, current);
                return Err(e);
            }
        };
        match outcome {
            SaveOutcome::Conflict { deleted } => {
                self.set_buffer(file, current);
                Ok(DraftWritten {
                    saved: None,
                    conflict: Some(deleted),
                    refused: None,
                    ids: Vec::new(),
                })
            }
            SaveOutcome::Saved(mut saved) => {
                let at = provenance_now();
                let records: Vec<Record> = ids
                    .iter()
                    .map(|line| Record {
                        line: line.clone(),
                        origin: AI.to_owned(),
                        task: task.to_owned(),
                        model: model.to_owned(),
                        at: at.clone(),
                        ..Record::default()
                    })
                    .collect();
                if !records.is_empty()
                    && let Err(e) = append_provenance(&self.root, &records)
                {
                    saved.provenance_problem = Some(format!(
                        "{e}（AI の {}行を記録できませんでした）",
                        ids.len()
                    ));
                }
                Ok(DraftWritten {
                    saved: Some(saved),
                    conflict: None,
                    refused: None,
                    ids,
                })
            }
        }
    }

    /// AI がシーンを書き足す章を確かめる（T3-10）：開いている、保存していない変更もまだ取り込んでいない外部の
    /// 変更も無い。今の本文を返す。だめならそのわけ。
    fn plot_target(&self, file: &str) -> Result<String, String> {
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(format!("作品フォルダに無い章です：{file}"));
        };
        if self.is_dirty(file) || self.pending.contains_key(file) {
            return Err(
                "この章には保存していない変更があります。保存してから書き足します".to_owned(),
            );
        }
        if self.project.has_reading_errors() {
            return Err(
                "project.toml の本文の読み方の設定を読めないので、行IDを付けられず、書き足しません"
                    .to_owned(),
            );
        }
        match read_disk(&path) {
            Disk::Text(on_disk) if self.disk_text(file) == Some(on_disk.as_str()) => Ok(current),
            _ => Err("章が外で変わっています。取り込んでから書き足します".to_owned()),
        }
    }

    /// 提案ノードを採用する（人の操作。T3-10、D071）：スタブのシーン（`status: ai-draft`）を選択肢を足すシーンの
    /// 章の末尾に足し、そのシーンに選択肢を書き足して（フロー図からの編集と同じ規則）保存する。足した選択肢の行の
    /// 出自を `ai` と記録し、足したシーンと選択肢を足したシーンを検証する。人が書いた行は変えない。
    pub fn adopt_branch(
        &mut self,
        proposal: &crate::ai::BranchProposal,
        task: &str,
        model: &str,
        generator: &mut LineIdGenerator,
    ) -> Result<PlotWritten, ProjectError> {
        let refuse = |file: &str, reason: String| PlotWritten {
            file: file.to_owned(),
            written: DraftWritten {
                saved: None,
                conflict: None,
                refused: Some(reason),
                ids: Vec::new(),
            },
            checks: Vec::new(),
        };
        if !proposal.problems.is_empty() {
            return Ok(refuse("", proposal.problems.join("。")));
        }
        let ir = crate::analyze(&self.project);
        let Some(file) = ir
            .scenes
            .iter()
            .find(|s| s.id == proposal.from)
            .map(|s| s.file.clone())
        else {
            return Ok(refuse("", format!("シーン {} がありません", proposal.from)));
        };
        let current = match self.plot_target(&file) {
            Ok(text) => text,
            Err(reason) => return Ok(refuse(&file, reason)),
        };
        let mut ids_seen = BTreeSet::new();
        for stub in &proposal.stubs {
            if let Err(e) = self.check_new_scene(&file, &stub.id, &stub.title) {
                return Ok(refuse(&file, e.to_string()));
            }
            if !ids_seen.insert(stub.id.as_str()) {
                return Ok(refuse(&file, format!("シーンID {} が2つあります", stub.id)));
            }
        }
        let nl = if current.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut with_stubs = current.clone();
        for stub in &proposal.stubs {
            with_stubs.push_str(&scene_separator(&with_stubs, nl));
            with_stubs.push_str(&crate::ai::skeleton_scene_text(
                &crate::ai::SkeletonScene {
                    id: stub.id.clone(),
                    title: stub.title.trim().to_owned(),
                    summary: stub.summary.clone(),
                    route: None,
                    lines: Vec::new(),
                },
                None,
                nl,
            ));
        }
        let mut trial = self.project.clone();
        if let Some(f) = trial.files.iter_mut().find(|f| f.path == file) {
            f.text = with_stubs.clone();
        }
        let insert = match crate::flowedit::insert_choice(
            &trial,
            &proposal.from,
            &proposal.target,
            &proposal.label,
            &proposal.effects,
        ) {
            Ok(insert) => insert,
            Err(reason) => return Ok(refuse(&file, reason)),
        };
        let (text, ids) =
            crate::ai::with_line_ids(&trial, &file, &insert.text, insert.inserted, generator);
        if let Some(reason) = more_errors(&self.project, &file, &text) {
            return Ok(refuse(&file, reason));
        }
        let seen = self.disk_text(&file).map(str::to_owned);
        let written = self.save_ai_text(
            &file,
            &current,
            &text,
            ids,
            task,
            model,
            seen.as_deref(),
            generator,
        )?;
        let mut scenes = vec![proposal.from.clone()];
        scenes.extend(proposal.stubs.iter().map(|s| s.id.clone()));
        let checks = if written.saved.is_some() {
            let ir = crate::analyze(&self.project);
            crate::ai::scene_checks(&self.project, &ir, &scenes)
        } else {
            Vec::new()
        };
        Ok(PlotWritten {
            file,
            written,
            checks,
        })
    }

    /// ルートの骨格を書く（T3-10、D071）：スタブのシーン（見出し、`summary`・`route`・`status: ai-draft`、流れの
    /// 行）を章の末尾に足して保存し、足した行のうち行IDの付く行（選択肢）の出自を `ai` と記録する。今あるシーンは
    /// 変えない。シーンIDが重なる、行き先が無い、足した範囲に誤り（E）が出るなら書かない。
    pub fn write_skeleton(
        &mut self,
        file: &str,
        scenes: &[crate::ai::SkeletonScene],
        route: Option<&str>,
        task: &str,
        model: &str,
        generator: &mut LineIdGenerator,
    ) -> Result<PlotWritten, ProjectError> {
        let refuse = |reason: String| PlotWritten {
            file: file.to_owned(),
            written: DraftWritten {
                saved: None,
                conflict: None,
                refused: Some(reason),
                ids: Vec::new(),
            },
            checks: Vec::new(),
        };
        if scenes.is_empty() {
            return Ok(refuse("足すシーンがありません".to_owned()));
        }
        let current = match self.plot_target(file) {
            Ok(text) => text,
            Err(reason) => return Ok(refuse(reason)),
        };
        let ir = crate::analyze(&self.project);
        let mut problems = Vec::new();
        let mut ids = BTreeSet::new();
        for scene in scenes {
            if !crate::ai::is_scene_id(&scene.id) {
                problems.push(format!("シーンID {} は英数字と _ で書きます", scene.id));
            } else if ir.scenes.iter().any(|s| s.id == scene.id) {
                problems.push(format!("シーンID {} はもう使われています", scene.id));
            } else if !ids.insert(scene.id.as_str()) {
                problems.push(format!("シーンID {} が2つあります", scene.id));
            }
            if scene.title.trim().is_empty() || scene.title.contains(['\n', '\r']) {
                problems.push(format!(
                    "シーン {} の題名が空か、改行を含んでいます",
                    scene.id
                ));
            }
        }
        for scene in scenes {
            for to in crate::ai::skeleton_targets(scene) {
                if !ids.contains(to.as_str()) && !ir.scenes.iter().any(|s| s.id == to) {
                    problems.push(format!("シーン {} の行き先 {to} がありません", scene.id));
                }
            }
        }
        if !problems.is_empty() {
            return Ok(refuse(problems.join("。")));
        }
        let nl = if current.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut text = current.clone();
        let start = text.len() + scene_separator(&text, nl).len();
        for scene in scenes {
            text.push_str(&scene_separator(&text, nl));
            text.push_str(&crate::ai::skeleton_scene_text(scene, route, nl));
        }
        let mut trial = self.project.clone();
        if let Some(f) = trial.files.iter_mut().find(|f| f.path == file) {
            f.text = text.clone();
        }
        let errors: Vec<String> = crate::analyze(&trial)
            .diagnostics
            .iter()
            .filter(|d| d.file == file && d.span.start >= start && d.code.starts_with('E'))
            .map(|d| format!("{} {}", d.code, d.message))
            .collect();
        if !errors.is_empty() {
            return Ok(refuse(format!(
                "足すシーンに誤りがあるので、書きません：{}",
                errors.join("、")
            )));
        }
        let (text, line_ids) =
            crate::ai::with_line_ids(&trial, file, &text, (start, text.len()), generator);
        let seen = self.disk_text(file).map(str::to_owned);
        let written = self.save_ai_text(
            file,
            &current,
            &text,
            line_ids,
            task,
            model,
            seen.as_deref(),
            generator,
        )?;
        let checks = if written.saved.is_some() {
            let ir = crate::analyze(&self.project);
            let added: Vec<String> = scenes.iter().map(|s| s.id.clone()).collect();
            crate::ai::scene_checks(&self.project, &ir, &added)
        } else {
            Vec::new()
        };
        Ok(PlotWritten {
            file: file.to_owned(),
            written,
            checks,
        })
    }

    /// シーンのメタ情報の `summary` を書く（同期のずれを採用する人の操作。T3-10、D071）：その1行だけを書き換え
    /// （無ければ足し、メタ情報が無ければ作る）、章を保存する（保存していない変更も一緒に）。`seen` はフロントが
    /// 知っているディスクの中身。
    pub fn set_scene_summary(
        &mut self,
        file: &str,
        scene: &str,
        summary: &str,
        seen: Option<&str>,
        generator: &mut LineIdGenerator,
    ) -> Result<DraftWritten, ProjectError> {
        let refuse = |reason: String| DraftWritten {
            saved: None,
            conflict: None,
            refused: Some(reason),
            ids: Vec::new(),
        };
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        let Some(summary) = crate::ideas::summary_line(summary) else {
            return Ok(refuse("要約が空か、メタ情報に書けない形です".to_owned()));
        };
        let ir = crate::analyze(&self.project);
        let Some((scene_file, start, end)) = crate::ai::scene_range(&self.project, &ir, scene)
        else {
            return Ok(refuse(format!("シーン {scene} がありません")));
        };
        if scene_file != file {
            return Ok(refuse(format!("シーン {scene} は {file} にありません")));
        }
        let scene_text = current.get(start..end).unwrap_or_default();
        let replaced = match crate::ai::set_meta_line(scene_text, "summary", &summary) {
            Ok(text) => text,
            Err(reason) => return Ok(refuse(reason)),
        };
        let text = format!(
            "{}{replaced}{}",
            current.get(..start).unwrap_or_default(),
            current.get(end..).unwrap_or_default()
        );
        self.save_ai_text(file, &current, &text, Vec::new(), "", "", seen, generator)
    }

    /// 外部の変更（ファイルの監視の知らせ）を取り込む。
    pub fn external_change(&mut self, path: &Path) -> Change {
        let Some(rel) = self.relative(path) else {
            return Change::None;
        };
        let parts: Vec<&str> = rel.split('/').collect();
        let hidden = parts.last().is_some_and(|name| name.starts_with('.'));
        if hidden {
            return Change::None;
        }
        match parts.as_slice() {
            // `story/` の下のフォルダの章も（T2-10、D061）。隠しフォルダの中は読まない（上で隠しファイルは除いた）。
            ["story", .., name]
                if name.ends_with(".scn") && !parts.iter().any(|p| p.starts_with('.')) =>
            {
                self.story_change(&rel)
            }
            [LAYOUT_FILE] => {
                let on_disk = fs::read(self.root.join(LAYOUT_FILE)).ok();
                let mine = self.layout_written.as_deref().map(str::as_bytes);
                if on_disk.is_some() && on_disk.as_deref() == mine {
                    Change::None
                } else {
                    // 外で変わった。後で自分の書いた中身に戻されても（取り消しなど）、読み直すように忘れる。
                    self.layout_written = None;
                    Change::Layout
                }
            }
            _ if rel == PROVENANCE_FILE => Change::Provenance,
            ["ai", "proposals", name] if name.ends_with(".md") => Change::Proposals,
            _ if is_idea_path(&rel) => {
                let on_disk = fs::read(self.root.join(&rel)).ok();
                let mine = self.settings_written.get(&rel).map(String::as_bytes);
                if on_disk.is_some() && on_disk.as_deref() == mine {
                    return Change::None;
                }
                self.settings_written.remove(&rel);
                Change::Ideas
            }
            // AIタスクの定義（`ai/` の直下の `*.yaml`。T3-6）も設定として読み直す。
            _ if SETTINGS_FILES.contains(&rel.as_str())
                || parts.first() == Some(&"bible")
                || matches!(parts.as_slice(), ["ai", name] if name.ends_with(".yaml")) =>
            {
                let on_disk = fs::read(self.root.join(&rel)).ok();
                let mine = self.settings_written.get(&rel).map(String::as_bytes);
                if on_disk.is_some() && on_disk.as_deref() == mine {
                    return Change::None;
                }
                if on_disk.is_none() && self.moved_away.contains(&rel) {
                    return Change::None;
                }
                // 外で変わった。後で自分の書いた中身に戻されても（取り消しなど）、読み直すように忘れる。
                self.settings_written.remove(&rel);
                self.moved_away.remove(&rel);
                match self.reload_settings() {
                    Ok(()) => Change::Settings,
                    Err(_) => Change::None,
                }
            }
            _ => Change::None,
        }
    }

    fn story_change(&mut self, file: &str) -> Change {
        let on_disk = match read_disk(&self.root.join(file)) {
            Disk::Text(text) => Some(text),
            Disk::Missing => None,
            Disk::Unreadable => {
                return Change::Unreadable {
                    file: file.to_owned(),
                };
            }
        };
        if self.file(file).is_none() {
            let Some(text) = on_disk else {
                return Change::None;
            };
            self.insert_file(file, &text);
            return Change::Added {
                file: file.to_owned(),
                text,
            };
        }
        // 自分で書いた中身や、もう知らせた中身なら、何もしない。
        if self.disk.get(file) == Some(&on_disk) || self.pending.get(file) == Some(&on_disk) {
            return Change::None;
        }
        if self.is_dirty(file) {
            let deleted = on_disk.is_none();
            self.pending.insert(file.to_owned(), on_disk);
            return Change::Conflict {
                file: file.to_owned(),
                deleted,
            };
        }
        match on_disk {
            Some(text) => {
                self.set_buffer(file, &text);
                self.synced(file, &text);
                Change::Reloaded {
                    file: file.to_owned(),
                    text,
                }
            }
            None => {
                self.remove_file(file);
                Change::Removed {
                    file: file.to_owned(),
                }
            }
        }
    }

    fn insert_file(&mut self, file: &str, text: &str) {
        self.project.files.push(SourceFile {
            path: file.to_owned(),
            text: text.to_owned(),
        });
        self.project.files.sort_by(|a, b| a.path.cmp(&b.path));
        self.synced(file, text);
        self.removed.remove(file);
    }

    /// 外で消されたファイルを閉じる。
    fn remove_file(&mut self, file: &str) {
        self.project.files.retain(|f| f.path != file);
        self.disk.remove(file);
        self.base.remove(file);
        self.pending.remove(file);
        self.removed.insert(file.to_owned());
    }

    /// 「読み込み直す」：外の中身にする。外で消されていれば、ファイルを閉じて `None` を返す。
    /// 開いているファイル（と、外で消されて閉じたファイル）だけを受け付ける。ほかのパスは読まない
    /// （フロントに任意のパスを読ませない。`.claude/rules/rust.md`）。
    pub fn reload_from_disk(&mut self, file: &str) -> Result<Option<String>, ProjectError> {
        let path = self.root.join(file);
        let on_disk = if let Some(pending) = self.pending.remove(file) {
            pending
        } else if self.file(file).is_some() {
            match read_disk(&path) {
                Disk::Text(text) => Some(text),
                Disk::Missing => None,
                Disk::Unreadable => {
                    return Err(error(
                        &path,
                        io::ErrorKind::InvalidData,
                        "UTF-8で読めません",
                    ));
                }
            }
        } else if self.removed.contains(file) {
            return Ok(None);
        } else {
            return Err(not_open(&path));
        };
        match on_disk {
            Some(text) => {
                self.set_buffer(file, &text);
                self.synced(file, &text);
                Ok(Some(text))
            }
            None => {
                self.remove_file(file);
                Ok(None)
            }
        }
    }

    /// 「自分の変更を残す」：`text`（保存していない変更）のままにする。同じ外の中身については、もう聞かない。
    /// 外で消されていたなら、保存でファイルを作り直す。コアが先に閉じたファイル（アプリにまだ送っていない
    /// 編集があった）は開き直す。ディスクの中身（外の中身。消されていれば `None`）を返す。
    pub fn keep_mine(&mut self, file: &str, text: &str) -> Result<Option<String>, ProjectError> {
        if let Some(on_disk) = self.pending.remove(file) {
            self.disk.insert(file.to_owned(), on_disk);
        }
        if !self.set_buffer(file, text) {
            if !self.removed.remove(file) {
                return Err(not_open(&self.root.join(file)));
            }
            self.project.files.push(SourceFile {
                path: file.to_owned(),
                text: text.to_owned(),
            });
            self.project.files.sort_by(|a, b| a.path.cmp(&b.path));
            self.disk.insert(file.to_owned(), None);
        }
        Ok(self.disk.get(file).cloned().flatten())
    }

    /// 設定ファイルを読み直す。本文は今の中身（保存していない編集）のまま。
    pub fn reload_settings(&mut self) -> Result<(), ProjectError> {
        let mut fresh = load_project(&self.root)?;
        fresh.files = std::mem::take(&mut self.project.files);
        self.project = fresh;
        Ok(())
    }

    /// 未登録の話者（W202）を、設定DBのキャラクターとして登録する（T1-8、D047）。キャラクターの型で
    /// [`Workspace::create_entry`] を呼ぶ。
    pub fn register_character(&mut self, id: &str, name: &str) -> Result<String, ProjectError> {
        let type_id = self.project.bible.character_type().to_owned();
        self.create_entry(&type_id, id, name)
    }

    // ---- 設定DBと変数（T1-10、D049） ----

    /// 設定DBのエントリのパス（`bible/entries/<型>/<名前>.md`）を確かめる。ほかのパスは読み書きしない
    /// （フロントに任意のパスを読ませない。`.claude/rules/rust.md`）。
    /// フォームとソースで書き換える文書（設定DBのエントリ、企画書、アイデアメモ）のパス。
    fn entry_path(&self, file: &str) -> Result<PathBuf, ProjectError> {
        let parts: Vec<&str> = file.split('/').collect();
        let safe = |part: &str| !part.is_empty() && !part.starts_with('.') && !part.contains('\\');
        match parts.as_slice() {
            ["bible", "entries", type_id, name]
                if safe(type_id) && safe(name) && name.ends_with(".md") =>
            {
                Ok(self.root.join(file))
            }
            [CONCEPT_FILE] => Ok(self.root.join(file)),
            _ if is_idea_path(file) => Ok(self.root.join(file)),
            _ => Err(error(
                &self.root.join(file),
                io::ErrorKind::InvalidInput,
                "設定DBのエントリ、企画書、アイデアメモのパスではありません",
            )),
        }
    }

    /// ディスクの中身を読む。無ければ（外で消された）エラー。
    fn read_settings_file(&self, path: &Path) -> Result<String, ProjectError> {
        match read_disk(path) {
            Disk::Text(text) => Ok(text),
            Disk::Missing => Err(error(
                path,
                io::ErrorKind::NotFound,
                "ファイルがありません（外で消されたかもしれません）",
            )),
            Disk::Unreadable => Err(error(path, io::ErrorKind::InvalidData, "UTF-8で読めません")),
        }
    }

    /// 設定ファイルを書き、覚えて（監視の知らせを無視するため）、設定を読み直す。アイデアメモは設定ではない
    /// ので、読み直さない。
    fn write_settings_file(&mut self, file: &str, text: &str) -> Result<(), ProjectError> {
        write_source(&self.root, file, text)?;
        self.settings_written
            .insert(file.to_owned(), text.to_owned());
        if is_idea_path(file) {
            return Ok(());
        }
        self.reload_settings()
    }

    /// 人が企画書かエントリを書いた後に、直した AI の項目を記録する（T4-3）。書けなかったら、わけを
    /// [`Self::take_field_provenance_problem`] で受け取れるようにする（書いた中身は残す）。
    fn record_edited_ai_fields(&mut self, file: &str, before: &str, after: &str) {
        if !is_field_file(file) {
            return;
        }
        let result = read_provenance(&self.root).and_then(|provenance| {
            let records = edited_ai_fields(file, before, after, &provenance, &provenance_now());
            append_provenance(&self.root, &records)
        });
        self.field_provenance_problem = result.err().map(|e| e.to_string());
    }

    /// 企画書かエントリを作り直したパスに残っている AI の項目の記録を打ち消す（T4-3）。
    fn reset_stale_fields(&mut self, file: &str) {
        let result = read_provenance(&self.root).and_then(|provenance| {
            append_provenance(
                &self.root,
                &reset_field_records(&provenance, file, &provenance_now()),
            )
        });
        self.field_provenance_problem = result.err().map(|e| e.to_string());
    }

    /// 直前の書き換えで、企画書・エントリの項目の出自を書けなかったわけ（T4-3）。
    pub fn take_field_provenance_problem(&mut self) -> Option<String> {
        self.field_provenance_problem.take()
    }

    /// 設定DBのエントリのファイル（読めたかどうかによらず、`bible/entries/<型>/*.md` のすべて）。
    /// パスの順。中身がUTF-8で読めなければ `None`。
    pub fn entry_files(&self) -> Vec<(String, Option<String>)> {
        let mut files = Vec::new();
        let Ok(types) = fs::read_dir(self.root.join("bible/entries")) else {
            return files;
        };
        for type_dir in types.flatten() {
            let type_path = type_dir.path();
            if !type_path.is_dir() {
                continue;
            }
            let Some(type_id) = type_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Ok(entries) = fs::read_dir(&type_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || !name.ends_with(".md") {
                    continue;
                }
                let text = match read_disk(&entry.path()) {
                    Disk::Text(text) => Some(text),
                    _ => None,
                };
                files.push((format!("bible/entries/{type_id}/{name}"), text));
            }
        }
        files.sort();
        files
    }

    /// エントリの1つの項目の値を書き換える（[`set_field`]）。`seen` は、フロントが知っているディスクの中身。
    /// 外で変わっていても、その項目が外で変わっていなければ、今の中身に重ねて書く。その項目も外で
    /// 変わっていれば書かずに [`EntryEdit::Conflict`] を返す。
    pub fn set_entry_field(
        &mut self,
        file: &str,
        key: &str,
        value: &serde_json::Value,
        seen: &str,
    ) -> Result<EntryEdit, ProjectError> {
        let path = self.entry_path(file)?;
        let current = self.read_settings_file(&path)?;
        if current != seen {
            let before = front_fields(seen).ok().and_then(|f| f.get(key).cloned());
            let now = front_fields(&current)
                .ok()
                .and_then(|f| f.get(key).cloned());
            if before != now {
                return Ok(EntryEdit::Conflict { text: current });
            }
        }
        match set_field(&current, key, value) {
            Ok(text) => {
                self.write_settings_file(file, &text)?;
                self.record_edited_ai_fields(file, &current, &text);
                Ok(EntryEdit::Saved { text })
            }
            Err(e) => Ok(EntryEdit::Unsupported {
                message: e.to_string(),
            }),
        }
    }

    /// エントリの本文（フロントマターの後ろ）を書き換える。外で変わっていても、本文が外で変わっていなければ
    /// 今の中身に重ねて書く。本文も外で変わっていれば書かずに [`EntryEdit::Conflict`] を返す。
    pub fn set_entry_body(
        &mut self,
        file: &str,
        body: &str,
        seen: &str,
    ) -> Result<EntryEdit, ProjectError> {
        let path = self.entry_path(file)?;
        let current = self.read_settings_file(&path)?;
        let mut body = body.to_owned();
        if current != seen && body_of(&current) != body_of(seen) {
            // 外でも本文が変わった：3方向でマージし（T2-7、D058）、ぶつからなければ書く。
            let lf = |text: &str| text.replace("\r\n", "\n");
            let merged = merge_text(&lf(body_of(seen)), &lf(&body), &lf(body_of(&current)));
            match merged.resolve(&[]) {
                Some(text) => body = text,
                None => return Ok(EntryEdit::Conflict { text: current }),
            }
        }
        let text = set_body(&current, &body);
        self.write_settings_file(file, &text)?;
        self.record_edited_ai_fields(file, &current, &text);
        Ok(EntryEdit::Saved { text })
    }

    /// エントリのファイル全体を書く（ソース表示）。`seen` が今の中身と違えば（外で変わった）、書かずに
    /// [`EntryEdit::Conflict`] を返す。`seen` が `None` なら上書きする（人が「上書きする」を選んだ）。
    /// 改行は、今のファイルの改行にそろえる（画面の入力欄は `\n` で返すため）。
    pub fn write_entry_source(
        &mut self,
        file: &str,
        text: &str,
        seen: Option<&str>,
    ) -> Result<EntryEdit, ProjectError> {
        let path = self.entry_path(file)?;
        let current = self.read_settings_file(&path)?;
        if seen.is_some_and(|seen| seen != current) {
            return Ok(EntryEdit::Conflict { text: current });
        }
        let text = same_newlines(&current, text);
        self.write_settings_file(file, &text)?;
        self.record_edited_ai_fields(file, &current, &text);
        Ok(EntryEdit::Saved { text })
    }

    /// エントリを新しく作る：`bible/entries/<型>/<id>.md` に、`id` と `name` だけのフロントマターを書き、
    /// 設定を読み直す。型は型定義のある型か、キャラクターの型。`id` は英数字と `_`（変数の名前空間や
    /// 立ち絵の差分のキーにもなるため）。書いたファイルのパスを返す。
    pub fn create_entry(
        &mut self,
        type_id: &str,
        id: &str,
        name: &str,
    ) -> Result<String, ProjectError> {
        self.create_entry_with_body(type_id, id, name, "")
    }

    /// エントリを新しく作り、本文（フロントマターの後ろ）に `body` を書く（[`Self::create_entry`]）。
    fn create_entry_with_body(
        &mut self,
        type_id: &str,
        id: &str,
        name: &str,
        body: &str,
    ) -> Result<String, ProjectError> {
        let file = self.check_new_entry(type_id, id, name, false)?;
        let dir = format!("bible/entries/{type_id}");
        let invalid =
            |message: &str| error(&self.root.join(&dir), io::ErrorKind::InvalidInput, message);
        let name = name.trim();
        // `0x1F`・`1e3`・`null` のように、素のままでは文字列として読めないIDは引用して書く。
        let text = set_field(
            &format!("---\nid: {}\n---\n", yaml_scalar(id)),
            "name",
            &serde_json::Value::String(name.to_owned()),
        )
        .map_err(|e| invalid(&e.to_string()))?;
        let text = if body.is_empty() {
            text
        } else {
            set_body(&text, body)
        };
        let folder = self.root.join(&dir);
        fs::create_dir_all(&folder).map_err(|source| ProjectError::Write {
            path: folder,
            source,
        })?;
        self.write_settings_file(&file, &text)?;
        self.reset_stale_fields(&file);
        Ok(file)
    }

    /// 新しいエントリを作れるかを確かめ、書くパスを返す（[`Self::create_entry`]）。`type_to_come` なら、型の定義が
    /// まだ無くてもよい（伏線への昇格で、標準の定義を先に作るとき）。
    fn check_new_entry(
        &self,
        type_id: &str,
        id: &str,
        name: &str,
        type_to_come: bool,
    ) -> Result<String, ProjectError> {
        let bible = &self.project.bible;
        let dir = format!("bible/entries/{type_id}");
        let invalid =
            |message: &str| error(&self.root.join(&dir), io::ErrorKind::InvalidInput, message);
        let known = type_to_come
            || bible.types.iter().any(|t| t.id == type_id)
            || type_id == bible.character_type();
        if !known || type_id.is_empty() || type_id.starts_with('.') || type_id.contains(['/', '\\'])
        {
            return Err(invalid("型の定義（bible/types/<型>.yaml）の無い型です"));
        }
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(invalid("IDは英数字と _ で書きます"));
        }
        let name = name.trim();
        if name.is_empty() || name.contains(['\n', '\r']) {
            return Err(invalid("名前が空か、改行を含んでいます"));
        }
        let file = format!("{dir}/{id}.md");
        let path = self.root.join(&file);
        let taken = bible
            .entries
            .iter()
            .any(|e| e.type_id == type_id && e.id == id);
        if taken || path.exists() {
            return Err(error(
                &path,
                io::ErrorKind::AlreadyExists,
                "同じIDのエントリがあります",
            ));
        }
        Ok(file)
    }

    /// `variables.yaml` の中身（無ければ `None`）。
    pub fn variables_text(&self) -> Result<Option<String>, ProjectError> {
        let path = self.root.join(VARIABLES_FILE);
        match read_disk(&path) {
            Disk::Text(text) => Ok(Some(text)),
            Disk::Missing => Ok(None),
            Disk::Unreadable => Err(error(
                &path,
                io::ErrorKind::InvalidData,
                "UTF-8で読めません",
            )),
        }
    }

    /// `variables.yaml` を書く（変数の宣言はテキストで直す。フォームはフェーズ2）。`seen` が今の中身（無ければ
    /// 空の文字列）と違えば、書かずに [`EntryEdit::Conflict`] を返す。`None` なら上書きする。
    pub fn write_variables(
        &mut self,
        text: &str,
        seen: Option<&str>,
    ) -> Result<EntryEdit, ProjectError> {
        let current = self.variables_text()?.unwrap_or_default();
        if seen.is_some_and(|seen| seen != current) {
            return Ok(EntryEdit::Conflict { text: current });
        }
        let text = same_newlines(&current, text);
        self.write_settings_file(VARIABLES_FILE, &text)?;
        Ok(EntryEdit::Saved { text })
    }

    /// 変数の宣言の1つの項目を直す（フォーム。T2-10、D061）。`seen` は画面が知っている中身で、外で変わって
    /// いても、その宣言のその項目が外で変わっていなければ、今の中身に重ねて書く。変わっていれば
    /// [`EntryEdit::Conflict`]。直したキーの行のほかを変えずに書けなければ [`EntryEdit::Unsupported`]。
    pub fn set_variable_field(
        &mut self,
        key: &str,
        field: &str,
        value: &serde_json::Value,
        seen: &str,
    ) -> Result<EntryEdit, ProjectError> {
        let current = self.variables_text()?.unwrap_or_default();
        if current != seen {
            let of = |text: &str| -> Option<serde_json::Value> {
                let doc: yaml_serde::Value = yaml_serde::from_str(without_bom(text)).ok()?;
                let doc = serde_json::to_value(doc).ok()?;
                let bare = key.strip_prefix("flag.").unwrap_or(key);
                doc.get(key)
                    .or_else(|| doc.get(bare))
                    .and_then(|d| d.get(field))
                    .cloned()
            };
            if of(seen) != of(&current) {
                return Ok(EntryEdit::Conflict { text: current });
            }
        }
        match set_decl_field(&current, key, field, value) {
            Ok(text) => {
                self.write_settings_file(VARIABLES_FILE, &text)?;
                Ok(EntryEdit::Saved { text })
            }
            Err(e) => Ok(EntryEdit::Unsupported {
                message: e.to_string(),
            }),
        }
    }

    /// 変数を宣言する（フォーム。T2-10、D061）。ファイルが無ければ作る。
    pub fn add_variable(
        &mut self,
        key: &str,
        ty: &str,
        values: &[String],
    ) -> Result<EntryEdit, ProjectError> {
        let current = self.variables_text()?.unwrap_or_default();
        let text = add_decl(&current, key, ty, values).map_err(ProjectError::Refused)?;
        self.write_settings_file(VARIABLES_FILE, &text)?;
        Ok(EntryEdit::Saved { text })
    }

    /// 型定義のファイルのパス（`bible/types/<型>.yaml`）。型のIDが正しくなければ断る。
    fn type_path(&self, type_id: &str) -> Result<String, ProjectError> {
        if type_id.is_empty()
            || type_id.starts_with('.')
            || type_id.contains(['/', '\\'])
            || !type_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(ProjectError::Refused(
                "型のIDは英数字と _ で書きます".to_owned(),
            ));
        }
        Ok(format!("bible/types/{type_id}.yaml"))
    }

    /// 型定義のファイルの中身（T2-10）。
    pub fn type_text(&self, type_id: &str) -> Result<String, ProjectError> {
        let file = self.type_path(type_id)?;
        self.read_settings_file(&self.root.join(file))
    }

    /// 型定義を編集する（フォーム。T2-10、D061。[`crate::typedef`]）。`seen` が今の中身と違えば（外で変わった）
    /// 書かずに [`EntryEdit::Conflict`]。直した行のほかを変えずに書けなければ [`EntryEdit::Unsupported`]。
    pub fn edit_type(
        &mut self,
        type_id: &str,
        edit: &TypeEdit,
        seen: &str,
    ) -> Result<EntryEdit, ProjectError> {
        let file = self.type_path(type_id)?;
        let current = self.read_settings_file(&self.root.join(&file))?;
        if current != seen {
            return Ok(EntryEdit::Conflict { text: current });
        }
        match edit_type(&current, edit) {
            Ok(text) => {
                self.write_settings_file(&file, &text)?;
                Ok(EntryEdit::Saved { text })
            }
            Err(e) => Ok(EntryEdit::Unsupported {
                message: e.to_string(),
            }),
        }
    }

    /// 型を作る（`bible/types/<型>.yaml` に名前だけを書く。T2-10）。もうあれば断る。
    pub fn create_type(&mut self, type_id: &str, name: &str) -> Result<String, ProjectError> {
        let file = self.type_path(type_id)?;
        let name = name.trim();
        if name.is_empty() || name.contains(['\n', '\r']) {
            return Err(ProjectError::Refused("型の名前を1行で書きます".to_owned()));
        }
        let path = self.root.join(&file);
        if path.exists() || self.project.bible.types.iter().any(|t| t.id == type_id) {
            return Err(ProjectError::Refused(format!(
                "型 {type_id} はもうあります"
            )));
        }
        let dir = self.root.join("bible/types");
        fs::create_dir_all(&dir).map_err(|source| ProjectError::Write { path: dir, source })?;
        let text = format!("name: {}\nfields:\n", yaml_scalar(name));
        self.write_settings_file(&file, &text)?;
        Ok(file)
    }

    // ---- 素材カタログ（T2-3、D054） ----

    /// 素材カタログのテキスト（無ければ `None`）。
    pub fn catalog_text(&self) -> Result<Option<String>, ProjectError> {
        let path = self.root.join(CATALOG_FILE);
        match read_disk(&path) {
            Disk::Text(text) => Ok(Some(text)),
            Disk::Missing => Ok(None),
            Disk::Unreadable => Err(error(
                &path,
                io::ErrorKind::InvalidData,
                "UTF-8で読めません",
            )),
        }
    }

    /// 今のカタログに `edit` を当てて書く（無ければ作る）。変わらなければ書かない。外の変更とぶつからないよう、
    /// いつも今のディスクの中身に当てる（足すか1行を差し替えるだけで、ほかの行は変えないため）。
    fn edit_catalog(
        &mut self,
        edit: impl FnOnce(&str) -> Result<String, CatalogEditError>,
    ) -> Result<CatalogEdit, ProjectError> {
        let current = self.catalog_text()?.unwrap_or_default();
        let text = match edit(&current) {
            Ok(text) => text,
            Err(e) => {
                return Ok(CatalogEdit::Unsupported {
                    message: e.message(),
                });
            }
        };
        if text == current {
            return Ok(CatalogEdit::Unchanged);
        }
        let dir = self.root.join("assets");
        fs::create_dir_all(&dir).map_err(|source| ProjectError::Write {
            path: dir.clone(),
            source,
        })?;
        self.write_settings_file(CATALOG_FILE, &text)?;
        Ok(CatalogEdit::Saved { text })
    }

    /// カタログに意味名を足す（仮素材。[`add_entry`]）。まとめて足し、1回だけ書く。立ち絵の差分は、
    /// キャラクターが設定DBにあるときだけ足す。
    pub fn add_catalog_entries(&mut self, items: &[AssetKey]) -> Result<CatalogEdit, ProjectError> {
        let unknown = items.iter().find(|item| {
            item.kind == "expression"
                && !item
                    .character
                    .as_deref()
                    .is_some_and(|id| self.project.characters.iter().any(|c| c.id == id))
        });
        if let Some(item) = unknown {
            return Ok(CatalogEdit::Unsupported {
                message: format!(
                    "{} は設定DBに無いキャラクターなので、差分をカタログに足せません。先にキャラクターを登録してください",
                    item.character.as_deref().unwrap_or("（キャラクターなし）")
                ),
            });
        }
        self.edit_catalog(|text| {
            let mut text = text.to_owned();
            for item in items {
                text = add_entry(&text, &item.kind, item.character.as_deref(), &item.name)?;
            }
            Ok(text)
        })
    }

    /// カタログの意味名に、`assets/` の中のファイルを割り当てる（[`set_file`]）。`file` は選んだファイルの
    /// パス。`assets/` の外のファイルは割り当てない。
    pub fn assign_asset_file(
        &mut self,
        item: &AssetKey,
        file: &Path,
    ) -> Result<CatalogEdit, ProjectError> {
        let Some(relative) = assets_relative(file, &self.root) else {
            return Ok(CatalogEdit::Unsupported {
                message: "作品フォルダの assets/ の中のファイルを選んでください".to_owned(),
            });
        };
        self.edit_catalog(|text| {
            set_file(
                text,
                &item.kind,
                item.character.as_deref(),
                &item.name,
                &relative,
            )
        })
    }

    /// フロー図の配置を保存する（[`update_layout`]）。書いた中身を覚え、その監視の知らせを無視する
    /// （自分で書いた配置を読み直して、動かしている途中のノードを戻さないため。D048）。
    pub fn update_layout(
        &mut self,
        level: &str,
        positions: &BTreeMap<String, Point>,
        replace: bool,
    ) -> Result<(), LayoutError> {
        let text = update_layout(&self.root, level, positions, replace)?;
        self.layout_written = Some(text);
        Ok(())
    }

    /// `story/` に章（空の `.scn`）を足す。`name` に `.scn` が無ければ補う。`part1/ch01` のように `/` で区切れば、
    /// `story/` の下のフォルダに足す（無ければ作る。T2-10、D061）。足したファイルのパスを返す。
    pub fn add_chapter(&mut self, name: &str) -> Result<String, ProjectError> {
        let name = chapter_name(name).ok_or_else(|| {
            error(
                &self.root.join("story"),
                io::ErrorKind::InvalidInput,
                "章のファイル名は、`.` で始まらない名前を `/` で区切って書きます（`..` は使えません）",
            )
        })?;
        let file = format!("story/{name}");
        let path = self.root.join(&file);
        if self.file(&file).is_some() || path.exists() {
            return Err(error(
                &path,
                io::ErrorKind::AlreadyExists,
                "同じ名前の章があります",
            ));
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|source| ProjectError::Write {
                path: dir.to_owned(),
                source,
            })?;
        }
        write_source(&self.root, &file, "")?;
        self.insert_file(&file, "");
        Ok(file)
    }

    // ---- フロー図からの編集（T2-9、D060） ----

    /// 章の末尾にスタブのシーン（見出しと要約）を足す。保存していない変更のある章、まだ取り込んでいない外部の
    /// 変更のある章には足さない。要約はメタ情報の値として読み違えない1行だけ（`ideas::summary_line`）。
    pub fn add_stub(
        &mut self,
        file: &str,
        id: &str,
        title: &str,
        summary: Option<&str>,
    ) -> Result<StoryWritten, ProjectError> {
        let title = self.check_new_scene(file, id, title)?;
        let summary = summary.and_then(summary_line);
        let text = self.append_scene(file, id, &title, summary.as_deref())?;
        Ok(StoryWritten {
            file: file.to_owned(),
            text,
        })
    }

    /// フロー図で引いた辺を、本文に書き足す（ジャンプか選択肢。[`crate::flowedit`]）。書き足す章に保存していない
    /// 変更か、まだ取り込んでいない外部の変更があれば断る。
    pub fn add_edge(&mut self, edge: &FlowEdge) -> Result<StoryWritten, ProjectError> {
        let insert = insert_edge(&self.project, edge).map_err(ProjectError::Refused)?;
        let path = self.root.join(&insert.file);
        if self.is_dirty(&insert.file) || self.pending.contains_key(&insert.file) {
            return Err(error(
                &path,
                io::ErrorKind::InvalidInput,
                "この章には保存していない変更があります。保存してから書き足してください",
            ));
        }
        match read_disk(&path) {
            Disk::Text(on_disk) if self.disk_text(&insert.file) == Some(on_disk.as_str()) => {}
            _ => {
                return Err(error(
                    &path,
                    io::ErrorKind::InvalidData,
                    "章が外で変わっています。取り込んでから書き足してください",
                ));
            }
        }
        write_source(&self.root, &insert.file, &insert.text)?;
        self.set_buffer(&insert.file, &insert.text);
        self.synced(&insert.file, &insert.text);
        Ok(StoryWritten {
            file: insert.file,
            text: insert.text,
        })
    }

    // ---- 名前の変更と章の名前の変更・削除（T2-8、D059） ----

    /// 名前を変える（[`crate::rename`]）。置き換える本文のファイルは、保存していない変更ごと書く（変わるファイルを
    /// すべてそろえて書くため）。まだ取り込んでいない外部の変更のある章があれば、何も変えずに断る。
    pub fn rename(&mut self, target: &RenameTarget, new: &str) -> Result<Renamed, ProjectError> {
        let plan =
            plan_rename(&self.project, &self.root, target, new).map_err(ProjectError::Refused)?;
        // 書く前に、すべてを確かめる（途中で断って、一部だけ書かないため）。
        let read_only = |path: &Path| fs::metadata(path).is_ok_and(|m| m.permissions().readonly());
        for change in &plan.story {
            let path = self.root.join(&change.file);
            let same = match read_disk(&path) {
                Disk::Text(text) => self.disk_text(&change.file) == Some(text.as_str()),
                Disk::Missing | Disk::Unreadable => false,
            };
            if self.pending.contains_key(&change.file) || !same {
                return Err(error(
                    &path,
                    io::ErrorKind::InvalidData,
                    "外で変わっています。取り込んでから名前を変えてください",
                ));
            }
            if read_only(&path) {
                return Err(error(
                    &path,
                    io::ErrorKind::PermissionDenied,
                    "読み取り専用のファイルです",
                ));
            }
        }
        let moved_to: BTreeSet<&str> = plan.moves.iter().map(|m| m.to.as_str()).collect();
        for m in &plan.moves {
            let (from, to) = (self.root.join(&m.from), self.root.join(&m.to));
            if !from.exists() || to.exists() {
                return Err(error(
                    &to,
                    io::ErrorKind::AlreadyExists,
                    "ファイルの名前を変えられません",
                ));
            }
            if read_only(&from) {
                return Err(error(
                    &from,
                    io::ErrorKind::PermissionDenied,
                    "読み取り専用のファイルです",
                ));
            }
        }
        for change in &plan.settings {
            let path = self.root.join(&change.file);
            if !moved_to.contains(change.file.as_str()) && read_only(&path) {
                return Err(error(
                    &path,
                    io::ErrorKind::PermissionDenied,
                    "読み取り専用のファイルです",
                ));
            }
        }
        let mut done = Renamed {
            story: Vec::new(),
            settings: Vec::new(),
            story_edits: plan.story_edits,
            notes: plan.notes.clone(),
            problem: None,
            provenance: Vec::new(),
        };
        let result = self.write_rename(&plan, &mut done);
        if !plan.settings.is_empty() || !plan.moves.is_empty() {
            let _ = self.reload_settings();
        }
        if let Err(e) = result {
            done.problem = Some(e.to_string());
        }
        Ok(done)
    }

    /// 名前の変更を書く（本文、ファイル名、設定、フロー図の位置の順）。書いたものを `done` に入れる。
    fn write_rename(
        &mut self,
        plan: &crate::rename::RenamePlan,
        done: &mut Renamed,
    ) -> Result<(), ProjectError> {
        for change in &plan.story {
            // 保存していない変更ごと書くので、名前を置き換える前の本文で、人が直した AI の行を記録する
            // （名前の置き換えそのものは数えない。本文を書けてから記録する。T3-4、D065）。
            let before = self.base.get(&change.file).cloned();
            let current = self.text(&change.file).map(str::to_owned);
            let external = self.disk.get(&change.file).cloned().flatten();
            write_source(&self.root, &change.file, &change.text)?;
            if let (Some(before), Some(current)) = (before, current)
                && before != current
            {
                let (ids, problem) =
                    self.record_edited_ai_lines(&before, external.as_deref(), &current);
                done.provenance.extend(ids);
                if let Some(problem) = problem {
                    done.notes
                        .push(format!("出自の記録を書けませんでした：{problem}"));
                }
            }
            self.set_buffer(&change.file, &change.text);
            self.synced(&change.file, &change.text);
            done.story.push(StoryWritten {
                file: change.file.clone(),
                text: change.text.clone(),
            });
        }
        for m in &plan.moves {
            let (from, to) = (self.root.join(&m.from), self.root.join(&m.to));
            fs::rename(&from, &to).map_err(|source| ProjectError::Write { path: to, source })?;
            self.moved_away.insert(m.from.clone());
            // 項目の出自を新しいパスへ写す（移り先に残っていた古い記録は打ち消す。T4-3）。
            if is_field_file(&m.from) {
                let moved = read_provenance(&self.root).and_then(|provenance| {
                    let mut records = reset_field_records(&provenance, &m.to, &provenance_now());
                    records.extend(moved_field_records(&provenance, &m.from, &m.to));
                    append_provenance(&self.root, &records)
                });
                if let Err(e) = moved {
                    done.notes
                        .push(format!("出自の記録を書けませんでした：{e}"));
                }
            }
        }
        for change in &plan.settings {
            write_source(&self.root, &change.file, &change.text)?;
            self.settings_written
                .insert(change.file.clone(), change.text.clone());
            done.settings.push(change.file.clone());
        }
        if let Some(layout) = &plan.layout
            && let Ok(Some(text)) =
                rename_layout_key(&self.root, &layout.level, &layout.from, &layout.to)
        {
            self.layout_written = Some(text);
        }
        Ok(())
    }

    /// 章のファイル名を変える（`story/` の中で）。保存していない変更はそのまま持ち越す。`layout.json` の章の
    /// 位置も移す。変えた後のパスを返す。
    pub fn rename_chapter(&mut self, file: &str, name: &str) -> Result<String, ProjectError> {
        let path = self.root.join(file);
        if self.file(file).is_none() {
            return Err(not_open(&path));
        }
        if self.pending.contains_key(file) {
            return Err(error(
                &path,
                io::ErrorKind::InvalidData,
                "外で変わっています。取り込んでから名前を変えてください",
            ));
        }
        // 同じフォルダの中で名前を変える。
        let name = chapter_name(name)
            .filter(|n| !n.contains('/'))
            .ok_or_else(|| {
                error(
                    &path,
                    io::ErrorKind::InvalidInput,
                    "章のファイル名は、フォルダを含まない名前で書きます",
                )
            })?;
        let dir = file.rsplit_once('/').map_or("story", |(dir, _)| dir);
        let to = format!("{dir}/{name}");
        if to == file {
            return Ok(to);
        }
        let to_path = self.root.join(&to);
        if self.file(&to).is_some() || to_path.exists() {
            return Err(error(
                &to_path,
                io::ErrorKind::AlreadyExists,
                "同じ名前の章があります",
            ));
        }
        fs::rename(&path, &to_path).map_err(|source| ProjectError::Write {
            path: to_path.clone(),
            source,
        })?;
        if let Some(source) = self.project.files.iter_mut().find(|f| f.path == file) {
            to.clone_into(&mut source.path);
        }
        self.project.files.sort_by(|a, b| a.path.cmp(&b.path));
        if let Some(disk) = self.disk.remove(file) {
            self.disk.insert(to.clone(), disk);
        }
        if let Some(base) = self.base.remove(file) {
            self.base.insert(to.clone(), base);
        }
        if let Ok(Some(text)) = rename_layout_key(&self.root, "chapter", file, &to) {
            self.layout_written = Some(text);
        }
        Ok(to)
    }

    /// 章を消す（ファイルを消し、閉じる）。開いている章だけ。
    pub fn delete_chapter(&mut self, file: &str) -> Result<(), ProjectError> {
        let path = self.root.join(file);
        if self.file(file).is_none() {
            return Err(not_open(&path));
        }
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(ProjectError::Write { path, source }),
        }
        self.remove_file(file);
        self.removed.remove(file);
        Ok(())
    }

    // ---- 外部の変更とのマージ（T2-7、D058） ----

    /// マージの元・自分・外。自分は今の本文。外は、選ぶ前の外の中身があればそれ、無ければディスクの中身。元は、
    /// アプリが渡したもの（知らせの途中の競合で、アプリが読み込み直す前の中身を覚えているとき）か、選ぶ前の外の
    /// 中身があればディスクの中身（読み込んだときの中身）。外で消されたファイルはマージしない。
    fn merge_inputs(
        &self,
        file: &str,
        base: Option<&str>,
    ) -> Result<(String, String, String), ProjectError> {
        let path = self.root.join(file);
        let invalid = |message: &str| error(&path, io::ErrorKind::InvalidInput, message);
        let Some(mine) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        let (theirs, known) = match self.pending.get(file) {
            Some(None) => return Err(invalid("外で消されたファイルは、マージできません")),
            Some(Some(theirs)) => (theirs.clone(), self.disk_text(file).map(str::to_owned)),
            None => match self.disk_text(file) {
                Some(theirs) => (theirs.to_owned(), None),
                None => return Err(invalid("外で消されたファイルは、マージできません")),
            },
        };
        let Some(base) = base.map(str::to_owned).or(known) else {
            return Err(invalid(
                "マージの元が分からないので、マージできません。読み込み直すか、自分の変更を残してください",
            ));
        };
        Ok((base, mine, theirs))
    }

    /// 外部の変更とのマージの結果（3方向。行IDのある行は行IDで対応させる）。画面はぶつかりを並べて選ばせる。
    /// `base` は、アプリが覚えている元（知らせの途中の競合のとき）。
    pub fn merge_view(&self, file: &str, base: Option<&str>) -> Result<Merge, ProjectError> {
        let (base, mine, theirs) = self.merge_inputs(file, base)?;
        Ok(merge_scn(&base, &mine, &theirs))
    }

    /// マージを当てる：ぶつかりごとに選んだ結果を、保存していない変更として開く（ディスクの中身は外の中身。
    /// 「自分の変更を残す」と同じ扱い）。`token` は画面に見せたマージの [`Merge::token`] で、今の入力から作った
    /// マージと違えば（見せた後に本文か外の中身が変わった）、何も変えずに断る。
    pub fn apply_merge(
        &mut self,
        file: &str,
        base: Option<&str>,
        choices: &[Choice],
        token: &str,
    ) -> Result<MergeApplied, ProjectError> {
        // アプリが覚えていた元（知らせの途中の競合）は、人が直し始めた元でもある（出自の記録。T3-4）。
        let given = base.map(str::to_owned);
        let (base, mine, theirs) = self.merge_inputs(file, base)?;
        let merge = merge_scn(&base, &mine, &theirs);
        let refuse = |message: &str| {
            Err(error(
                &self.root.join(file),
                io::ErrorKind::InvalidInput,
                message,
            ))
        };
        if merge.token != token {
            return refuse(
                "マージを開いた後に、本文か外の中身が変わりました。もう一度マージしてください",
            );
        }
        let Some(text) = merge.resolve(choices) else {
            return refuse("選んだ数が、ぶつかりの数と合いません。もう一度マージしてください");
        };
        if let Some(given) = &given {
            self.remember_base(file, given);
        }
        let disk = self.keep_mine(file, &text)?;
        Ok(MergeApplied { text, disk })
    }

    // ---- 企画とアイデアメモ（T2-6、D057） ----

    /// 企画書（`concept.md`）を、ひな形（`scn new` と同じ）で作る。もうあれば作らない。書いた中身を返す。
    pub fn create_concept(&mut self) -> Result<String, ProjectError> {
        let path = self.root.join(CONCEPT_FILE);
        if path.exists() {
            return Err(error(
                &path,
                io::ErrorKind::AlreadyExists,
                "企画書（concept.md）はもうあります",
            ));
        }
        let text = CONCEPT_TEMPLATE.to_owned();
        self.write_settings_file(CONCEPT_FILE, &text)?;
        self.reset_stale_fields(CONCEPT_FILE);
        Ok(text)
    }

    /// アイデアメモを新しく作る：`ideas/idea-NNN.md`（使われていない最初の番号）に、`status: 未整理` と
    /// タグと本文を書く。書いたパスと中身を返す。
    pub fn create_idea(
        &mut self,
        body: &str,
        tags: &[String],
    ) -> Result<(String, String), ProjectError> {
        let dir = self.root.join(IDEAS_DIR);
        if body.trim().is_empty() {
            return Err(error(
                &dir,
                io::ErrorKind::InvalidInput,
                "メモの本文が空です",
            ));
        }
        let tags: Vec<String> = tags
            .iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        let text = new_idea_text(body, &tags)
            .map_err(|e| error(&dir, io::ErrorKind::InvalidInput, &e.to_string()))?;
        fs::create_dir_all(&dir).map_err(|source| ProjectError::Write {
            path: dir.clone(),
            source,
        })?;
        let file = new_idea_file(&self.root);
        self.write_settings_file(&file, &text)?;
        Ok((file, text))
    }

    /// アイデアメモを昇格する：昇格先（設定DBのエントリ、章の末尾のスタブのシーン）を作り、メモに
    /// `status: 採用` と `promoted_to` を書く（直したキーの行だけを差し替える）。`seen` はアプリが知っている
    /// メモの中身で、今の中身と違えば何も作らない。昇格先を作れないとき（使われているID、保存していない変更の
    /// ある章など）はエラーで、メモも変えない。
    pub fn promote_idea(
        &mut self,
        file: &str,
        seen: &str,
        promotion: &Promotion,
    ) -> Result<Promoted, ProjectError> {
        let path = self.root.join(file);
        if !is_idea_path(file) {
            return Err(error(
                &path,
                io::ErrorKind::InvalidInput,
                "アイデアメモ（ideas/*.md）のパスではありません",
            ));
        }
        let current = self.read_settings_file(&path)?;
        if current != seen {
            return Ok(Promoted::Conflict { text: current });
        }
        let target = promotion.promoted_to();
        let text = match set_field(&current, "status", &serde_json::Value::from(ADOPTED))
            .and_then(|t| set_field(&t, "promoted_to", &serde_json::Value::from(target.clone())))
        {
            Ok(text) => text,
            Err(e) => {
                return Ok(Promoted::Unsupported {
                    message: e.to_string(),
                });
            }
        };
        // メモに書けなければ（読み取り専用）、昇格先を作る前に断る。
        if fs::metadata(&path).is_ok_and(|m| m.permissions().readonly()) {
            return Err(error(
                &path,
                io::ErrorKind::PermissionDenied,
                "メモが読み取り専用なので、昇格できません",
            ));
        }
        // 本文の改行は、作るファイルの改行（`\n`）にそろえる。
        let body = body_of(&current).replace("\r\n", "\n");
        let scene = match promotion {
            Promotion::Entry { type_id, id, name } => {
                // 伏線の型の定義が無ければ標準の定義を作る。ファイルはあるのに読めなければ（YAML の誤りなど）、
                // 人の書いた定義を上書きしないように断る。
                let type_file = self
                    .root
                    .join(format!("bible/types/{FORESHADOW_TYPE}.yaml"));
                let defined = self.project.bible.types.iter().any(|t| t.id == *type_id);
                if type_id == FORESHADOW_TYPE && !defined && type_file.exists() {
                    return Err(error(
                        &type_file,
                        io::ErrorKind::InvalidData,
                        "伏線の型の定義を読めません。直してから昇格してください",
                    ));
                }
                let type_missing = type_id == FORESHADOW_TYPE && !defined;
                self.check_new_entry(type_id, id, name, type_missing)?;
                if type_missing {
                    let types = self.root.join("bible/types");
                    fs::create_dir_all(&types).map_err(|source| ProjectError::Write {
                        path: types,
                        source,
                    })?;
                    self.write_settings_file(
                        &format!("bible/types/{FORESHADOW_TYPE}.yaml"),
                        FORESHADOW_TYPE_TEXT,
                    )?;
                }
                self.create_entry_with_body(type_id, id, name, &body)?;
                None
            }
            Promotion::Scene {
                file: chapter,
                id,
                title,
            } => {
                let title = self.check_new_scene(chapter, id, title)?;
                let text =
                    self.append_scene(chapter, id, &title, summary_line(&body).as_deref())?;
                Some(StoryWritten {
                    file: chapter.clone(),
                    text,
                })
            }
        };
        // 昇格先はもう作ったので、メモを書けなくてもエラーにはせず、作ったものを返す（アプリの画面とコアの
        // 中身を食い違わせない）。
        if let Err(e) = self.write_settings_file(file, &text) {
            return Ok(Promoted::MemoUnwritten {
                target,
                scene,
                message: e.to_string(),
            });
        }
        Ok(Promoted::Saved {
            text,
            target,
            scene,
        })
    }

    /// スタブのシーンを足せるかを確かめ、題名（前後の空白を除いたもの）を返す。章は開いている本文のファイルで、
    /// 保存していない変更も、まだ取り込んでいない外部の変更も無いこと。シーンIDは形が正しく、使われていないこと。
    fn check_new_scene(&self, file: &str, id: &str, title: &str) -> Result<String, ProjectError> {
        let path = self.root.join(file);
        let invalid = |message: &str| error(&path, io::ErrorKind::InvalidInput, message);
        if self.file(file).is_none() {
            return Err(not_open(&path));
        }
        if self.is_dirty(file) || self.pending.contains_key(file) {
            return Err(invalid(
                "この章には保存していない変更があります。保存してから書き足してください",
            ));
        }
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(invalid("シーンIDは英数字と _ で書きます"));
        }
        if crate::analyze(&self.project)
            .scenes
            .iter()
            .any(|s| s.id == id)
        {
            return Err(invalid(&format!("シーンID {id} はもう使われています")));
        }
        let title = title.trim();
        if title.is_empty() || title.contains(['\n', '\r']) {
            return Err(invalid("題名が空か、改行を含んでいます"));
        }
        Ok(title.to_owned())
    }

    /// 章の末尾にスタブのシーンを足して書く（前の中身は1バイトも変えない）。ディスクの中身が知っている中身と
    /// 違えば（まだ取り込んでいない外部の変更）、書かない。書いた中身を返す。
    fn append_scene(
        &mut self,
        file: &str,
        id: &str,
        title: &str,
        summary: Option<&str>,
    ) -> Result<String, ProjectError> {
        let path = self.root.join(file);
        let Some(current) = self.text(file).map(str::to_owned) else {
            return Err(not_open(&path));
        };
        match read_disk(&path) {
            Disk::Text(on_disk) if self.disk_text(file) == Some(on_disk.as_str()) => {}
            _ => {
                return Err(error(
                    &path,
                    io::ErrorKind::InvalidData,
                    "章が外で変わっています。取り込んでから書き足してください",
                ));
            }
        }
        let text = format!("{current}{}", stub_scene(&current, title, id, summary));
        write_source(&self.root, file, &text)?;
        self.set_buffer(file, &text);
        self.synced(file, &text);
        Ok(text)
    }
}

/// 章の末尾にシーンを足すときの区切り（前の行と空行で分ける。もう空行で終わっていれば足さない）。
fn scene_separator(text: &str, nl: &str) -> String {
    let mut out = String::new();
    if !text.is_empty() && !text.ends_with('\n') {
        out.push_str(nl);
    }
    let blank_end =
        text.trim_end_matches(['\r', '\n']).is_empty() || text.ends_with(&format!("{nl}{nl}"));
    if !blank_end {
        out.push_str(nl);
    }
    out
}

/// 章の本文を `text` にすると、その章の誤り（E）が増えるなら、そのわけ。
fn more_errors(project: &Project, file: &str, text: &str) -> Option<String> {
    let errors = |p: &Project| {
        crate::analyze(p)
            .diagnostics
            .into_iter()
            .filter(|d| d.file == file && d.code.starts_with('E'))
            .count()
    };
    let before = errors(project);
    let mut trial = project.clone();
    if let Some(f) = trial.files.iter_mut().find(|f| f.path == file) {
        f.text = text.to_owned();
    }
    (errors(&trial) > before).then(|| "書き足すと章の誤り（E）が増えるので、書きません".to_owned())
}
