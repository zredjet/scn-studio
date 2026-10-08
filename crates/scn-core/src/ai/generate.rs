//! 生成（T3-9、D070）：スタブのシーン（台詞も地の文も無いシーン）に、AI の下書きの本文を書き込む。
//!
//! - 書き込むのは本文の行（台詞、地の文、演出の命令 `@背景`・`@立ち絵`・`@退場`・`@BGM`・`@SE`・`@効果`・`@待機`）
//!   だけ。見出し・メタ情報・選択肢・条件・ジャンプ・`@設定`・`@呼出`・`@エンディング`・`@raw`・コメント、行IDの
//!   付いた行（今ある行を写したもの）は、応答にあっても書かない（人が作ったスタブの構造を変えない）。
//! - 差し込む場所は、メタ情報（無ければ見出し）とスタブの演出の命令の後ろ、最初の流れの行（ジャンプ・選択肢・
//!   条件・呼び出し・エンディングなど）の手前。今ある行は1バイトも変えない。
//! - シーンのメタ情報の `status` を `ai-draft` にする（無ければ足す）。スタブのメタ情報のうち、書き換えるのは
//!   この1行だけ。
//! - 差し込んだ行にだけ行IDを付ける（[`with_line_ids`]）。出自の記録はワークスペースが書く。

use std::collections::BTreeSet;

use crate::ir::Ir;
use crate::line_id::{LineIdGenerator, assign_line_ids, insert};
use crate::project::Project;

/// 書き込んでよい演出の命令（日本語名と英語名）。
const PRESENTATION: [&str; 14] = [
    "背景",
    "bg",
    "立ち絵",
    "show",
    "退場",
    "hide",
    "BGM",
    "bgm",
    "SE",
    "se",
    "効果",
    "fx",
    "待機",
    "wait",
];

/// 流れの行（差し込む場所の目印）。
fn is_flow(head: &str) -> bool {
    if head.starts_with("->") || head.starts_with("- ") || head == "-" {
        return true;
    }
    let Some(directive) = head.strip_prefix('@') else {
        return false;
    };
    let name = directive.split_whitespace().next().unwrap_or_default();
    !PRESENTATION.contains(&name)
}

/// 行末に行ID（` ^xxxx`）があるか。
fn has_line_id(line: &str) -> bool {
    let t = line.trim_end_matches([' ', '\t', '\r']);
    t.rsplit_once(" ^").is_some_and(|(_, id)| {
        (4..=12).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// 応答の文から、本文を読む部分：コードブロック（```）があれば最初のブロックの中だけ（前置きや後書きを
/// 本文にしない）。
fn response_body(text: &str) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    let fence = |l: &&str| l.trim_start().starts_with("```");
    match lines.iter().position(fence) {
        Some(open) => {
            let rest = lines.get(open + 1..).unwrap_or_default();
            let close = rest.iter().position(fence).unwrap_or(rest.len());
            rest.get(..close).unwrap_or_default().to_vec()
        }
        None => lines,
    }
}

/// 応答の文の行末の、点検のために付けた仮の行ID（`temp`）を除く（自律の直しの応答。T3-9）。
pub fn without_temp_ids(text: &str, temp: &[String]) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_end_matches([' ', '\t', '\r']);
            match trimmed.rsplit_once(" ^") {
                Some((rest, id)) if temp.iter().any(|t| t == id) => rest.trim_end().to_owned(),
                _ => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 応答の文（`.scn` の形）から、書き込む本文の行を取る。コードブロックがあればその中だけ、2つ目のシーンの見出し
/// より後ろは読まない。構造の行、ブロックの中、行IDの付いた行は除く。前後の空行は除き、続く空行は1つにまとめる。
pub fn generated_body(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut in_meta = false;
    let mut after_heading = false;
    let mut headings = 0usize;
    for raw in response_body(text) {
        let line = raw.trim_end_matches([' ', '\t', '\r']);
        let head = line.trim_start_matches([' ', '\t']);
        if in_meta {
            if head == "---" {
                in_meta = false;
            }
            continue;
        }
        if head.starts_with("## ") || head.starts_with("# ") {
            // 2つ目のシーン（か、本文の後ろの見出し）からは読まない。
            if headings > 0 || !out.is_empty() {
                break;
            }
            headings += 1;
            after_heading = true;
            continue;
        }
        if after_heading && head == "---" {
            in_meta = true;
            after_heading = false;
            continue;
        }
        if !head.is_empty() {
            after_heading = false;
        }
        if depth > 0 {
            if head == "@end" {
                depth -= 1;
            } else if ["@選択", "@choice", "@if", "@raw"]
                .iter()
                .any(|d| head.starts_with(d))
            {
                depth += 1;
            }
            continue;
        }
        if ["@選択", "@choice", "@if", "@raw"]
            .iter()
            .any(|d| head.starts_with(d))
        {
            depth += 1;
            continue;
        }
        if head.starts_with("//") || head == "@end" || is_flow(head) || has_line_id(head) {
            continue;
        }
        if head.is_empty() {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        out.push(head.to_owned());
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// シーンの本文の範囲（見出しの行の始まりから、同じファイルの次の見出しの手前まで）。
pub fn scene_range(project: &Project, ir: &Ir, scene: &str) -> Option<(String, usize, usize)> {
    let this = ir.scenes.iter().find(|s| s.id == scene)?;
    let file = project.files.iter().find(|f| f.path == this.file)?;
    let end = ir
        .scenes
        .iter()
        .filter(|s| s.file == this.file && s.span.start > this.span.start)
        .map(|s| s.span.start)
        .min()
        .unwrap_or(file.text.len());
    Some((this.file.clone(), this.span.start, end))
}

/// 下書きを差し込んだシーンの本文と、差し込んだ範囲（シーンの本文の中のバイト位置）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drafted {
    pub text: String,
    pub inserted: (usize, usize),
}

/// スタブのシーンの本文（見出しから）に、下書きの行を差し込み、メタ情報の `status` を `ai-draft` にする。
/// 改行はシーンの本文にそろえる。下書きの初めの、スタブに今ある行と同じ行（写して返した演出の命令など）は
/// 差し込まない（途中で同じ演出に戻る行は差し込む）。差し込む行が無い、メタ情報が閉じていない（E102）なら、
/// そのわけ。
pub fn draft_into_scene(scene_text: &str, body: &[String]) -> Result<Drafted, String> {
    let existing: BTreeSet<&str> = scene_text
        .lines()
        .map(|l| l.trim_matches([' ', '\t', '\r']))
        .filter(|l| !l.is_empty())
        .collect();
    let copied = body
        .iter()
        .take_while(|l| l.is_empty() || existing.contains(l.trim_matches([' ', '\t'])))
        .count();
    let mut body: Vec<String> = body.get(copied..).unwrap_or_default().to_vec();
    while body.first().is_some_and(String::is_empty) {
        body.remove(0);
    }
    while body.last().is_some_and(String::is_empty) {
        body.pop();
    }
    if body.is_empty() {
        return Err("書き込む本文の行がありません".to_owned());
    }
    let nl = if scene_text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines: Vec<&str> = scene_text.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut i = 0;
    // 見出し。
    if let Some(heading) = lines.first() {
        out.push_str(heading);
        if !heading.ends_with('\n') {
            out.push_str(nl);
        }
        i = 1;
    }
    // メタ情報（あれば status を書き換えるか足す。無ければ作る）。
    let is_fence = |l: &str| l.trim_end_matches(['\r', '\n']).trim() == "---";
    if lines.get(i).is_some_and(|l| is_fence(l)) {
        // 閉じの `---` が無ければ（E102）、どこまでがメタ情報か分からないので書かない。
        if !lines.iter().skip(i + 1).any(|l| is_fence(l)) {
            return Err("メタ情報が閉じていない（E102）ので、書き込みません".to_owned());
        }
        out.push_str(lines.get(i).copied().unwrap_or_default());
        i += 1;
        let mut wrote = false;
        while let Some(line) = lines.get(i) {
            if is_fence(line) {
                if !wrote {
                    out.push_str(&format!("status: ai-draft{nl}"));
                }
                out.push_str(line);
                if !line.ends_with('\n') {
                    out.push_str(nl);
                }
                i += 1;
                break;
            }
            let key = line.split(':').next().unwrap_or_default().trim();
            if key == "status" {
                let end = if line.ends_with("\r\n") {
                    "\r\n"
                } else if line.ends_with('\n') {
                    "\n"
                } else {
                    ""
                };
                out.push_str(&format!("status: ai-draft{end}"));
                wrote = true;
            } else {
                out.push_str(line);
            }
            i += 1;
        }
    } else {
        out.push_str(&format!("---{nl}status: ai-draft{nl}---{nl}"));
    }
    // スタブの演出の命令と空行は、差し込む前に残す。最初の流れの行（か、シーンの終わりの空行の続き）の手前に差し込む。
    let rest: Vec<&str> = lines.get(i..).unwrap_or_default().to_vec();
    let flow_at = rest
        .iter()
        .position(|l| {
            let head = l.trim().trim_start_matches([' ', '\t']);
            !head.is_empty() && is_flow(head) && !head.starts_with("//")
        })
        .unwrap_or_else(|| {
            // 流れの行が無ければ、終わりの空行の手前。
            let mut end = rest.len();
            while end > 0 && rest.get(end - 1).is_some_and(|l| l.trim().is_empty()) {
                end -= 1;
            }
            end
        });
    for line in rest.get(..flow_at).unwrap_or_default() {
        out.push_str(line);
    }
    if !out.ends_with('\n') {
        out.push_str(nl);
    }
    let start = out.len();
    for line in &body {
        out.push_str(line);
        out.push_str(nl);
    }
    // 流れの行との間に空行を1つ置く。
    if rest.get(flow_at).is_some_and(|l| !l.trim().is_empty()) {
        out.push_str(nl);
    }
    let end = out.len();
    for line in rest.get(flow_at..).unwrap_or_default() {
        out.push_str(line);
    }
    Ok(Drafted {
        text: out,
        inserted: (start, end),
    })
}

/// プロジェクトの `file` の本文 `text` の、`range`（バイト位置）の中で行IDの無い行（I302）にだけ行IDを付ける。
/// 付けた本文と、付けた行IDを返す（ほかの行IDと重ならない）。
pub fn with_line_ids(
    project: &Project,
    file: &str,
    text: &str,
    range: (usize, usize),
    generator: &mut LineIdGenerator,
) -> (String, Vec<String>) {
    let mut trial = project.clone();
    if let Some(f) = trial.files.iter_mut().find(|f| f.path == file) {
        f.text = text.to_owned();
    }
    let Some(found) = assign_line_ids(&trial, generator)
        .into_iter()
        .find(|f| f.path == file)
    else {
        return (text.to_owned(), Vec::new());
    };
    let inside: Vec<_> = found
        .assigned
        .into_iter()
        .filter(|a| a.offset >= range.0 && a.offset <= range.1)
        .collect();
    let ids = inside.iter().map(|a| a.id.clone()).collect();
    (insert(text, &inside), ids)
}

/// スタブのシーン（台詞も地の文も無いシーン）か。
pub fn is_stub(ir: &Ir, scene: &str) -> bool {
    ir.scenes.iter().any(|s| s.id == scene && s.is_stub)
}

/// 章（ファイル）のスタブのシーン（本文の順）。
pub fn stubs_in(ir: &Ir, file: &str) -> Vec<String> {
    ir.scenes
        .iter()
        .filter(|s| s.file == file && s.is_stub)
        .map(|s| s.id.clone())
        .collect()
}

/// 本文に出てくる行IDの集まり（差し込みの前後で、今ある行IDが変わっていないことを確かめる）。
pub fn line_ids_in(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| has_line_id(l))
        .filter_map(|l| {
            l.trim_end_matches([' ', '\t', '\r'])
                .rsplit_once(" ^")
                .map(|(_, id)| id.to_owned())
        })
        .collect()
}

/// AI下書き（`status: ai-draft`）のシーンの本文を省いた IR（T3-9、D070。書き出しと縦書きビューの
/// 「AI下書きを含める」を切ったとき）。シーンの流れ（選択肢・条件・ジャンプ・変数・演出）は残し、台詞・地の文・
/// 段落の区切りを除いて、シーンの初めに「省いた」ことを示す地の文を1つ置く。
pub fn without_ai_drafts(ir: &Ir) -> Ir {
    fn strip(ops: &mut Vec<crate::ir::Op>) {
        use crate::ir::Op;
        ops.retain(|op| !matches!(op, Op::Say { .. } | Op::Narrate { .. } | Op::Break { .. }));
        for op in ops.iter_mut() {
            match op {
                Op::Choice { options, .. } => {
                    for option in options {
                        strip(&mut option.body);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        strip(&mut branch.body);
                    }
                    if let Some(body) = else_body {
                        strip(body);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = ir.clone();
    for scene in &mut out.scenes {
        if scene.meta.status.as_deref() != Some("ai-draft") {
            continue;
        }
        strip(&mut scene.ops);
        scene.ops.insert(
            0,
            crate::ir::Op::Narrate {
                span: scene.span,
                line_id: None,
                text: vec![crate::ir::IrInline::Text {
                    text: format!("（AI下書きのため省いたシーン「{}」）", scene.title),
                }],
            },
        );
    }
    out
}
