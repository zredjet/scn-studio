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

use crate::line_id::{AssignedLineId, LineIdGenerator, assign_line_ids};
use crate::project::{Project, ProjectError, SourceFile, load_project, write_source};

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
    /// 設定ファイル（`project.toml`、変数、素材カタログ、設定DB、企画書）が変わったので、設定を読み直した。
    Settings,
}

/// 書いた本文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// 書いた本文（付けた行IDを含む）。
    pub text: String,
    /// 保存のときに付けた行ID（D045）。
    pub assigned: Vec<AssignedLineId>,
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
        Ok(Self {
            root: root.to_owned(),
            canonical: fs::canonicalize(root).unwrap_or_else(|_| root.to_owned()),
            project,
            disk,
            pending: BTreeMap::new(),
            removed: BTreeSet::new(),
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
        self.disk.insert(file.to_owned(), Some(text.clone()));
        Ok(SaveOutcome::Saved(Saved { text, assigned }))
    }

    /// 作品フォルダの根からの相対パス（区切りは `/`）。作品フォルダの外なら `None`。
    fn relative(&self, path: &Path) -> Option<String> {
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
            ["story", name] if name.ends_with(".scn") => self.story_change(&rel),
            _ if SETTINGS_FILES.contains(&rel.as_str()) || parts.first() == Some(&"bible") => {
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
                self.disk.insert(file.to_owned(), Some(text.clone()));
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
        self.disk.insert(file.to_owned(), Some(text.to_owned()));
        self.removed.remove(file);
    }

    /// 外で消されたファイルを閉じる。
    fn remove_file(&mut self, file: &str) {
        self.project.files.retain(|f| f.path != file);
        self.disk.remove(file);
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
                self.disk.insert(file.to_owned(), Some(text.clone()));
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

    /// 未登録の話者（W202）を、設定DBのキャラクターとして登録する（T1-8、D047）。
    /// `bible/entries/<キャラクターの型>/<id>.md` に、`id` と `name` だけのエントリを書き、設定を読み直す。
    /// `id` は英数字と `_`（変数の名前空間や立ち絵の差分のキーにもなるため）。書いたファイルのパスを返す。
    pub fn register_character(&mut self, id: &str, name: &str) -> Result<String, ProjectError> {
        let dir = format!("bible/entries/{}", self.project.bible.character_type());
        let invalid =
            |message: &str| error(&self.root.join(&dir), io::ErrorKind::InvalidInput, message);
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(invalid("IDは英数字と _ で書きます"));
        }
        let name = name.trim();
        if name.is_empty() || name.contains(['\n', '\r']) {
            return Err(invalid("名前が空か、改行を含んでいます"));
        }
        let file = format!("{dir}/{id}.md");
        let path = self.root.join(&file);
        let taken = self.project.characters.iter().any(|c| c.id == id);
        if taken || path.exists() {
            return Err(error(
                &path,
                io::ErrorKind::AlreadyExists,
                "同じIDのキャラクターがあります",
            ));
        }
        let folder = self.root.join(&dir);
        fs::create_dir_all(&folder).map_err(|source| ProjectError::Write {
            path: folder,
            source,
        })?;
        // 名前はJSONの文字列にして書く（YAMLとしても読める。`:` などを含んでも崩れない）。
        let quoted = serde_json::to_string(name).unwrap_or_default();
        write_source(
            &self.root,
            &file,
            &format!("---\nid: {id}\nname: {quoted}\n---\n"),
        )?;
        self.reload_settings()?;
        Ok(file)
    }

    /// `story/` に章（空の `.scn`）を足す。`name` に `.scn` が無ければ補う。足したファイルのパスを返す。
    pub fn add_chapter(&mut self, name: &str) -> Result<String, ProjectError> {
        let name = name.trim();
        let name = if name.ends_with(".scn") {
            name.to_owned()
        } else {
            format!("{name}.scn")
        };
        let stem = name.trim_end_matches(".scn");
        if stem.is_empty() || stem.starts_with('.') || name.contains(['/', '\\']) {
            return Err(error(
                &self.root.join("story"),
                io::ErrorKind::InvalidInput,
                "章のファイル名は、フォルダを含まない名前で書きます",
            ));
        }
        let file = format!("story/{name}");
        let path = self.root.join(&file);
        if self.file(&file).is_some() || path.exists() {
            return Err(error(
                &path,
                io::ErrorKind::AlreadyExists,
                "同じ名前の章があります",
            ));
        }
        let story = self.root.join("story");
        fs::create_dir_all(&story).map_err(|source| ProjectError::Write {
            path: story,
            source,
        })?;
        write_source(&self.root, &file, "")?;
        self.insert_file(&file, "");
        Ok(file)
    }
}
