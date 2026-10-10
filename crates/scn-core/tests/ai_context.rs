//! AIタスクの定義と文脈の組み立て（T3-6）：サンプルの各タスクの文脈（スナップショット）、削る順番、文体見本を
//! 削らないこと、シーンの要約のキャッシュ、設定DBのエントリが渡る内容。LLM は呼ばない。

use std::fs;
use std::path::PathBuf;

use scn_core::ai::{
    self, Assembled, ContextItem, ContextSources, SummarySource, TRIM_ORDER, TaskLevel, TaskOutput,
};
use scn_core::{Project, analyze, load_project};

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn houkago() -> (Project, ContextSources) {
    let root = sample("houkago");
    let project = load_project(&root).unwrap();
    let sources = ai::read_sources(&root);
    (project, sources)
}

/// 文脈に使える上限が `limit` 以上になる、いちばん短い文脈の長さ。
fn tokens_for(limit: u64) -> u64 {
    let mut ct = limit;
    while ai::context_limit(ct) < limit {
        ct += 1;
    }
    ct
}

/// 組み立てた文脈を、あらましと中身の文にする。
fn dump(a: &Assembled) -> String {
    let mut out = format!(
        "タスク {}、シーン {}、文脈の長さ {}、上限 {}、見積もり {}\n",
        a.task,
        a.scene.as_deref().unwrap_or("なし"),
        a.context_tokens,
        a.limit,
        a.tokens
    );
    for s in &a.sections {
        out.push_str(&format!("節 {}（{}）{}\n", s.item, s.title, s.tokens));
    }
    for t in &a.trimmed {
        out.push_str(&format!(
            "削った {}（{}）{} {}\n",
            t.item, t.title, t.how, t.tokens
        ));
    }
    for n in &a.notes {
        out.push_str(&format!("知らせ {n}\n"));
    }
    if let Some(p) = &a.problem {
        out.push_str(&format!("実行できない {p}\n"));
    }
    out.push_str("\n=== system ===\n");
    out.push_str(&a.system);
    out.push_str("\n\n=== user ===\n");
    out.push_str(&a.user);
    out.push('\n');
    out
}

/// サンプルのタスクの定義を、診断なしで読む。
#[test]
fn sample_tasks_load() {
    for name in ["houkago", "amayadori"] {
        let project = load_project(&sample(name)).unwrap();
        let ai_diags: Vec<_> = project
            .load_diagnostics
            .iter()
            .filter(|d| d.file.starts_with("ai/"))
            .collect();
        assert!(ai_diags.is_empty(), "{name}：{ai_diags:?}");
        for builtin in [
            "branch_ideas",
            "concept_questions",
            "idea_sort",
            "draft_chapter",
            "draft_stub",
            "rephrase",
            "route_skeleton",
            "scene_summary",
            "setting_check",
            "thin_branches",
            "tone_check",
            "wording_check",
        ] {
            assert!(
                project.ai_tasks.iter().any(|t| t.id == builtin),
                "{name} に {builtin} が無い"
            );
        }
    }
    let (project, _) = houkago();
    let ids: Vec<&str> = project.ai_tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids.first(), Some(&"branch_ideas"));
    let route = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_route")
        .unwrap();
    assert_eq!(route.level, TaskLevel::Auto);
    assert_eq!(route.output, TaskOutput::Draft);
    assert_eq!(route.scope.as_deref(), Some("empty_only"));
    assert_eq!(route.checks, ["check", "tone_check", "setting_check"]);
    assert_eq!(
        (route.max_iterations, route.budget_tokens),
        (Some(3), Some(400_000))
    );
    let tone = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    assert_eq!(
        tone.context,
        [
            ContextItem::Scene,
            ContextItem::Characters {
                fields: Some(vec!["speech".to_owned(), "first_person".to_owned()])
            }
        ]
    );
    assert_eq!(tone.model.as_deref(), Some("local"));
}

/// サンプルの各タスクの文脈（接続先の既定の長さで）。シーンを使うタスクは s012（台詞と変数のある分岐の
/// シーン）、スタブの本文を書くタスクは s014（スタブ）で。
#[test]
fn sample_contexts() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    for task in &project.ai_tasks {
        let scene = if task.context.contains(&ContextItem::Scene) {
            Some(if task.id == "draft_stub" {
                "s014"
            } else {
                "s012"
            })
        } else {
            None
        };
        let model = task.model.as_deref().unwrap_or("local");
        let decl = project
            .settings
            .ai
            .iter()
            .find(|d| d.name == model)
            .unwrap();
        let conn = ai::resolve(decl, &ai::MachineSettings::default()).unwrap();
        let assembled = ai::assemble(&project, &ir, &sources, task, scene, conn.context_tokens);
        assert_eq!(assembled.problem, None, "{}", task.id);
        assert!(assembled.trimmed.is_empty(), "{}", task.id);
        insta::assert_snapshot!(format!("houkago_{}", task.id), dump(&assembled));
    }
}

/// 入りきらなければ、決めた順に削る（前のシーンまでの要約 → … → 用語 → キャラクターを短く → 削る → 変数）。
/// 今のシーンは削らない。
#[test]
fn trims_in_order() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "setting_check")
        .unwrap();
    let full = ai::assemble(&project, &ir, &sources, task, Some("s012"), 200_000);
    assert!(full.trimmed.is_empty());
    let scene = full
        .sections
        .iter()
        .find(|s| s.item == "scene")
        .unwrap()
        .tokens;
    let mut previous: Vec<&str> = Vec::new();
    // 上限を少しずつ下げて、削る順番が TRIM_ORDER に沿うことを見る。
    let mut limits: Vec<u64> = (1..40)
        .map(|i| full.tokens.saturating_sub(i * 60))
        .collect();
    limits.push(scene + 10);
    for limit in limits {
        let a = ai::assemble(
            &project,
            &ir,
            &sources,
            task,
            Some("s012"),
            tokens_for(limit),
        );
        let order: Vec<&str> = a.trimmed.iter().map(|t| t.item).collect();
        let mut expected: Vec<&str> = TRIM_ORDER
            .iter()
            .copied()
            .filter(|i| full.sections.iter().any(|s| s.item == *i))
            .collect();
        expected.dedup();
        // 削った順は、TRIM_ORDER のうち文脈にある項目の順（characters は短くしてから削る）。
        let mut i = 0;
        for item in &order {
            while expected.get(i).is_some_and(|e| e != item) {
                i += 1;
            }
            assert!(expected.get(i).is_some(), "順番が違う：{order:?}");
        }
        assert!(
            a.sections.iter().any(|s| s.item == "scene"),
            "シーンを削った"
        );
        assert!(
            order.len() >= previous.len(),
            "上限を下げたのに削った数が減った"
        );
        assert_eq!(&order[..previous.len()], previous.as_slice());
        previous = order;
        if a.problem.is_none() {
            assert!(a.tokens <= a.limit);
        }
    }
    let shortened = previous.iter().position(|i| *i == "characters").unwrap();
    assert_eq!(previous.get(shortened + 1), Some(&"characters"));
    assert_eq!(
        previous.first().copied(),
        Some("previous_summaries"),
        "最初に前のシーンまでの要約を削る"
    );
    // シーンより短い上限では、実行できない。
    let a = ai::assemble(&project, &ir, &sources, task, Some("s012"), 100);
    assert!(a.problem.as_deref().unwrap().contains("入りきりません"));
    assert!(a.sections.iter().any(|s| s.item == "scene"));
    insta::assert_snapshot!(
        "houkago_setting_check_trimmed",
        dump(&ai::assemble(
            &project,
            &ir,
            &sources,
            task,
            Some("s012"),
            tokens_for(scene + 600)
        ))
    );
}

/// 生成のタスクでは文体（文体見本）を削らない。入りきらなければ実行せずに知らせる。指摘のタスクでは削れる。
#[test]
fn style_is_kept_for_generation() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let draft = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_stub")
        .unwrap();
    let full = ai::assemble(&project, &ir, &sources, draft, Some("s014"), 200_000);
    let style = full
        .sections
        .iter()
        .find(|s| s.item == "style")
        .unwrap()
        .tokens;
    let scene = full
        .sections
        .iter()
        .find(|s| s.item == "scene")
        .unwrap()
        .tokens;
    let a = ai::assemble(
        &project,
        &ir,
        &sources,
        draft,
        Some("s014"),
        (style + scene) * 2,
    );
    assert!(a.sections.iter().any(|s| s.item == "style"));
    assert!(!a.trimmed.iter().any(|t| t.item == "style"));
    let problem = a.problem.as_deref().unwrap();
    assert!(problem.contains("文体"), "{problem}");
    insta::assert_snapshot!("houkago_draft_stub_too_long", dump(&a));

    let check = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "wording_check")
        .unwrap();
    let full = ai::assemble(&project, &ir, &sources, check, Some("s012"), 200_000);
    let scene = full
        .sections
        .iter()
        .find(|s| s.item == "scene")
        .unwrap()
        .tokens;
    let a = ai::assemble(
        &project,
        &ir,
        &sources,
        check,
        Some("s012"),
        tokens_for(scene + 900),
    );
    assert!(
        a.trimmed.iter().any(|t| t.item == "style"),
        "{:?}",
        a.trimmed
    );
    assert_eq!(a.problem, None);
}

#[test]
fn needs_scene_and_prompt() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let tone = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    let a = ai::assemble(&project, &ir, &sources, tone, None, 8192);
    assert!(a.problem.as_deref().unwrap().contains("シーンを選びます"));
    let a = ai::assemble(&project, &ir, &sources, tone, Some("s999"), 8192);
    assert_eq!(a.problem.as_deref(), Some("シーン s999 がありません"));
    let mut no_prompt = tone.clone();
    no_prompt.prompt = None;
    let a = ai::assemble(&project, &ir, &sources, &no_prompt, Some("s012"), 8192);
    assert!(a.problem.as_deref().unwrap().contains("prompt"));
    // シーンを使わないタスクの前のシーンまでの要約は、知らせて入れない。
    let mut previous = tone.clone();
    previous.context = vec![ContextItem::PreviousSummaries];
    let a = ai::assemble(&project, &ir, &sources, &previous, None, 8192);
    assert_eq!(a.problem, None);
    assert!(a.sections.is_empty());
    assert!(a.notes.iter().any(|n| n.contains("前のシーンまでの要約")));
}

/// 返す分の上限（T4-10）：書けば、文脈の上限は「文脈の長さ − 返す分の上限」。文脈の長さ以上なら実行できない。
/// 書かなければ今までと同じ（4分の1、多くて4096）。
#[test]
fn output_budget() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let tone = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap();
    let budget = |context_tokens, output_tokens| ai::TokenBudget {
        context_tokens,
        output_tokens,
    };
    let a = ai::assemble(&project, &ir, &sources, tone, Some("s012"), 8192);
    assert_eq!((a.output_tokens, a.limit), (2048, 6144));
    let a = ai::assemble(&project, &ir, &sources, tone, Some("s012"), 262_144);
    assert_eq!((a.output_tokens, a.limit), (4096, 258_048));
    let a = ai::assemble(
        &project,
        &ir,
        &sources,
        tone,
        Some("s012"),
        budget(262_144, Some(16384)),
    );
    assert_eq!((a.output_tokens, a.limit), (16384, 245_760));
    assert_eq!(a.problem, None);
    for output in [8192, 9000] {
        let a = ai::assemble(
            &project,
            &ir,
            &sources,
            tone,
            Some("s012"),
            budget(8192, Some(output)),
        );
        let problem = a.problem.unwrap();
        assert!(problem.contains("文脈に使える分が残りません"), "{problem}");
        assert!(a.sections.is_empty());
    }
}

/// シーンの要約は、キャッシュ（本文のハッシュが合うとき）→ メタ情報の summary → 題名の順に使う。
#[test]
fn summary_cache() {
    let root = std::env::temp_dir().join(format!("scn-summary-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("story")).unwrap();
    fs::write(
        root.join("project.toml"),
        "format_version = 0\nstart_scene = \"s1\"\n",
    )
    .unwrap();
    fs::write(
        root.join("story/a.scn"),
        "## 始まり #s1\n---\nsummary: 二人が出会う\n---\n灯「やあ」 ^aaaa\n-> s2\n\n## 続き #s2\n地の文。 ^bbbb\n",
    )
    .unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let mut sources = ai::read_sources(&root);
    let summaries = ai::scene_summaries(&project, &ir, &sources.summaries);
    assert_eq!(
        summaries
            .iter()
            .map(|s| (s.summary.as_str(), s.source))
            .collect::<Vec<_>>(),
        [
            ("二人が出会う", SummarySource::Meta),
            ("", SummarySource::Title)
        ]
    );
    let source = ai::scene_source(&project, &ir, "s1").unwrap();
    assert!(
        source.starts_with("## 始まり #s1") && source.ends_with("-> s2\n\n"),
        "{source:?}"
    );
    ai::store_summary(
        &root,
        &mut sources.summaries,
        "s1",
        source,
        " 灯が声をかける。\n",
    )
    .unwrap();
    let reread = ai::read_sources(&root);
    assert_eq!(reread.summaries, sources.summaries);
    let s1 = ai::scene_summaries(&project, &ir, &reread.summaries).remove(0);
    assert_eq!(
        (s1.summary.as_str(), s1.source, s1.stale),
        ("灯が声をかける。", SummarySource::Cache, false)
    );

    // 行IDを付けただけ（保存）なら、要約は古くならない。
    assert_eq!(
        ai::source_hash("灯「やあ」 ^aaaa\n地の文。\t\n"),
        ai::source_hash("灯「やあ」\n地の文。\t\n")
    );
    assert_ne!(
        ai::source_hash("灯「やあ」\n"),
        ai::source_hash("灯「やあ、湊」\n")
    );
    assert_ne!(ai::source_hash("a ^Not\n"), ai::source_hash("a\n"));
    // CRLF の本文でも（行IDは `\r` の前に付く）。
    assert_eq!(
        ai::source_hash("灯「やあ」 ^aaaa\r\n地の文。\r\n"),
        ai::source_hash("灯「やあ」\r\n地の文。\r\n")
    );
    // 本文が変わったら、キャッシュを使わずにメタ情報へ戻る（作り直す印を付ける）。
    fs::write(
        root.join("story/a.scn"),
        "## 始まり #s1\n---\nsummary: 二人が出会う\n---\n灯「やあ、湊」 ^aaaa\n-> s2\n\n## 続き #s2\n地の文。 ^bbbb\n",
    )
    .unwrap();
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let s1 = ai::scene_summaries(&project, &ir, &reread.summaries).remove(0);
    assert_eq!(
        (s1.summary.as_str(), s1.source, s1.stale),
        ("二人が出会う", SummarySource::Meta, true)
    );

    // 前のシーンまでの要約と、作り直す知らせ。
    let mut task = project.ai_tasks.first().cloned().unwrap_or_else(|| {
        let root =
            yaml_serde::from_str("name: x\ncontext: [previous_summaries]\nprompt: p\n").unwrap();
        ai::parse_task("x", "ai/x.yaml", &root).0
    });
    task.context = vec![ContextItem::PreviousSummaries];
    let a = ai::assemble(&project, &ir, &reread, &task, Some("s2"), 8192);
    let previous = a
        .sections
        .iter()
        .find(|s| s.item == "previous_summaries")
        .unwrap();
    assert!(
        previous.text.ends_with("- s1「始まり」：二人が出会う"),
        "{}",
        previous.text
    );
    assert!(
        a.notes
            .iter()
            .any(|n| n.contains("作り直す") && n.contains("s1")),
        "{:?}",
        a.notes
    );
    let _ = fs::remove_dir_all(&root);
}

/// 設定DBの画面の「AIに渡す内容」：キャラクターは登場するシーンで、タスクごとに決めた項目が渡る。用語は言及された
/// シーンで全部が渡る。
#[test]
fn entry_contexts() {
    let (project, _) = houkago();
    let akari = ai::entry_context(&project, "bible/entries/character/akari.md").unwrap();
    assert!(akari.character);
    let labels: Vec<&str> = akari.parts.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels.first(), Some(&"呼び名"));
    assert_eq!(labels.last(), Some(&"本文"));
    let tone = akari.tasks.iter().find(|t| t.id == "tone_check").unwrap();
    assert_eq!(tone.labels, ["一人称", "口調メモ"]);
    let speech = akari
        .parts
        .iter()
        .find(|p| p.label == "口調メモ")
        .unwrap()
        .chars;
    assert_eq!(tone.chars, speech + "わたし".chars().count());
    let setting = akari
        .tasks
        .iter()
        .find(|t| t.id == "setting_check")
        .unwrap();
    assert_eq!(
        setting.chars,
        akari.parts.iter().map(|p| p.chars).sum::<usize>()
    );
    assert!(
        !akari
            .tasks
            .iter()
            .any(|t| t.id == "scene_summary" && t.labels.contains(&"口調メモ".to_owned()))
    );

    let school =
        ai::entry_context(&project, "bible/entries/organization/kitakaze_high.md").unwrap();
    assert!(!school.character);
    let ids: Vec<&str> = school.tasks.iter().map(|t| t.id.as_str()).collect();
    assert!(
        ids.contains(&"setting_check") && ids.contains(&"draft_stub"),
        "{ids:?}"
    );
    assert!(!ids.contains(&"tone_check"));
    assert_eq!(ai::entry_context(&project, "bible/entries/none.md"), None);
}

/// サンプルの組み込みのタスクは、`scn new` のひな形と同じ中身にしておく。
#[test]
fn builtin_tasks_match_templates() {
    let templates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/ai");
    let mut names: Vec<_> = fs::read_dir(&templates)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names.len(), 15);
    for name in names {
        let template = fs::read_to_string(templates.join(&name)).unwrap();
        for sample_name in ["houkago", "amayadori"] {
            let copy = fs::read_to_string(sample(sample_name).join("ai").join(&name)).unwrap();
            assert_eq!(
                copy,
                template,
                "{sample_name}/ai/{}",
                name.to_string_lossy()
            );
        }
    }
}

/// 言及された用語：名前か呼び名が今のシーンの本文に出るエントリだけ（s021 の「髪留め」。北風高校は出ない）。
#[test]
fn terms_mentioned_in_scene() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "setting_check")
        .unwrap();
    let a = ai::assemble(&project, &ir, &sources, task, Some("s021"), 200_000);
    let terms = a.sections.iter().find(|s| s.item == "terms").unwrap();
    assert!(terms.text.contains("### 髪留め（伏線）"), "{}", terms.text);
    assert!(terms.text.contains("母の形見"), "{}", terms.text);
    assert!(!terms.text.contains("北風高校"), "{}", terms.text);
    // シーンを選ばなければ、用語は全部。
    let mut all = task.clone();
    all.context = vec![ContextItem::Terms];
    let a = ai::assemble(&project, &ir, &sources, &all, None, 200_000);
    let terms = a.sections.iter().find(|s| s.item == "terms").unwrap();
    assert!(terms.text.contains("髪留め") && terms.text.contains("北風高校"));
}

/// 話者も `@立ち絵` も無いスタブでは、本文（メタ情報の summary を含む）に名前か呼び名が出るキャラクター。
#[test]
fn stub_characters_by_mention() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_stub")
        .unwrap();
    let a = ai::assemble(&project, &ir, &sources, task, Some("s014"), 200_000);
    let characters = a.sections.iter().find(|s| s.item == "characters").unwrap();
    // s014 の summary「黙ってうなずいた主人公の隣に、灯は…」：主人公（湊の呼び名）、灯の順。
    let minato = characters.text.find("### 湊（minato）").unwrap();
    let akari = characters.text.find("### 灯（akari）").unwrap();
    assert!(minato < akari, "{}", characters.text);
}

/// 短くする段は、書いた項目のうちの一人称と口調メモだけにする（頼んでいない項目を入れない）。
#[test]
fn shortening_respects_fields() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let mut task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "tone_check")
        .unwrap()
        .clone();
    task.context = vec![
        ContextItem::Scene,
        ContextItem::Characters {
            fields: Some(vec!["body".to_owned()]),
        },
    ];
    let full = ai::assemble(&project, &ir, &sources, &task, Some("s012"), 200_000);
    let characters = full
        .sections
        .iter()
        .find(|s| s.item == "characters")
        .unwrap();
    assert!(
        characters.text.contains("転校が決まった"),
        "{}",
        characters.text
    );
    assert!(!characters.text.contains("一人称"), "{}", characters.text);
    let scene = full
        .sections
        .iter()
        .find(|s| s.item == "scene")
        .unwrap()
        .tokens;
    let a = ai::assemble(
        &project,
        &ir,
        &sources,
        &task,
        Some("s012"),
        tokens_for(scene + 300),
    );
    if let Some(kept) = a.sections.iter().find(|s| s.item == "characters") {
        assert!(!kept.text.contains("一人称"), "{}", kept.text);
        assert!(!kept.text.contains("口調メモ"), "{}", kept.text);
    }
}

/// 生成と自律では、`context` に書いていなくても文体を入れる。`style.md` が無ければ実行しない。
#[test]
fn generation_always_has_style() {
    let (project, sources) = houkago();
    let ir = analyze(&project);
    let mut task = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "draft_stub")
        .unwrap()
        .clone();
    task.context = vec![ContextItem::Scene];
    let a = ai::assemble(&project, &ir, &sources, &task, Some("s014"), 200_000);
    assert!(a.sections.iter().any(|s| s.item == "style"));
    assert_eq!(a.problem, None);
    let mut no_style = sources.clone();
    no_style.style = None;
    let a = ai::assemble(&project, &ir, &no_style, &task, Some("s014"), 200_000);
    assert!(
        a.problem.as_deref().unwrap().contains("style.md"),
        "{:?}",
        a.problem
    );
    // 指摘のタスクは、文体が無くても実行できる。
    let check = project
        .ai_tasks
        .iter()
        .find(|t| t.id == "wording_check")
        .unwrap();
    let a = ai::assemble(&project, &ir, &no_style, check, Some("s012"), 200_000);
    assert_eq!(a.problem, None);
}
