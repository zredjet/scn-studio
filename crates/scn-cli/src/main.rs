//! 開発用CLI `scn`（D012）。
//!
//! - `scn parse <file.scn> [--json]`：1つのファイルを解析し、診断を出す。ファイルがプロジェクトの中
//!   （上のディレクトリに `project.toml` がある）なら、プロジェクト全体で意味解析を行い、
//!   設定ファイルの診断（E801〜W804）と、そのファイルの診断を出す（ほかの章の診断は出さない）。
//!   プロジェクトの外なら、構文解析だけを行う。
//! - `scn ir <project-dir> [--compact]`：プロジェクト全体のIRを、標準出力にJSONで出す（`--compact` は整形しない）。
//! - `scn check [project-dir] [--stage 2] [--json]`：プロジェクト全体を検証し（読み込み・構文解析・意味解析の診断と、
//!   検証の段階1。D043。`--stage 2` で自動巡回も。D063）、診断を `ファイル:行:列 コード メッセージ ^行ID` の形で出す。
//! - `scn graph [project-dir] [--level scene|chapter|route] [--json]`：シーン、辺、条件、変数の読み書きを、
//!   粒度ごとに一覧で出す（D044）。
//! - `scn new <dir> [--title 作品名]`：新しい作品フォルダを作る（D044）。
//! - `scn format-doc [project-dir] [--check]`：`FORMAT.md` と `SCN.md`（コマンドの使い方。T4-8、D082）を、今の
//!   ツールの版で書き直す（D044）。
//! - `scn export script [project-dir] [--format csv|xlsx] [--baseline] [-o file]`：台本を CSV か Excel 形式で
//!   書き出す（D051、D061）。収録の基準（`export/script-baseline.json`）があれば、変わった台詞を再収録候補にする。
//! - `scn export renpy [project-dir] [-o dir]`：Ren'Py のスクリプトを書き出す（D053）。
//! - `scn export tyrano [project-dir] [-o dir]`：ティラノスクリプトのシナリオを書き出す（D072）。
//! - `scn export utage [project-dir] [-o dir]`：宴のシナリオと設定のシート（Excel 形式）を書き出す（D073）。
//! - `scn export epub|pdf [project-dir] [--route 名前 | --ending ID] [--horizontal] [--no-notes] [-o file]`：
//!   道ごとの EPUB と、縦書きPDFを組むための印刷用の HTML を書き出す（D074）。
//! - `scn export pdf … --typeset`：印刷用の HTML を書いた後に、`export/book.yaml` の `pdf_command` で PDF を組む
//!   （D075）。
//! - `scn export <プロファイル名> [project-dir] [-o 書き出し先] [--no-ai-drafts]`：作品フォルダの出力プロファイル
//!   `export/<名前>.yaml` のプラグイン（テンプレートか外部コマンド）で書き出す（D075）。
//! - `scn export orders [project-dir] [-o file]`：発注リストを CSV で書き出す（D054）。
//! - `scn export text [project-dir] [--route 名前 | --ending ID] [--no-notes] [-o file]`：ルート単位の通読用の
//!   テキストを書き出す（D056）。
//! - `scn lsp`：言語サーバー（LSP）を標準入出力で動かす（D076。VS Code 拡張などから使う）。
//! - `scn fmt <project-dir> [--check] [--json]`：行IDの無い行に行IDを付ける（仕様書9節、D041）。
//!   付けた行IDを `ファイル:行:列 ^ID` の形で出す。`--check` では書き換えず、付ける行があれば1で終わる。
//!   `project.toml` の `format_version` か `[dialogue]` を読めなければ、台詞の括弧を書かれたとおりに読めないので、
//!   何もせずに2で終わる（D045）。
//!
//! 終了コードは、成功が0、エラー（E）の診断があれば1、使い方の誤り・`project.toml` の無いフォルダ・
//! 入出力の誤りは2。`fmt` は本文の診断では1にしない（`--check` で行IDの無い行があるときだけ1）。
//!
//! 入れ子の深い本文でもスタックがあふれないよう、コマンドは大きなスタックのスレッドで実行する
//! （Windowsのメインスレッドは1MB。アプリと同じ64MB）。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;

use clap::{Parser, Subcommand, ValueEnum};
use scn_core::check::check;
use scn_core::graph::{Graph, GraphEdge, Level, build_graph};
use scn_core::index::build_index;
use scn_core::ir::IrDiagnostic;
use scn_core::line_id::{LineIdGenerator, assign_line_ids};
use scn_core::project::SourceFile;
use scn_core::scaffold::{create_project, stale_docs, write_generated_docs};
use scn_core::{
    LineIndex, ParseOptions, ProjectError, Severity, analyze, load_project, parse, write_source,
};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "scn",
    version,
    about = "scn-studio のコマンド：作品フォルダの .scn と設定を解析・検証し、行IDを付け、書き出す",
    after_help = "使い方の説明：作品フォルダの SCN.md（scn-studio のリポジトリでは docs/guide/scn.md）。\n\
                  コマンドごとの説明は scn <コマンド> --help。"
)]
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
        /// 整形しない（字下げと改行の無い）JSONで出す。入れ子の深い本文でも大きくならない
        #[arg(long)]
        compact: bool,
    },
    /// プロジェクト全体を検証し、診断を `ファイル:行:列 コード メッセージ ^行ID` の形で出す
    Check {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 検証の段階。2 なら自動巡回（条件を実行して、選べない選択肢や着かないシーンを調べる）と表記の検査も行い、
        /// エンディングごとの到達条件を標準エラーに出す（`--json` では `playthrough` に）
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
        stage: u8,
        /// 診断をJSONで出す
        #[arg(long)]
        json: bool,
    },
    /// シーン、辺、条件、変数の読み書きを、粒度ごとに一覧で出す
    Graph {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// まとめる粒度
        #[arg(long, value_enum, default_value_t = GraphLevel::Scene)]
        level: GraphLevel,
        /// JSONで出す
        #[arg(long)]
        json: bool,
    },
    /// 新しい作品フォルダを作る（無いフォルダか、空のフォルダに）
    New {
        /// 作るフォルダ
        dir: PathBuf,
        /// 作品名。省くとフォルダ名
        #[arg(long)]
        title: Option<String>,
    },
    /// FORMAT.md（形式の仕様と作品フォルダの規約）と SCN.md（コマンドの使い方）を、今のツールの版で書き直す
    FormatDoc {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 書き直さずに、古いか（無いか）だけを確かめる。どちらかが古ければ終了コード1
        #[arg(long)]
        check: bool,
    },
    /// AIタスクの補助（LLM は呼ばない）
    Ai {
        #[command(subcommand)]
        what: AiCommand,
    },
    /// 言語サーバー（LSP）を標準入出力で動かす（VS Code 拡張などのエディタから使う。診断、補完、定義へ移動、
    /// 参照、名前の変更、ホバー）
    Lsp,
    /// 書き出す（台本など）
    #[command(
        after_help = "ほかの名前は、作品フォルダの出力プロファイル export/<名前>.yaml のプラグイン（template か \
                      command を書いたもの）で書き出します：scn export <名前> [project-dir] [-o 書き出し先] \
                      [--no-ai-drafts]"
    )]
    Export {
        #[command(subcommand)]
        what: ExportCommand,
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
        /// 付けた行IDを、AIが書いた行として出自の記録（ai/provenance.jsonl）に書き足す。値はタスクの名前
        /// （外部のエージェントなら external:claude-code など）
        #[arg(long, value_name = "TASK", requires = "model")]
        provenance: Option<String>,
        /// 出自の記録に書くモデルの名前（--provenance と一緒に）
        #[arg(long, requires = "provenance")]
        model: Option<String>,
    },
    /// AIが書いた行（出自の記録で ai か ai-edited の行）の一覧を出す。賞や公募の申告に使う
    Provenance {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// JSONで出す
        #[arg(long)]
        json: bool,
        /// 一覧を CSV（UTF-8 の BOM 付き、CRLF）で書き出す先。`-` なら標準出力
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AiCommand {
    /// タスクの定義から組み立てた文脈（LLM に渡す指示と依頼）を出す。LLM は呼ばない
    Context {
        /// タスクの名前（`ai/<名前>.yaml` の名前）
        task: String,
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 今のシーンのID（文脈に scene・previous_summaries を入れるタスクで）
        #[arg(long)]
        scene: Option<String>,
        /// 文脈の長さ（トークン数）。省くと、タスクの接続先の context_tokens
        #[arg(long)]
        tokens: Option<u64>,
        /// 返す分の上限（トークン数）。省くと、タスクの接続先の output_tokens（無ければ文脈の長さの4分の1、多くて
        /// 4096）
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        output_tokens: Option<u64>,
        /// 対象のルート（ルートの骨格を作るタスクで。依頼に書き足す）
        #[arg(long)]
        route: Option<String>,
        /// JSONで出す
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ExportCommand {
    /// 台本（収録と校正の表）を CSV（UTF-8 の BOM 付き、CRLF）か Excel 形式で書き出す
    Script {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 形式
        #[arg(long, value_enum, default_value_t = ScriptFormatArg::Csv)]
        format: ScriptFormatArg,
        /// 書き出した台本を、収録の基準として記録する（export/script-baseline.json。次からの書き出しで、
        /// 変わった台詞の備考に「再収録候補」と書く）
        #[arg(long)]
        baseline: bool,
        /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
        #[arg(long)]
        no_ai_drafts: bool,
        /// 書き出す先。省くと作品フォルダの export/script.csv（Excel 形式は export/script.xlsx）。`-` なら標準出力
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// 発注リスト（まだファイルの無い素材の表）を CSV（UTF-8 の BOM 付き、CRLF）で書き出す
    Orders {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 書き出す先。省くと作品フォルダの export/orders.csv。`-` なら標準出力
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// ルート単位の通読用のテキスト（ルビはなろう・カクヨム記法）を書き出す。`--route` も `--ending` も無ければ、
    /// 共通（ルートの無い作品は通し）とすべてのルートを export/text/ に書く
    Text {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// ルートの名前（シーンのメタ情報の route）
        #[arg(long, conflicts_with = "ending")]
        route: Option<String>,
        /// エンディングのID（そこまでの最短経路で読む）
        #[arg(long)]
        ending: Option<String>,
        /// 選んだ選択肢とエンディングの注記（〔選択：…〕）を書かない
        #[arg(long)]
        no_notes: bool,
        /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
        #[arg(long)]
        no_ai_drafts: bool,
        /// 書き出すファイル（`--route` か `--ending` で1つの道を選んだときだけ使える）。省くと
        /// export/text/<道の名前>.txt。`-` なら標準出力
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// 道（共通・ルート・エンディング）ごとの EPUB 3 を、出力プロファイル（export/book.yaml）に従って書き出す
    Epub(BookArgs),
    /// 縦書きPDFを組むための印刷用の HTML（CSS の @page）を書き出す。PDF は Vivliostyle などで組む
    Pdf(BookArgs),
    /// Ren'Py のスクリプトを、出力プロファイル（export/renpy.yaml）に従って書き出す。書き出し先の
    /// game/scn/ を作り直す
    Renpy {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 書き出し先（Ren'Py のプロジェクトのフォルダ）。省くとプロファイルの output（既定は export/renpy）
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
        #[arg(long)]
        no_ai_drafts: bool,
    },
    /// ティラノスクリプトのシナリオを、出力プロファイル（export/tyrano.yaml）に従って書き出す。書き出し先の
    /// data/scenario/scn/ と素材の data/*/scn/ を作り直す
    Tyrano {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 書き出し先（ティラノスクリプトのプロジェクトのフォルダ）。省くとプロファイルの output（既定は
        /// export/tyrano）
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
        #[arg(long)]
        no_ai_drafts: bool,
    },
    /// 宴のシナリオ（シーンごとのシート）と設定のシート（Character・Texture・Sound・Param）を、出力プロファイル
    /// （export/utage.yaml）に従って Excel 形式で書き出す。書き出し先をまるごと作り直す
    Utage {
        /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 書き出し先。省くとプロファイルの output（既定は export/utage）
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
        #[arg(long)]
        no_ai_drafts: bool,
    },
    /// 作品フォルダの出力プロファイル export/<名前>.yaml のプラグイン（T3-14）
    #[command(external_subcommand)]
    Plugin(Vec<OsString>),
}

/// `scn export <プロファイル名>` の引数（作品フォルダの出力プロファイル export/<名前>.yaml のプラグイン。T3-14）。
#[derive(Parser)]
#[command(
    name = "scn export <プロファイル名>",
    about = "作品フォルダの出力プロファイル export/<名前>.yaml のプラグインで書き出す（template：テンプレートで\
             テキストを書く。command：IR の JSON を標準入力で渡して外部コマンドを実行する）"
)]
struct PluginArgs {
    /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
    #[arg(default_value = ".")]
    project: PathBuf,
    /// 書き出し先（テンプレートはファイルで、`-` なら標準出力。外部コマンドはフォルダ）。省くとプロファイルの
    /// output（既定は export/<名前>/ の中）
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
    #[arg(long)]
    no_ai_drafts: bool,
}

/// `scn export epub`・`scn export pdf` の引数。
#[derive(clap::Args)]
struct BookArgs {
    /// プロジェクトのフォルダ（project.toml のあるところ）。省くと今のフォルダ
    #[arg(default_value = ".")]
    project: PathBuf,
    /// ルートの名前（シーンのメタ情報の route）
    #[arg(long, conflicts_with = "ending")]
    route: Option<String>,
    /// エンディングのID（そこまでの最短経路で読む）
    #[arg(long)]
    ending: Option<String>,
    /// 横書きにする（既定はプロファイルの writing、無ければ縦書き）
    #[arg(long)]
    horizontal: bool,
    /// 選んだ選択肢とエンディングの注記（〔選択：…〕）を書かない
    #[arg(long)]
    no_notes: bool,
    /// AI下書き（status: ai-draft）のシーンの台詞と地の文を省く（流れは残し、省いたことを地の文で示す）
    #[arg(long)]
    no_ai_drafts: bool,
    /// 書き出すファイル（`--route` か `--ending` で1つの道を選んだときだけ使える）。省くと
    /// export/epub/<道の名前>.epub（PDF は export/pdf/<道の名前>.html）。`-` なら標準出力
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// 印刷用の HTML を書いた後に、export/book.yaml の pdf_command を実行して PDF（HTML と同じ名前の .pdf）を組む
    /// （pdf だけ）
    #[arg(long)]
    typeset: bool,
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
        Command::Ir { project, compact } => run_ir(&project, compact),
        Command::Check {
            project,
            stage,
            json,
        } => run_check(&project, stage, json),
        Command::Graph {
            project,
            level,
            json,
        } => run_graph(&project, level.into(), json),
        Command::New { dir, title } => run_new(&dir, title.as_deref()),
        Command::Lsp => run_lsp(),
        Command::FormatDoc { project, check } => run_format_doc(&project, check),
        Command::Fmt {
            project,
            check,
            json,
            provenance,
            model,
        } => run_fmt(
            &project,
            check,
            json,
            provenance.as_deref().zip(model.as_deref()),
        ),
        Command::Provenance {
            project,
            json,
            output,
        } => run_provenance(&project, json, output.as_deref()),
        Command::Ai {
            what:
                AiCommand::Context {
                    task,
                    project,
                    scene,
                    tokens,
                    output_tokens,
                    route,
                    json,
                },
        } => run_ai_context(
            &project,
            &task,
            scene.as_deref(),
            route.as_deref(),
            tokens,
            output_tokens,
            json,
        ),
        Command::Export {
            what:
                ExportCommand::Script {
                    project,
                    format,
                    baseline,
                    no_ai_drafts,
                    output,
                },
        } => run_export_script(
            &project,
            format.into(),
            baseline,
            no_ai_drafts,
            output.as_deref(),
        ),
        Command::Export {
            what:
                ExportCommand::Renpy {
                    project,
                    output,
                    no_ai_drafts,
                },
        } => run_export_renpy(&project, output.as_deref(), no_ai_drafts),
        Command::Export {
            what: ExportCommand::Epub(args),
        } => run_export_book(&args, false),
        Command::Export {
            what: ExportCommand::Pdf(args),
        } => run_export_book(&args, true),
        Command::Export {
            what:
                ExportCommand::Tyrano {
                    project,
                    output,
                    no_ai_drafts,
                },
        } => run_export_tyrano(&project, output.as_deref(), no_ai_drafts),
        Command::Export {
            what:
                ExportCommand::Utage {
                    project,
                    output,
                    no_ai_drafts,
                },
        } => run_export_utage(&project, output.as_deref(), no_ai_drafts),
        Command::Export {
            what: ExportCommand::Orders { project, output },
        } => run_export_orders(&project, output.as_deref()),
        Command::Export {
            what: ExportCommand::Plugin(args),
        } => run_export_plugin(&args),
        Command::Export {
            what:
                ExportCommand::Text {
                    project,
                    route,
                    ending,
                    no_notes,
                    no_ai_drafts,
                    output,
                },
        } => run_export_text(
            &project,
            route.as_deref(),
            ending.as_deref(),
            !no_notes,
            no_ai_drafts,
            output.as_deref(),
        ),
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
/// 解析にも響くため）と、そのファイルの診断を返す。ファイルが `story/` の下の `*.scn` でなくても、
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

fn run_ir(dir: &Path, compact: bool) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let body = if compact {
        serde_json::to_string(&ir)
    } else {
        serde_json::to_string_pretty(&ir)
    }
    .map_err(|e| CliError::Write(e.into()))?;
    finish(
        writeln!(io::stdout().lock(), "{body}"),
        exit_code(&ir.diagnostics),
    )
}

// ---- scn export script ----

/// `--format` の値。
#[derive(Clone, Copy, ValueEnum)]
enum ScriptFormatArg {
    /// CSV（UTF-8 の BOM 付き、CRLF）
    Csv,
    /// Excel 形式（.xlsx。欄はすべて文字列）
    Xlsx,
}

impl From<ScriptFormatArg> for scn_core::script::ScriptFormat {
    fn from(format: ScriptFormatArg) -> Self {
        match format {
            ScriptFormatArg::Csv => Self::Csv,
            ScriptFormatArg::Xlsx => Self::Xlsx,
        }
    }
}

/// 台本を書き出す（T1-12、D051。Excel 形式と再収録候補は T2-10、D061）。既定の書き出し先は作品フォルダの
/// `export/script.csv`（Excel 形式は `export/script.xlsx`。コアの `write_script`）。行IDの無い台詞、エラーの
/// 診断、収録の基準と比べた数があれば、標準エラーに出す。`baseline` なら、書き出した後に収録の基準を書き直す。
/// 終了コードは `ir` と同じ（エラーの診断があれば1）。
/// 書き出しの IR。`no_ai_drafts` なら AI下書きのシーンの台詞と地の文を省く（T3-9）。
fn analyze_for_export(project: &scn_core::Project, no_ai_drafts: bool) -> scn_core::Ir {
    let ir = analyze(project);
    if no_ai_drafts {
        scn_core::ai::without_ai_drafts(&ir)
    } else {
        ir
    }
}

fn run_export_script(
    dir: &Path,
    format: scn_core::script::ScriptFormat,
    baseline: bool,
    no_ai_drafts: bool,
    output: Option<&Path>,
) -> Result<u8, CliError> {
    use scn_core::script::{mark_retakes, read_baseline, write_baseline};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, no_ai_drafts);
    let mut sheet = scn_core::script::script(&project, &ir);
    let code = exit_code(&ir.diagnostics);
    match read_baseline(dir) {
        Ok(Some(base)) => {
            mark_retakes(&mut sheet, &base);
            if let Some(r) = &sheet.retakes {
                eprintln!(
                    "scn: 収録の基準から、変わった台詞 {} 行（再収録候補）、足した台詞 {} 行、無くなった台詞 {} 行",
                    r.changed, r.added, r.removed
                );
                if !r.removed_ids.is_empty() {
                    eprintln!("scn: 無くなった台詞の行ID：{}", r.removed_ids.join(" "));
                }
            }
        }
        Ok(None) => {}
        Err(problem) => eprintln!("scn: {problem}（再収録候補を付けずに書き出します）"),
    }
    if sheet.missing_line_ids > 0 {
        eprintln!(
            "scn: 行IDの無い（または形の合わない）台詞が {} 行あります。ボイスファイル名を空にしました（scn check で確かめ、scn fmt で行IDを付けられます）",
            sheet.missing_line_ids
        );
    }
    let errors = ir
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error.as_str())
        .count();
    if errors > 0 {
        eprintln!("scn: エラーの診断が {errors} 件あります（scn check で確かめられます）");
    }
    let bytes = format.bytes(&sheet).map_err(CliError::Project)?;
    let path = match output {
        Some(path) if path == Path::new("-") => None,
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|e| {
                    CliError::Refused(format!("{} を作れません：{e}", parent.display()))
                })?;
            }
            fs::write(path, &bytes)
                .map_err(|e| CliError::Refused(format!("{} に書けません：{e}", path.display())))?;
            Some(path.to_owned())
        }
        None => {
            Some(scn_core::script::write_script(dir, &sheet, format).map_err(CliError::Project)?)
        }
    };
    if baseline {
        let written = write_baseline(dir, &sheet).map_err(CliError::Project)?;
        eprintln!("scn: 収録の基準を {} に記録しました", written.display());
    }
    let Some(path) = path else {
        return finish(io::stdout().lock().write_all(&bytes), code);
    };
    finish(
        writeln!(
            io::stdout().lock(),
            "{}（{}行）",
            path.display(),
            sheet.rows.len()
        ),
        code,
    )
}

fn run_export_orders(dir: &Path, output: Option<&Path>) -> Result<u8, CliError> {
    use scn_core::assets::{asset_list, orders, orders_csv, write_orders};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let index = scn_core::index::build_index(&project, &ir);
    let rows = orders(&asset_list(&project, dir, &index));
    let code = exit_code(&ir.diagnostics);
    let errors = ir
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error.as_str())
        .count();
    if errors > 0 {
        eprintln!("scn: エラーの診断が {errors} 件あります（scn check で確かめられます）");
    }
    let path = match output {
        Some(path) if path == Path::new("-") => {
            return finish(
                io::stdout().lock().write_all(orders_csv(&rows).as_bytes()),
                code,
            );
        }
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|e| {
                    CliError::Refused(format!("{} を作れません：{e}", parent.display()))
                })?;
            }
            fs::write(path, orders_csv(&rows))
                .map_err(|e| CliError::Refused(format!("{} に書けません：{e}", path.display())))?;
            path.to_owned()
        }
        None => write_orders(dir, &rows).map_err(CliError::Project)?,
    };
    finish(
        writeln!(
            io::stdout().lock(),
            "{}（{}行）",
            path.display(),
            rows.len()
        ),
        code,
    )
}

fn run_export_text(
    dir: &Path,
    route: Option<&str>,
    ending: Option<&str>,
    notes: bool,
    no_ai_drafts: bool,
    output: Option<&Path>,
) -> Result<u8, CliError> {
    use scn_core::plaintext::{plain_text, write_plain_text};
    use scn_core::route::{ReadingTarget, Variables, read, reading_choices, target_label};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    let targets: Vec<ReadingTarget> = match (route, ending) {
        (Some(name), _) => vec![ReadingTarget::Route {
            name: name.to_owned(),
        }],
        (None, Some(id)) => vec![ReadingTarget::Ending { id: id.to_owned() }],
        (None, None) => reading_choices(&ir)
            .into_iter()
            .map(|c| c.target)
            .filter(|t| !matches!(t, ReadingTarget::Ending { .. }))
            .collect(),
    };
    if targets.is_empty() {
        return Err(CliError::Refused("シーンがありません".to_owned()));
    }
    let single = route.is_some() || ending.is_some();
    if output.is_some() && targets.len() > 1 {
        return Err(CliError::Refused(
            "-o は --route か --ending と一緒に使います（1つの道だけを書き出すとき）".to_owned(),
        ));
    }
    let mut out = io::stdout().lock();
    for target in &targets {
        let reading = read(&project, &ir, target, &Variables::new());
        let label = target_label(&ir, target);
        if let Some(message) = &reading.message {
            // 1つの道を指定して着けなければ、書き出せないので2。まとめて書くときは、着けない道を飛ばす。
            if single {
                return Err(CliError::Refused(format!("{label}：{message}")));
            }
            eprintln!("scn: {label}：{message}");
            continue;
        }
        let text = plain_text(&project, &reading, &label, notes);
        match output {
            Some(path) if path == Path::new("-") => {
                return finish(out.write_all(text.as_bytes()), code);
            }
            Some(path) => {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent).map_err(|e| {
                        CliError::Refused(format!("{} を作れません：{e}", parent.display()))
                    })?;
                }
                fs::write(path, &text).map_err(|e| {
                    CliError::Refused(format!("{} に書けません：{e}", path.display()))
                })?;
                return finish(writeln!(out, "{}", path.display()), code);
            }
            None => {
                let path = write_plain_text(dir, &label, &text).map_err(CliError::Project)?;
                if let Err(e) = writeln!(out, "{}（{}）", path.display(), reading.stop) {
                    return finish(Err(e), code);
                }
            }
        }
    }
    Ok(code)
}

/// `scn export epub`・`scn export pdf`：道ごとに EPUB か印刷用の HTML を書き出す（道の選び方は `export text` と
/// 同じ）。
fn run_export_book(args: &BookArgs, print: bool) -> Result<u8, CliError> {
    use scn_core::book::{
        BOOK_PROFILE, EPUB_DIR, PRINT_DIR, book_path, epub, print_html, read_profile, write_book,
    };
    use scn_core::route::{ReadingTarget, Variables, read, reading_choices, target_label};
    let dir = args.project.as_path();
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, args.no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    let (mut profile, problems) = read_profile(dir);
    for problem in &problems {
        eprintln!("scn: {BOOK_PROFILE}：{problem}");
    }
    let typeset = if args.typeset {
        if !print {
            return Err(CliError::Refused(
                "--typeset は scn export pdf で使います".to_owned(),
            ));
        }
        if args.output.as_deref() == Some(Path::new("-")) {
            return Err(CliError::Refused(
                "--typeset は -o - と一緒に使えません（PDF を組むには HTML のファイルが要ります）"
                    .to_owned(),
            ));
        }
        match &profile.pdf_command {
            Some(argv) => Some(argv.clone()),
            None => {
                return Err(CliError::Refused(format!(
                    "{BOOK_PROFILE} に pdf_command（PDF を組むコマンド）がありません"
                )));
            }
        }
    } else {
        None
    };
    if args.horizontal {
        profile.vertical = false;
    }
    if args.no_notes {
        profile.notes = false;
    }
    let targets: Vec<ReadingTarget> = match (&args.route, &args.ending) {
        (Some(name), _) => vec![ReadingTarget::Route { name: name.clone() }],
        (None, Some(id)) => vec![ReadingTarget::Ending { id: id.clone() }],
        (None, None) => reading_choices(&ir)
            .into_iter()
            .map(|c| c.target)
            .filter(|t| !matches!(t, ReadingTarget::Ending { .. }))
            .collect(),
    };
    if targets.is_empty() {
        return Err(CliError::Refused("シーンがありません".to_owned()));
    }
    let single = args.route.is_some() || args.ending.is_some();
    if args.output.is_some() && !single {
        return Err(CliError::Refused(
            "-o は --route か --ending と一緒に使います（1つの道だけを書き出すとき）".to_owned(),
        ));
    }
    let modified = scn_core::provenance::now();
    let mut out = io::stdout().lock();
    for target in &targets {
        let reading = read(&project, &ir, target, &Variables::new());
        let label = target_label(&ir, target);
        if let Some(message) = &reading.message {
            if single {
                return Err(CliError::Refused(format!("{label}：{message}")));
            }
            eprintln!("scn: {label}：{message}");
            continue;
        }
        let (bytes, default) = if print {
            (
                print_html(&project, &reading, &label, &profile).into_bytes(),
                book_path(dir, PRINT_DIR, &label, "html"),
            )
        } else {
            (
                epub(&project, &reading, &label, &profile, &modified)
                    .map_err(|e| CliError::Refused(e.to_string()))?,
                book_path(dir, EPUB_DIR, &label, "epub"),
            )
        };
        if args.output.as_deref() == Some(Path::new("-")) {
            return finish(out.write_all(&bytes), code);
        }
        let path = args.output.clone().unwrap_or(default);
        write_book(&path, &bytes).map_err(CliError::Project)?;
        if let Err(e) = writeln!(out, "{}（{}）", path.display(), reading.stop) {
            return finish(Err(e), code);
        }
        if let Some(argv) = &typeset {
            let pdf = path.with_extension("pdf");
            eprintln!("scn: 実行します：{}", scn_core::plugin::command_line(argv));
            scn_core::plugin::typeset(dir, argv, profile.pdf_timeout, &path, &pdf, false)
                .map_err(|e| CliError::Refused(e.to_string()))?;
            if let Err(e) = writeln!(out, "{}", pdf.display()) {
                return finish(Err(e), code);
            }
        }
    }
    if print && typeset.is_none() {
        if profile.pdf_command.is_some() {
            eprintln!("scn: --typeset で {BOOK_PROFILE} の pdf_command を実行して PDF を組みます");
        } else {
            eprintln!(
                "scn: PDF は CSS 組版の道具で組みます（例：vivliostyle build <HTML> -o <PDF>。\
                 {BOOK_PROFILE} の pdf_command に書けば --typeset で組めます）"
            );
        }
    }
    Ok(code)
}

/// `scn lsp`：言語サーバーを標準入出力で動かす（T3-15、D076）。大きなスタックのスレッドの上で、1つのスレッドの
/// 実行環境を回す（入れ子の深い本文の解析でスタックがあふれないように）。
fn run_lsp() -> Result<u8, CliError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::Refused(format!("言語サーバーを動かせません：{e}")))?;
    runtime.block_on(scn_lsp::serve_stdio());
    Ok(SUCCESS)
}

/// `scn export <プロファイル名>`：作品フォルダの出力プロファイル `export/<名前>.yaml` のプラグインで書き出す
/// （T3-14、D075）。テンプレートは書いたファイルを、外部コマンドは書き出し先のフォルダを出す。外部コマンドの
/// 標準出力と標準エラーはそのまま流す。プロファイルやテンプレートの誤り、コマンドの失敗は2。
fn run_export_plugin(raw: &[OsString]) -> Result<u8, CliError> {
    use scn_core::plugin::{
        PluginError, PluginKind, command_line, export_command, list_plugins, read_plugin,
        render_template, write_output,
    };
    let Some(name) = raw.first().and_then(|n| n.to_str()) else {
        return Err(CliError::Refused(
            "プロファイルの名前は UTF-8 の文字で書きます".to_owned(),
        ));
    };
    // 使い方の表示は `scn export <名前>` にする。
    let shown = std::iter::once(OsString::from(format!("scn export {name}")))
        .chain(raw.iter().skip(1).cloned());
    let args = match PluginArgs::try_parse_from(shown) {
        Ok(args) => args,
        Err(e) => {
            let _ = e.print();
            return Ok(if e.use_stderr() { FAILURE } else { SUCCESS });
        }
    };
    let dir = args.project.as_path();
    let project = load_project(dir).map_err(CliError::Project)?;
    let plugin = read_plugin(dir, name).map_err(|e| {
        let mut message = e.to_string();
        if matches!(e, PluginError::Missing(_)) {
            let names: Vec<String> = list_plugins(dir).into_iter().map(|(n, _)| n).collect();
            if names.is_empty() {
                message.push_str("（プラグインのプロファイルはありません）");
            } else {
                message.push_str(&format!(
                    "（プラグインのプロファイル：{}）",
                    names.join("、")
                ));
            }
        }
        CliError::Refused(message)
    })?;
    for problem in &plugin.problems {
        eprintln!("scn: {}：{problem}", plugin.profile);
    }
    let ir = analyze_for_export(&project, args.no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    match &plugin.kind {
        PluginKind::Template { .. } => {
            let text = render_template(dir, &plugin, &project, &ir)
                .map_err(|e| CliError::Refused(e.to_string()))?;
            let mut out = io::stdout().lock();
            match args.output.as_deref() {
                Some(path) if path == Path::new("-") => {
                    finish(out.write_all(text.as_bytes()), code)
                }
                output => {
                    let path = output.map_or_else(|| dir.join(&plugin.output), Path::to_owned);
                    write_output(&path, &text).map_err(|e| CliError::Refused(e.to_string()))?;
                    finish(writeln!(out, "{}", path.display()), code)
                }
            }
        }
        PluginKind::Command { argv } => {
            if args.output.as_deref() == Some(Path::new("-")) {
                return Err(CliError::Refused(
                    "外部コマンドの -o は書き出し先のフォルダです（- は使えません）".to_owned(),
                ));
            }
            let path = args
                .output
                .clone()
                .unwrap_or_else(|| dir.join(&plugin.output));
            eprintln!("scn: 実行します：{}", command_line(argv));
            export_command(dir, &plugin, &ir, &path, false)
                .map_err(|e| CliError::Refused(e.to_string()))?;
            finish(writeln!(io::stdout().lock(), "{}", path.display()), code)
        }
    }
}

fn run_export_renpy(dir: &Path, output: Option<&Path>, no_ai_drafts: bool) -> Result<u8, CliError> {
    use scn_core::renpy::{RENPY_PROFILE, export_renpy, read_profile, write_renpy};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    let (profile, problems) = read_profile(dir);
    for problem in &problems {
        eprintln!("scn: {RENPY_PROFILE}：{problem}");
    }
    let export = export_renpy(&project, dir, &ir, &profile);
    for warning in &export.warnings {
        eprintln!("scn: {warning}");
    }
    let errors = ir
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error.as_str())
        .count();
    if errors > 0 {
        eprintln!("scn: エラーの診断が {errors} 件あります（scn check で確かめられます）");
    }
    let base = write_renpy(dir, &profile, &export, output).map_err(CliError::Project)?;
    finish(
        writeln!(
            io::stdout().lock(),
            "{}（{}ファイル）",
            base.display(),
            export.files.len()
        ),
        code,
    )
}

fn run_export_tyrano(
    dir: &Path,
    output: Option<&Path>,
    no_ai_drafts: bool,
) -> Result<u8, CliError> {
    use scn_core::tyrano::{TYRANO_PROFILE, export_tyrano, read_profile, write_tyrano};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    let (profile, problems) = read_profile(dir);
    for problem in &problems {
        eprintln!("scn: {TYRANO_PROFILE}：{problem}");
    }
    let export = export_tyrano(&project, dir, &ir, &profile);
    for warning in &export.warnings {
        eprintln!("scn: {warning}");
    }
    let errors = ir
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error.as_str())
        .count();
    if errors > 0 {
        eprintln!("scn: エラーの診断が {errors} 件あります（scn check で確かめられます）");
    }
    let base = write_tyrano(dir, &profile, &export, output).map_err(CliError::Project)?;
    finish(
        writeln!(
            io::stdout().lock(),
            "{}（{}ファイル）",
            base.display(),
            export.files.len()
        ),
        code,
    )
}

fn run_export_utage(dir: &Path, output: Option<&Path>, no_ai_drafts: bool) -> Result<u8, CliError> {
    use scn_core::utage::{UTAGE_PROFILE, export_utage, read_profile, write_utage};
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze_for_export(&project, no_ai_drafts);
    let code = exit_code(&ir.diagnostics);
    let (profile, problems) = read_profile(dir);
    for problem in &problems {
        eprintln!("scn: {UTAGE_PROFILE}：{problem}");
    }
    let export = export_utage(&project, dir, &ir, &profile);
    for warning in &export.warnings {
        eprintln!("scn: {warning}");
    }
    let errors = ir
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error.as_str())
        .count();
    if errors > 0 {
        eprintln!("scn: エラーの診断が {errors} 件あります（scn check で確かめられます）");
    }
    let base = write_utage(dir, &profile, &export, output).map_err(CliError::Project)?;
    finish(
        writeln!(
            io::stdout().lock(),
            "{}（{}ファイル）",
            base.display(),
            export.files.len()
        ),
        code,
    )
}

// ---- scn fmt ----

/// 行IDを付けた（`--check` では、付ける）行。行と列は1から数える（列は文字の数）。
#[derive(Serialize, Clone)]
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
fn run_fmt(
    dir: &Path,
    check: bool,
    json: bool,
    provenance: Option<(&str, &str)>,
) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    // 台詞の括弧が既定に戻っていると、複数行の台詞の途中の行にも行IDを付けてしまう（D042、D045）。
    if project.has_reading_errors() {
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
            "project.toml の format_version か [dialogue] を書かれたとおりに読めないので、行IDを付けません（台詞の括弧が既定に戻るため）".into(),
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
        // 書いたファイルの行IDは、出力に失敗しても出自の記録に書き足せるよう、先に覚える。
        done.extend(entries.iter().cloned());
        if !json {
            let written = entries.iter().try_for_each(|e| match &e.id {
                Some(id) => writeln!(out, "{}:{}:{} ^{id}", e.file, e.line, e.column),
                None => writeln!(out, "{}:{}:{} 行IDがありません", e.file, e.line, e.column),
            });
            if let Err(e) = written
                && e.kind() != io::ErrorKind::BrokenPipe
            {
                failure = Some(CliError::Write(e));
                break;
            }
        }
    }

    if json {
        let body = serde_json::to_string_pretty(&done).map_err(|e| CliError::Write(e.into()))?;
        let written = writeln!(out, "{body}");
        if let Err(e) = written
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            // 出力に失敗しても、書いたファイルの出自は下で書き足す（先の誤りがあれば、それを残す）。
            failure.get_or_insert(CliError::Write(e));
        }
    }
    // 付けた行IDを、AIが書いた行として出自の記録に書き足す（T3-4、D065。書き終えたファイルの分だけ）。
    if let Some((task, model)) = provenance
        && !check
    {
        let at = scn_core::provenance::now();
        let records: Vec<scn_core::provenance::Record> = done
            .iter()
            .filter_map(|e| e.id.clone())
            .map(|line| scn_core::provenance::Record {
                line,
                origin: scn_core::provenance::AI.to_owned(),
                task: task.to_owned(),
                model: model.to_owned(),
                at: at.clone(),
                ..scn_core::provenance::Record::default()
            })
            .collect();
        match scn_core::provenance::append_provenance(dir, &records) {
            Ok(()) if !records.is_empty() => eprintln!(
                "出自を記録しました：{}件（{}）",
                records.len(),
                scn_core::provenance::PROVENANCE_FILE
            ),
            Ok(()) => {}
            // 書き込みの誤りが先にあれば、それを返す（出自の誤りは知らせるだけ）。
            Err(e) if failure.is_some() => eprintln!("scn: 出自を記録できませんでした：{e}"),
            Err(e) => return Err(CliError::Project(e)),
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

// ---- scn provenance ----

/// JSONで出す、AIの行。
#[derive(Serialize)]
struct CliProvenance<'a> {
    file: String,
    line: usize,
    column: usize,
    id: &'a str,
    /// 項目の行（企画書と設定DBのエントリ。T4-3）のキー。本文の行では書かない。
    #[serde(skip_serializing_if = "str::is_empty")]
    key: &'a str,
    scene: &'a str,
    origin: &'a str,
    task: &'a str,
    model: &'a str,
    at: &'a str,
    text: &'a str,
}

/// `scn ai context`：タスクの文脈を組み立てて出す。あらまし（節、削った節、知らせ）は標準エラーに、指示と依頼は
/// 標準出力に。実行できない（入りきらない、シーンが要る）ときは終了コード1。
/// `tokens`・`output_tokens` は `--tokens`・`--output-tokens`（省くと接続先の値）。
fn run_ai_context(
    dir: &Path,
    task_id: &str,
    scene: Option<&str>,
    route: Option<&str>,
    tokens: Option<u64>,
    output_tokens: Option<u64>,
    json: bool,
) -> Result<u8, CliError> {
    use scn_core::ai;
    let project = load_project(dir).map_err(CliError::Project)?;
    let Some(task) = project.ai_tasks.iter().find(|t| t.id == task_id) else {
        let names: Vec<&str> = project.ai_tasks.iter().map(|t| t.id.as_str()).collect();
        return Err(CliError::Refused(format!(
            "ai/{task_id}.yaml がありません（タスク：{}）",
            if names.is_empty() {
                "なし".to_owned()
            } else {
                names.join("、")
            }
        )));
    };
    // 接続先の値（CLI は端末の上書き `ai.toml` を読まない）。`--tokens` と `--output-tokens` がそれぞれ上書きする。
    let machine = ai::MachineSettings::default();
    let name = task
        .model
        .clone()
        .or_else(|| ai::default_connection(&project.settings.ai, &machine).map(str::to_owned));
    let resolved = name.as_deref().and_then(|name| {
        project
            .settings
            .ai
            .iter()
            .find(|d| d.name == name)
            .map(|d| ai::resolve(d, &machine))
    });
    let connection = match resolved {
        Some(Ok(conn)) => Some(conn.budget()),
        Some(Err(problem)) => {
            if tokens.is_none() {
                eprintln!(
                    "{problem}。文脈の長さを {} とします",
                    ai::DEFAULT_CONTEXT_TOKENS
                );
            }
            None
        }
        None => {
            if tokens.is_none() {
                eprintln!(
                    "接続先「{}」が project.toml にありません。文脈の長さを {} とします",
                    name.as_deref().unwrap_or("（なし）"),
                    ai::DEFAULT_CONTEXT_TOKENS
                );
            }
            None
        }
    };
    let budget = ai::TokenBudget {
        context_tokens: tokens
            .or(connection.map(|c| c.context_tokens))
            .unwrap_or(ai::DEFAULT_CONTEXT_TOKENS),
        output_tokens: output_tokens.or(connection.and_then(|c| c.output_tokens)),
    };
    let ir = analyze(&project);
    let sources = ai::read_sources(dir);
    let task = &match route {
        Some(route) if ai::is_skeleton(task) => ai::with_route(task, route),
        Some(_) => {
            eprintln!(
                "{} はルートの骨格を作るタスクではないので、--route を使いません",
                task.id
            );
            task.clone()
        }
        None => task.clone(),
    };
    let assembled = ai::assemble(&project, &ir, &sources, task, scene, budget);
    let code = if assembled.problem.is_some() {
        HAS_ERRORS
    } else {
        SUCCESS
    };
    let mut out = io::stdout().lock();
    if json {
        let text = serde_json::to_string_pretty(&assembled)
            .map_err(|e| CliError::Write(io::Error::other(e)))?;
        return finish(writeln!(out, "{text}"), code);
    }
    eprintln!(
        "タスク {}（{}）、文脈の長さ {}、返す分 {}、上限 {}、見積もり {}",
        task.id,
        task.name,
        assembled.context_tokens,
        assembled.output_tokens,
        assembled.limit,
        assembled.tokens
    );
    for s in &assembled.sections {
        eprintln!("  入れた：{}（{}）{}", s.title, s.item, s.tokens);
    }
    for t in &assembled.trimmed {
        let how = if t.how == "shortened" {
            "短くした"
        } else {
            "削った"
        };
        eprintln!("  {how}：{}（{}）{}", t.title, t.item, t.tokens);
    }
    for n in &assembled.notes {
        eprintln!("  知らせ：{n}");
    }
    if let Some(problem) = &assembled.problem {
        eprintln!("実行できません：{problem}");
    }
    if assembled.system.is_empty() {
        // 組み立てる前に止まった（タスクに prompt が無い、シーンが要る）。
        return Ok(code);
    }
    finish(
        write!(
            out,
            "=== system ===\n{}\n\n=== user ===\n{}\n",
            assembled.system, assembled.user
        ),
        code,
    )
}

/// AIが書いた行の一覧を出す（T3-4、D065）。`-o` で CSV に書き出す。出自の記録の読めない行と数を標準エラーに。
fn run_provenance(dir: &Path, json: bool, output: Option<&Path>) -> Result<u8, CliError> {
    use scn_core::provenance::{
        provenance_csv, provenance_field_rows, provenance_rows, read_provenance,
    };
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let provenance = read_provenance(dir).map_err(CliError::Project)?;
    for (line, problem) in &provenance.problems {
        eprintln!(
            "{}:{line}:1 出自の記録の行を読めません：{problem}",
            display_path(dir, scn_core::provenance::PROVENANCE_FILE)
        );
    }
    let (mut rows, counts) = provenance_rows(&project, &ir, &provenance);
    let (field_rows, fields) = provenance_field_rows(dir, &project, &provenance);
    rows.extend(field_rows);
    eprintln!(
        "AIの行 {}（ai {}、ai-edited {}）／行IDのある行 {}",
        counts.ai + counts.ai_edited,
        counts.ai,
        counts.ai_edited,
        counts.lines
    );
    if fields.ai + fields.ai_edited > 0 {
        eprintln!(
            "AIの項目 {}（ai {}、ai-edited {}）",
            fields.ai + fields.ai_edited,
            fields.ai,
            fields.ai_edited
        );
    }
    let mut out = io::stdout().lock();
    if let Some(path) = output {
        let csv = provenance_csv(&rows);
        if path == Path::new("-") {
            return finish(out.write_all(csv.as_bytes()), SUCCESS);
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::Refused(format!("{} を作れません：{e}", parent.display()))
            })?;
        }
        fs::write(path, csv)
            .map_err(|e| CliError::Refused(format!("{} に書けません：{e}", path.display())))?;
        return finish(
            writeln!(out, "{}（{}行）", path.display(), rows.len()),
            SUCCESS,
        );
    }
    let positions = |file: &str, offset: usize| {
        project
            .files
            .iter()
            .find(|f| f.path == file)
            .map_or((1, 1), |f| LineIndex::new(&f.text).line_col(offset))
    };
    if json {
        let list: Vec<CliProvenance<'_>> = rows
            .iter()
            .map(|r| {
                let (line, column) = positions(&r.file, r.span.start);
                CliProvenance {
                    file: display_path(dir, &r.file),
                    line,
                    column,
                    id: &r.id,
                    key: &r.key,
                    scene: &r.scene,
                    origin: &r.origin,
                    task: &r.task,
                    model: &r.model,
                    at: &r.at,
                    text: &r.text,
                }
            })
            .collect();
        let body = serde_json::to_string_pretty(&list).map_err(|e| CliError::Write(e.into()))?;
        return finish(writeln!(out, "{body}"), SUCCESS);
    }
    let written = rows.iter().try_for_each(|r| {
        if !r.key.is_empty() {
            // 項目の行（T4-3）：ファイルと項目。
            return writeln!(
                out,
                "{} {} {} {} {} {}",
                display_path(dir, &r.file),
                r.key,
                r.origin,
                r.task,
                r.model,
                r.at
            );
        }
        let (line, column) = positions(&r.file, r.span.start);
        writeln!(
            out,
            "{}:{line}:{column} ^{} {} {} {} {}",
            display_path(dir, &r.file),
            r.id,
            r.origin,
            r.task,
            r.model,
            r.at
        )
    });
    finish(written, SUCCESS)
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
fn run_check(dir: &Path, stage: u8, json: bool) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let loaded = project.load_diagnostics.len();
    let mut rest: Vec<IrDiagnostic> = ir.diagnostics.iter().skip(loaded).cloned().collect();
    rest.extend(check(&project, &ir, &index));
    // 検証の段階2（自動巡回。T3-2、D063）。
    let report = (stage >= 2).then(|| scn_core::playthrough::stage2(&project, &ir));
    if let Some(report) = &report {
        rest.extend(report.diagnostics.iter().cloned());
    }
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
        let body = match &report {
            // 段階2では、診断と自動巡回の結果を1つの対応表にする。
            Some(report) => serde_json::to_string_pretty(&serde_json::json!({
                "diagnostics": list,
                "playthrough": report,
            })),
            None => serde_json::to_string_pretty(&list),
        }
        .map_err(|e| CliError::Write(e.into()))?;
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
    if let Some(report) = &report {
        print_playthrough(report);
    }
    eprintln!(
        "エラー {}件、警告 {}件、情報 {}件",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info)
    );
    finish(written, code)
}

/// 自動巡回の結果（カバレッジ、エンディングごとの到達条件、変数ごとの出た値）を標準エラーに出す。
fn print_playthrough(report: &scn_core::playthrough::Playthrough) {
    eprintln!(
        "自動巡回：状態 {}、周回の始め方 {}、シーン {}/{}、選べた選択肢 {}/{}",
        report.states,
        report.starts,
        report.scenes.reached,
        report.scenes.total,
        report.options.reached,
        report.options.total
    );
    if let Some(limit) = &report.limit {
        eprintln!("自動巡回を調べきれませんでした：{limit}（W506・W507・W509 は出していません）");
    }
    for ending in &report.endings {
        if !ending.reached {
            // 調べきれなかったときは「着かない」と言わない。
            let state = if report.complete {
                "着かない"
            } else {
                "見つかっていない"
            };
            eprintln!("エンディング {}（{}）：{state}", ending.id, ending.scene);
            continue;
        }
        let example = scn_core::playthrough::example_text(&ending.example);
        let values: Vec<String> = ending
            .values
            .iter()
            .map(|(k, v)| {
                let mut shown: Vec<String> = v
                    .iter()
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
                    .collect();
                if ending.more.contains(k) {
                    shown.push("…".to_owned());
                }
                format!("{k} = {}", shown.join(" | "))
            })
            .collect();
        eprintln!(
            "エンディング {}（{}）：着く（状態 {}。例：{example}）{}",
            ending.id,
            ending.scene,
            ending.states,
            if values.is_empty() {
                String::new()
            } else {
                format!("　{}", values.join("、"))
            }
        );
    }
    for (var, seen) in &report.variables {
        let mut shown: Vec<String> = seen
            .values
            .iter()
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .collect();
        if seen.more {
            shown.push("…".to_owned());
        }
        eprintln!(
            "変数 {var}：{}{}",
            shown.join(" | "),
            if seen.flow {
                ""
            } else {
                "（流れに関わらないので、まとめた道の先の値は数えていません）"
            }
        );
    }
}

// ---- scn graph ----

/// `--level` の値。
#[derive(Clone, Copy, ValueEnum)]
enum GraphLevel {
    /// シーンごと
    Scene,
    /// 章（ファイル）ごと
    Chapter,
    /// ルートごと
    Route,
}

impl From<GraphLevel> for Level {
    fn from(level: GraphLevel) -> Self {
        match level {
            GraphLevel::Scene => Level::Scene,
            GraphLevel::Chapter => Level::Chapter,
            GraphLevel::Route => Level::Route,
        }
    }
}

fn run_graph(dir: &Path, level: Level, json: bool) -> Result<u8, CliError> {
    let project = load_project(dir).map_err(CliError::Project)?;
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    let graph = build_graph(&ir, &index, level);
    let mut out = io::stdout().lock();
    if json {
        let body = serde_json::to_string_pretty(&graph).map_err(|e| CliError::Write(e.into()))?;
        return finish(writeln!(out, "{body}"), SUCCESS);
    }
    finish(write_graph(&mut out, dir, &graph), SUCCESS)
}

/// 構造の一覧を、人が読む形で書く。
fn write_graph(out: &mut impl Write, dir: &Path, graph: &Graph) -> io::Result<()> {
    for group in &graph.groups {
        match graph.level {
            Level::Scene => {
                let mut notes = Vec::new();
                if let Some(file) = &group.file {
                    notes.push(display_path(dir, file));
                }
                if let Some(route) = &group.route {
                    notes.push(format!("ルート {route}"));
                }
                if let Some(status) = &group.status {
                    notes.push(status.clone());
                }
                if group.is_stub == Some(true) {
                    notes.push("スタブ".into());
                }
                writeln!(
                    out,
                    "{} {}（{}）",
                    group.id.as_deref().unwrap_or_default(),
                    group.title.as_deref().unwrap_or_default(),
                    notes.join("、")
                )?;
            }
            Level::Chapter => writeln!(
                out,
                "{}（シーン {}）",
                display_path(dir, group.id.as_deref().unwrap_or_default()),
                group.scenes.join("、")
            )?,
            Level::Route => writeln!(
                out,
                "{}（シーン {}）",
                group
                    .id
                    .as_deref()
                    .map_or("ルートなし".to_owned(), |r| format!("ルート {r}")),
                group.scenes.join("、")
            )?,
        }
        for edge in &group.exits {
            writeln!(out, "  {}", edge_text(dir, graph.level, edge))?;
        }
        if !group.endings.is_empty() {
            writeln!(out, "  @エンディング {}", group.endings.join("、"))?;
        }
        if !group.reads.is_empty() {
            writeln!(out, "  読む：{}", group.reads.join("、"))?;
        }
        if !group.writes.is_empty() {
            writeln!(out, "  書く：{}", group.writes.join("、"))?;
        }
    }
    Ok(())
}

/// 辺を1行にする（`-> s014（選択肢「黙ってうなずく」 if 勇気 >= 2）`）。章とルートの粒度では、
/// 出るシーンと、行き先のまとまりも書く。
fn edge_text(dir: &Path, level: Level, edge: &GraphEdge) -> String {
    let arrow = if edge.kind == "call" { "@呼出" } else { "->" };
    let target = match level {
        Level::Scene => format!("{arrow} {}", edge.to),
        Level::Chapter | Level::Route => format!("{} {arrow} {}", edge.from, edge.to),
    };
    let cond = edge
        .cond_text
        .as_deref()
        .map(|c| format!(" if {c}"))
        .unwrap_or_default();
    let via = match edge.via {
        "choice" => format!(
            "（選択肢「{}」{cond}）",
            edge.label.as_deref().unwrap_or_default()
        ),
        // `if` は `@if` と `@elif` の両方の枝（いちばん内側の文脈だけ。D044）。
        "if" => format!("（条件 {}）", edge.cond_text.as_deref().unwrap_or_default()),
        "else" => "（@else の枝）".into(),
        _ => String::new(),
    };
    let group = match (level, &edge.to_group) {
        (_, _) if !edge.to_exists => " ※シーンがありません".to_owned(),
        (Level::Scene, _) => String::new(),
        (Level::Chapter, Some(file)) => format!(" → {}", display_path(dir, file)),
        (Level::Route, Some(route)) => format!(" → ルート {route}"),
        (_, None) => " → ルートなし".to_owned(),
    };
    format!("{target}{via}{group}")
}

// ---- scn new ----

fn run_new(dir: &Path, title: Option<&str>) -> Result<u8, CliError> {
    // 作品名を省くとフォルダ名（`scn new .` でも、今のフォルダの名前にする）。
    let title = title.map_or_else(
        || {
            std::path::absolute(dir)
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "作品".into())
        },
        str::to_owned,
    );
    create_project(dir, &title).map_err(CliError::Project)?;
    eprintln!(
        "作品フォルダを作りました：{}（作品名：{title}）",
        dir.display()
    );
    Ok(SUCCESS)
}

// ---- scn format-doc ----

fn run_format_doc(dir: &Path, check: bool) -> Result<u8, CliError> {
    // 作品フォルダでなければ（`project.toml` が無ければ）書かない。
    load_project(dir).map_err(CliError::Project)?;
    let shown = |name: &str| display_path(dir, name);
    if check {
        let stale = stale_docs(dir);
        for name in scn_core::scaffold::GENERATED_DOCS {
            if stale.contains(&name) {
                eprintln!(
                    "{} は、今のツールの版と違います（scn format-doc で書き直します）",
                    shown(name)
                );
            } else {
                eprintln!("{} は、今のツールの版と同じです", shown(name));
            }
        }
        return Ok(if stale.is_empty() {
            SUCCESS
        } else {
            HAS_ERRORS
        });
    }
    let written = write_generated_docs(dir).map_err(CliError::Project)?;
    for name in scn_core::scaffold::GENERATED_DOCS {
        if written.contains(&name) {
            eprintln!("{} を書き直しました", shown(name));
        } else {
            eprintln!(
                "{} は、今のツールの版と同じです（書き直していません）",
                shown(name)
            );
        }
    }
    Ok(SUCCESS)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    /// コマンドの使い方の説明（`docs/guide/scn.md`。作品フォルダの `SCN.md`。T4-8、D082）。
    const GUIDE: &str = include_str!("../../../docs/guide/scn.md");

    /// 完了条件：すべてのコマンドと長いオプションが、使い方の説明にある（コマンドやオプションを足したら説明も直す）。
    #[test]
    fn guide_covers_every_command_and_option() {
        fn walk(command: &clap::Command, path: &str, missing: &mut Vec<String>) {
            for sub in command.get_subcommands() {
                let name = sub.get_name();
                if name == "help" {
                    continue;
                }
                let full = format!("{path} {name}");
                // `scn export epub|pdf` のようにまとめて書いたものも数える。
                let written = GUIDE.contains(&format!("`{full}"))
                    || GUIDE.contains(&format!("{path} {name}|"))
                    || GUIDE.contains(&format!("|{name} ["));
                if !written {
                    missing.push(full.clone());
                }
                for arg in sub.get_arguments() {
                    if let Some(long) = arg.get_long()
                        && long != "help"
                        && !GUIDE.contains(&format!("--{long}"))
                    {
                        missing.push(format!("{full} --{long}"));
                    }
                }
                walk(sub, &full, missing);
            }
        }
        let mut missing = Vec::new();
        walk(&Cli::command(), "scn", &mut missing);
        for arg in PluginArgs::command().get_arguments() {
            if let Some(long) = arg.get_long()
                && long != "help"
                && !GUIDE.contains(&format!("--{long}"))
            {
                missing.push(format!("scn export <プロファイル名> --{long}"));
            }
        }
        assert!(GUIDE.contains("`scn export <プロファイル名>"));
        assert!(missing.is_empty(), "説明に無い：{missing:?}");
    }
}
