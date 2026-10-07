//! 開発用CLI `scn`（D012）。
//!
//! - `scn parse <file.scn> [--json]`：1つのファイルを解析し、診断を出す。ファイルがプロジェクトの中
//!   （上のディレクトリに `project.toml` がある）なら、プロジェクト全体で意味解析を行い、
//!   そのファイルの診断だけを出す。プロジェクトの外なら、構文解析だけを行う。
//! - `scn ir <project-dir>`：プロジェクト全体のIRを、標準出力にJSONで出す。
//! - `scn fmt <project-dir> [--check] [--json]`：行IDの無い行に行IDを付ける（仕様書9節、D041）。
//!   付けた行IDを `ファイル:行:列 ^ID` の形で出す。`--check` では書き換えず、付ける行があれば1で終わる。
//!
//! 終了コードは、成功が0、エラー（E）の診断があれば1、使い方や入出力の誤りは2。
//! `fmt` は本文の診断では1にしない（`--check` で行IDの無い行があるときだけ1）。

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
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
    Read { path: PathBuf, source: io::Error },
    Project(ProjectError),
    Write(io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Read { path, source } => {
                write!(f, "{} を読めません：{source}", path.display())
            }
            CliError::Project(e) => write!(f, "{e}"),
            CliError::Write(e) => write!(f, "出力に書けません：{e}"),
        }
    }
}

/// 終了コード。
const SUCCESS: u8 = 0;
const HAS_ERRORS: u8 = 1;
const FAILURE: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Parse { file, json } => run_parse(&file, json),
        Command::Ir { project } => run_ir(&project),
        Command::Fmt {
            project,
            check,
            json,
        } => run_fmt(&project, check, json),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("scn: {e}");
            ExitCode::from(FAILURE)
        }
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
}

fn run_parse(file: &Path, json: bool) -> Result<u8, CliError> {
    let text = fs::read_to_string(file).map_err(|source| CliError::Read {
        path: file.to_owned(),
        source,
    })?;
    let (text, diagnostics) = match find_project_root(file) {
        Some(root) => analyze_in_project(&root, file, text)?,
        None => {
            let diagnostics = parse_alone(&text);
            (text, diagnostics)
        }
    };

    let index = LineIndex::new(&text);
    let shown = file.display().to_string();
    let code = exit_code(&diagnostics);
    let mut out = io::stdout().lock();
    if json {
        let list: Vec<CliDiagnostic<'_>> = diagnostics
            .iter()
            .map(|d| {
                let (line, column) = index.line_col(d.span.start);
                let (end_line, end_column) = index.line_col(d.span.end);
                CliDiagnostic {
                    code: d.code,
                    severity: d.severity,
                    file: shown.clone(),
                    line,
                    column,
                    end_line,
                    end_column,
                    span: d.span,
                    message: &d.message,
                }
            })
            .collect();
        let body = serde_json::to_string_pretty(&list).map_err(|e| CliError::Write(e.into()))?;
        finish(writeln!(out, "{body}"), code)
    } else {
        let written = diagnostics.iter().try_for_each(|d| {
            let (line, column) = index.line_col(d.span.start);
            writeln!(out, "{shown}:{line}:{column} {} {}", d.code, d.message)
        });
        let count = |severity: Severity| {
            diagnostics
                .iter()
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

/// プロジェクトの中：プロジェクト全体で解析し、そのファイルの診断だけを返す。
/// ファイルが `story/*.scn` でなくても、プロジェクトの本文に加えて解析する。
fn analyze_in_project(
    root: &Path,
    file: &Path,
    text: String,
) -> Result<(String, Vec<IrDiagnostic>), CliError> {
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
    let text = match project.files.iter_mut().find(|f| f.path == relative) {
        Some(existing) => {
            existing.text.clone_from(&text);
            text
        }
        None => {
            project.files.push(SourceFile {
                path: relative.clone(),
                text: text.clone(),
            });
            project.files.sort_by(|a, b| a.path.cmp(&b.path));
            text
        }
    };
    let ir = analyze(&project);
    let diagnostics = ir
        .diagnostics
        .into_iter()
        .filter(|d| d.file == relative)
        .collect();
    Ok((text, diagnostics))
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
