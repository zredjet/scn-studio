//! 開発用CLI `scn`（D012）。
//!
//! - `scn parse <file.scn> [--json]`：1つのファイルを解析し、診断を出す。ファイルがプロジェクトの中
//!   （上のディレクトリに `project.toml` がある）なら、プロジェクト全体で意味解析を行い、
//!   設定ファイルの診断（E801〜W804）と、そのファイルの診断を出す（ほかの章の診断は出さない）。
//!   プロジェクトの外なら、構文解析だけを行う。
//! - `scn ir <project-dir>`：プロジェクト全体のIRを、標準出力にJSONで出す。
//! - `scn check [project-dir] [--json]`：プロジェクト全体を検証し（読み込み・構文解析・意味解析の診断と、
//!   検証の段階1。D043）、診断を `ファイル:行:列 コード メッセージ ^行ID` の形で出す。
//! - `scn fmt <project-dir> [--check] [--json]`：行IDの無い行に行IDを付ける（仕様書9節、D041）。
//!   付けた行IDを `ファイル:行:列 ^ID` の形で出す。`--check` では書き換えず、付ける行があれば1で終わる。
//!   `project.toml` にエラーがあれば、台詞の括弧を書かれたとおりに読めないので、何もせずに2で終わる。
//!
//! 終了コードは、成功が0、エラー（E）の診断があれば1、使い方の誤り・`project.toml` の無いフォルダ・
//! 入出力の誤りは2。`fmt` は本文の診断では1にしない（`--check` で行IDの無い行があるときだけ1）。
//!
//! 入れ子の深い本文でもスタックがあふれないよう、コマンドは大きなスタックのスレッドで実行する
//! （Windowsのメインスレッドは1MB。アプリと同じ64MB）。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;

use clap::{Parser, Subcommand};
use scn_core::check::check;
use scn_core::index::build_index;
use scn_core::ir::IrDiagnostic;
use scn_core::line_id::{LineIdGenerator, assign_line_ids};
use scn_core::project::SourceFile;
use scn_core::{
    LineIndex, ParseOptions, ProjectError, Severity, analyze, load_project, parse, write_source,
};
use serde::Serialize;

#[derive(Parser)]
#[command(name = "scn", version, about = ".scn の開発用CLI（フェーズ0）")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 1つの .scn ファイルを解析し、診断を `ファイル:行:列 コード メッセージ` の形で出す
    Parse {
        /// 解析する .scn ファイル
        file: PathBuf,
        /// 診断をJSONで出す
        #[arg(long)]
        json: bool,
    },
    /// プロジェクト全体のIRを、標準出力にJSONで出す
    Ir {
        /// プロジェクトのフォルダ（project.toml のあるところ）
        project: PathBuf,
    },
    /// プロジェクト全体を検証し、診断を `ファイル:行:列 コード メッセージ ^行ID` の形で出す
    Check {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 診断をJSONで出す
        #[arg(long)]
        json: bool,
    },
    /// 行IDの無い行に行IDを付け、付けた行IDを `ファイル:行:列 ^ID` の形で出す。整形はしない
    Fmt {
        /// プロジェクトのフォルダ（project.toml のあるところ）
        project: PathBuf,
        /// 書き換えずに、行IDの無い行を出す。あれば終了コード1
        #[arg(long)]
        check: bool,
        /// JSONで出す
        #[arg(long)]
        json: bool,
    },
}

/// 使い方や入出力の誤り（終了コード2）。
enum CliError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Project(ProjectError),
    Write(io::Error),
    /// 安全に実行できない（`project.toml` のエラーのときの `fmt`）。
    Refused(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Read { path, source } => {
                write!(f, "{} を読めません：{source}", path.display())
            }
            CliError::Project(e) => write!(f, "{e}"),
            CliError::Write(e) => write!(f, "出力に書けません：{e}"),
            CliError::Refused(message) => write!(f, "{message}"),
        }
    }
}

/// 終了コード。
const SUCCESS: u8 = 0;
const HAS_ERRORS: u8 = 1;
const FAILURE: u8 = 2;

/// コマンドを実行するスレッドのスタックの大きさ（アプリの解析のスレッドと同じ）。
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = thread::Builder::new()
        .name("scn".into())
        .stack_size(STACK_SIZE)
        .spawn(move || run(cli.command))
        .map_err(|e| CliError::Refused(format!("実行のスレッドを作れません：{e}")))
        .and_then(|worker| {
            worker
                .join()
                .unwrap_or_else(|_| Err(CliError::Refused("実行が途中で止まりました".into())))
        });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("scn: {e}");
            ExitCode::from(FAILURE)
        }
    }
}

fn run(command: Command) -> Result<u8, CliError> {
    match command {
        Command::Parse { file, json } => run_parse(&file, json),
        Command::Ir { project } => run_ir(&project),
        Command::Check { project, json } => run_check(&project, json),
        Command::Fmt {
            project,
            check,
            json,
        } => run_fmt(&project, check, json),
    }
}

fn exit_code(diagnostics: &[IrDiagnostic]) -> u8 {
    let has_errors = diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error.as_str());
    if has_errors { HAS_ERRORS } else { SUCCESS }
}

/// 書き出しの結果を、終了コードにする。パイプの先が閉じた（`scn ir … | head` など）ときは
/// 黙って書くのをやめ、終了コードは診断に従う（D037）。
fn finish(written: io::Result<()>, code: u8) -> Result<u8, CliError> {
    match written {
        Ok(()) => Ok(code),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(code),
        Err(e) => Err(CliError::Write(e)),
    }
}

// ---- scn parse ----

/// JSONで出す診断。IRの診断に、行と列（1から数える。列は文字の数）を足したもの。
#[derive(Serialize)]
struct CliDiagnostic<'a> {
    code: &'a str,
    severity: &'a str,
    file: String,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    span: scn_core::Span,
    message: &'a str,
    /// 診断の行の行ID（`scn check` だけ。行IDが無ければ出さない）。
    #[serde(skip_serializing_if = "Option::is_none")]
    line_id: Option<&'a str>,
}

/// 診断を出すファイルと、位置（行と列）を数える本文。
struct Shown {
    /// 出力するパス。
    file: String,
    text: String,
    diagnostics: Vec<IrDiagnostic>,
}

fn run_parse(file: &Path, json: bool) -> Result<u8, CliError> {
    let text = fs::read_to_string(file).map_err(|source| CliError::Read {
        path: file.to_owned(),
        source,
    })?;
    let shown = file.display().to_string();
    let groups = match find_project_root(file) {
        Some(root) => analyze_in_project(&root, file, text)?,
        None => {
            let diagnostics = parse_alone(&text);
            vec![Shown {
                file: shown.clone(),
                text,
                diagnostics,
            }]
        }
    };

    let all: Vec<&IrDiagnostic> = groups.iter().flat_map(|g| &g.diagnostics).collect();
    let code = if all.iter().any(|d| d.severity == Severity::Error.as_str()) {
        HAS_ERRORS
    } else {
        SUCCESS
    };
    let mut out = io::stdout().lock();
    if json {
        let mut list: Vec<CliDiagnostic<'_>> = Vec::new();
        for group in &groups {
            let index = LineIndex::new(&group.text);
            for d in &group.diagnostics {
                let (line, column) = index.line_col(d.span.start);
                let (end_line, end_column) = index.line_col(d.span.end);
                list.push(CliDiagnostic {
                    code: d.code,
                    severity: d.severity,
                    file: group.file.clone(),
                    line,
                    column,
                    end_line,
                    end_column,
                    span: d.span,
                    message: &d.message,
                    line_id: None,
                });
            }
        }
        let body = serde_json::to_string_pretty(&list).map_err(|e| CliError::Write(e.into()))?;
        finish(writeln!(out, "{body}"), code)
    } else {
        let written = groups.iter().try_for_each(|group| {
            let index = LineIndex::new(&group.text);
            group.diagnostics.iter().try_for_each(|d| {
                let (line, column) = index.line_col(d.span.start);
                writeln!(
                    out,
                    "{}:{line}:{column} {} {}",
                    group.file, d.code, d.message
                )
            })
        });
        let count = |severity: Severity| {
            all.iter()
                .filter(|d| d.severity == severity.as_str())
                .count()
        };
        eprintln!(
            "{shown}：エラー {}件、警告 {}件、情報 {}件",
            count(Severity::Error),
            count(Severity::Warning),
            count(Severity::Info)
        );
        finish(written, code)
    }
}

/// ファイルのあるディレクトリから上へ、`project.toml` のあるディレクトリを探す。
fn find_project_root(file: &Path) -> Option<PathBuf> {
    let file = fs::canonicalize(file).ok()?;
    file.ancestors()
        .skip(1)
        .find(|dir| dir.join("project.toml").is_file())
        .map(Path::to_path_buf)
}

/// プロジェクトの外：構文解析だけを行う（括弧は既定の「」だけ）。
fn parse_alone(text: &str) -> Vec<IrDiagnostic> {
    parse(text, &ParseOptions::default())
        .diagnostics
        .into_iter()
        .map(|d| IrDiagnostic {
            code: d.code.as_str(),
            severity: d.severity().as_str(),
            file: String::new(),
            span: d.span,
            message: d.message,
        })
        .collect()
}

/// プロジェクトの中：プロジェクト全体で解析し、設定ファイルの読み込みの診断（E801〜W804。どの本文の
/// 解析にも響くため）と、そのファイルの診断を返す。ファイルが `story/*.scn` でなくても、
/// プロジェクトの本文に加えて解析する。
fn analyze_in_project(root: &Path, file: &Path, text: String) -> Result<Vec<Shown>, CliError> {
    let mut project = load_project(root).map_err(CliError::Project)?;
    let relative = fs::canonicalize(file)
        .ok()
        .and_then(|f| f.strip_prefix(root).ok().map(Path::to_path_buf))
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    // 読み込んだあとで中身が変わっていても、表示と解析に同じ中身を使う。
    match project.files.iter_mut().find(|f| f.path == relative) {
        Some(existing) => existing.text.clone_from(&text),
        None => {
            project.files.push(SourceFile {
                path: relative.clone(),
                text: text.clone(),
            });
            project.files.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }
    let loaded = project.load_diagnostics.len();
    let ir = analyze(&project);
    let shown_root = typed_root(file, &relative).unwrap_or_else(|| root.to_path_buf());

    // IRの診断は、読み込みの診断が先に並ぶ（`analyze`）。
    let mut groups: Vec<Shown> = Vec::new();
    let mut own = Vec::new();
    for (i, d) in ir.diagnostics.into_iter().enumerate() {
        if i < loaded && (d.file == relative || !d.file.starts_with("story/")) {
            match groups.iter_mut().find(|g| g.file == d.file) {
                Some(group) => group.diagnostics.push(d),
                None => groups.push(Shown {
                    file: d.file.clone(),
                    text: fs::read(root.join(&d.file))
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_default(),
                    diagnostics: vec![d],
                }),
            }
            continue;
        }
        if i >= loaded && d.file == relative {
            own.push(d);
        }
    }
    for group in &mut groups {
        group.file = shown_root.join(&group.file).display().to_string();
    }
    groups.push(Shown {
        file: file.display().to_string(),
        text,
        diagnostics: own,
    });
    Ok(groups)
}

/// 引数のパスから、プロジェクトの根を、引数と同じ書き方で求める
/// （`samples/houkago/story/a.scn` と `story/a.scn` なら `samples/houkago`）。求められなければ `None`。
fn typed_root(file: &Path, relative: &str) -> Option<PathBuf> {
    let mut root = file.to_path_buf();
    for part in relative.rsplit('/') {
        if root.file_name()? != part {
            return None;
        }
        root.pop();
    }
    Some(root)
}

// ---- scn ir ----

fn run_ir(dir: &Path) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let body = serde_json::to_string_pretty(&ir).map_err(|e| CliError::Write(e.into()))?;
    finish(
        writeln!(io::stdout().lock(), "{body}"),
        exit_code(&ir.diagnostics),
    )
}

// ---- scn fmt ----

/// 行IDを付けた（`--check` では、付ける）行。行と列は1から数える（列は文字の数）。
#[derive(Serialize)]
struct FmtEntry {
    /// 引数のフォルダにプロジェクトの根からの相対パスをつなげたもの（`.` なら相対パスだけ）。
    file: String,
    line: usize,
    column: usize,
    /// 付けた行ID。`--check` では無い。
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

/// 付けた行IDは、ファイルを1つ書くたびに、その分を出す。途中のファイルで書き込みに失敗しても、
/// 書き終えたファイルの行IDは出してから終了コード2で終える（AIの出自の記録に使うため。D041）。
fn run_fmt(dir: &Path, check: bool, json: bool) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    // 台詞の括弧が既定に戻っていると、複数行の台詞の途中の行にも行IDを付けてしまう（D042）。
    if project.has_project_toml_errors() {
        let text = fs::read(dir.join("project.toml"))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let index = LineIndex::new(&text);
        let shown = display_path(dir, "project.toml");
        for d in project
            .load_diagnostics
            .iter()
            .filter(|d| d.file == "project.toml")
        {
            let (line, column) = index.line_col(d.diag.span.start);
            eprintln!(
                "{shown}:{line}:{column} {} {}",
                d.diag.code.as_str(),
                d.diag.message
            );
        }
        return Err(CliError::Refused(
            "project.toml にエラーがあるので、行IDを付けません（台詞の括弧を書かれたとおりに読めないため）".into(),
        ));
    }
    let results = assign_line_ids(&project, &mut LineIdGenerator::new());
    let mut out = io::stdout().lock();
    let mut done: Vec<FmtEntry> = Vec::new();
    let mut failure = None;
    for result in &results {
        if !check && let Err(e) = write_source(dir, &result.path, &result.text) {
            failure = Some(CliError::Project(e));
            break;
        }
        let original = project
            .files
            .iter()
            .find(|f| f.path == result.path)
            .map(|f| f.text.as_str())
            .unwrap_or_default();
        let index = LineIndex::new(original);
        let shown = display_path(dir, &result.path);
        let entries: Vec<FmtEntry> = result
            .assigned
            .iter()
            .map(|assigned| {
                let (line, column) = index.line_col(assigned.offset);
                FmtEntry {
                    file: shown.clone(),
                    line,
                    column,
                    id: (!check).then(|| assigned.id.clone()),
                }
            })
            .collect();
        if !json {
            let written = entries.iter().try_for_each(|e| match &e.id {
                Some(id) => writeln!(out, "{}:{}:{} ^{id}", e.file, e.line, e.column),
                None => writeln!(out, "{}:{}:{} 行IDがありません", e.file, e.line, e.column),
            });
            if let Err(e) = written
                && e.kind() != io::ErrorKind::BrokenPipe
            {
                return Err(CliError::Write(e));
            }
        }
        done.extend(entries);
    }

    if json {
        let body = serde_json::to_string_pretty(&done).map_err(|e| CliError::Write(e.into()))?;
        let written = writeln!(out, "{body}");
        if let Err(e) = written
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(CliError::Write(e));
        }
    }
    if let Some(e) = failure {
        if !done.is_empty() {
            eprintln!(
                "scn: 書き終えたファイルには行IDを付けました：{}件",
                done.len()
            );
        }
        return Err(e);
    }
    if done.is_empty() {
        eprintln!("行IDの無い行はありません");
    } else if check {
        eprintln!("行IDの無い行：{}件", done.len());
    } else {
        eprintln!(
            "行IDを付けました：{}件（{}ファイル）",
            done.len(),
            results.len()
        );
    }
    Ok(if check && !done.is_empty() {
        HAS_ERRORS
    } else {
        SUCCESS
    })
}

/// 出力するパス。引数のフォルダに、プロジェクトの根からの相対パスをつなげる（`.` なら相対パスだけ）。
fn display_path(dir: &Path, relative: &str) -> String {
    if dir == Path::new(".") {
        relative.to_owned()
    } else {
        dir.join(relative).display().to_string()
    }
}

// ---- scn check ----

/// プロジェクト全体を検証する。読み込みの診断を先に、続けて本文のファイル（プロジェクトの順）、
/// ほかのファイル（パスの順）の診断を、位置の順に出す。
fn run_check(dir: &Path, json: bool) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let loaded = project.load_diagnostics.len();
    let mut rest: Vec<IrDiagnostic> = ir.diagnostics.iter().skip(loaded).cloned().collect();
    rest.extend(check(&project, &ir, &index));
    let rank = |file: &str| {
        project
            .files
            .iter()
            .position(|f| f.path == file)
            .map_or((1, usize::MAX), |i| (0, i))
    };
    rest.sort_by(|a, b| {
        (rank(&a.file), &a.file, a.span.start).cmp(&(rank(&b.file), &b.file, b.span.start))
    });
    let diagnostics: Vec<IrDiagnostic> = ir
        .diagnostics
        .iter()
        .take(loaded)
        .cloned()
        .chain(rest)
        .collect();

    // 位置を数える本文。本文のファイルは読み込んだもの、ほかはディスクから読む。
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for d in &diagnostics {
        if !texts.contains_key(&d.file) {
            let text = match project.files.iter().find(|f| f.path == d.file) {
                Some(f) => f.text.clone(),
                None => fs::read(dir.join(&d.file))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            };
            texts.insert(d.file.clone(), text);
        }
    }
    // 行と列は、ファイルごとに1回だけ作った `LineIndex` で数える。
    let line_indexes: BTreeMap<&str, LineIndex<'_>> = texts
        .iter()
        .map(|(file, text)| (file.as_str(), LineIndex::new(text)))
        .collect();
    let line_col = |d: &IrDiagnostic, offset: usize| {
        line_indexes
            .get(d.file.as_str())
            .map_or((1, 1), |index| index.line_col(offset))
    };
    // 行IDは、ファイルごとに位置の順に並べて、二分探索で引く（行IDの付く行は重ならない）。
    let mut line_ids: BTreeMap<&str, Vec<(usize, usize, &str)>> = BTreeMap::new();
    for l in &index.line_ids {
        line_ids.entry(l.location.file.as_str()).or_default().push((
            l.location.span.start,
            l.location.span.end,
            l.id.as_str(),
        ));
    }
    for list in line_ids.values_mut() {
        list.sort_unstable();
    }
    let line_id = |d: &IrDiagnostic| {
        let list = line_ids.get(d.file.as_str())?;
        let at = list.partition_point(|(start, _, _)| *start <= d.span.start);
        let (start, end, id) = list.get(at.checked_sub(1)?)?;
        (*start <= d.span.start && d.span.start <= *end).then_some(*id)
    };

    let code = exit_code(&diagnostics);
    let mut out = io::stdout().lock();
    if json {
        let list: Vec<CliDiagnostic<'_>> = diagnostics
            .iter()
            .map(|d| {
                let (line, column) = line_col(d, d.span.start);
                let (end_line, end_column) = line_col(d, d.span.end);
                CliDiagnostic {
                    code: d.code,
                    severity: d.severity,
                    file: display_path(dir, &d.file),
                    line,
                    column,
                    end_line,
                    end_column,
                    span: d.span,
                    message: &d.message,
                    line_id: line_id(d),
                }
            })
            .collect();
        let body = serde_json::to_string_pretty(&list).map_err(|e| CliError::Write(e.into()))?;
        return finish(writeln!(out, "{body}"), code);
    }
    let written = diagnostics.iter().try_for_each(|d| {
        let (line, column) = line_col(d, d.span.start);
        let shown = display_path(dir, &d.file);
        match line_id(d) {
            Some(id) => writeln!(
                out,
                "{shown}:{line}:{column} {} {} ^{id}",
                d.code, d.message
            ),
            None => writeln!(out, "{shown}:{line}:{column} {} {}", d.code, d.message),
        }
    });
    let count = |severity: Severity| {
        diagnostics
            .iter()
            .filter(|d| d.severity == severity.as_str())
            .count()
    };
    eprintln!(
        "エラー {}件、警告 {}件、情報 {}件",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info)
    );
    finish(written, code)
}
