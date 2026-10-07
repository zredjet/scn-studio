//! 合成データの解析時間を測る。
//!
//! - 1万行（1ファイル）：`.claude/rules/rust.md` の目安（リリースビルドで100ミリ秒以内）
//! - 10万行（20ファイル、約3200シーン）：打鍵ごとに全体を解析し直してよいかの判断（T1-3、D040）。
//!   解析、索引、IRのJSONへの直列化を測る。JSONは `CARGO_TARGET_TMPDIR` に書き出す（フロントでの読み込みを別に測るため）
//!
//! 実行：`cargo bench -p scn-core --bench analyze`

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use scn_core::index::build_index;
use scn_core::project::{Project, SourceFile};
use scn_core::{analyze, load_project, parse};

/// サンプル作品の書き方をまねたシーンを並べて、`lines` 行ほどの `.scn` を作る。
/// シーンIDと行IDは `first_scene` 番から振り、最後のシーンは次のファイルの最初のシーンへ飛ぶ。
/// `last` なら、最後にエンディングのシーンを置く。
fn synthetic_source(lines: usize, first_scene: usize, last: bool) -> (String, usize) {
    let mut out = String::from("# 第1章 合成データ\n\n");
    let mut line_id = first_scene * 100;
    let mut scene = first_scene - 1;
    while out.lines().count() < lines {
        scene += 1;
        let mut id = || {
            line_id += 1;
            format!("^{line_id:06x}")
        };
        let lines = [
            format!("## 合成シーン{scene} #s{scene}"),
            "---".into(),
            format!("summary: 合成データの{scene}番目のシーン"),
            "status: draft".into(),
            "route: 共通".into(),
            "---".into(),
            "@背景 屋上/夕方 フェード".into(),
            "@BGM 切ない".into(),
            "@立ち絵 灯 笑顔 左".into(),
            String::new(),
            format!("灯「やっと来た。遅いよ」 {}", id()),
            format!("\u{3000}風が強くて、灯の髪が|乱《みだ》れていた。 {}", id()),
            format!("灯「あのね、{{w 0.5}}話があるんだ」 {}", id()),
            format!("湊「どうしたの」 {}", id()),
            format!("灯『……やっぱり、言えないかも』 {}", id()),
            String::new(),
            format!("@選択 どう答える {}", id()),
            format!("- 「ごめん、遅くなって」 {}", id()),
            "  @設定 灯.好感度 += 1".into(),
            format!("- 黙ってうなずく if 勇気 >= 2 {}", id()),
            format!("  灯「……そういうところ、ずるいよね」 {}", id()),
            "  @設定 勇気 -= 1".into(),
            "@end".into(),
            String::new(),
            "@if 灯.好感度 >= 3 and not flag.ルート == \"紬\"".into(),
            format!("  灯「やっぱり、君には言っておく」 {}", id()),
            "@else".into(),
            format!("  \u{3000}灯は黙っていた。 {}", id()),
            "@end".into(),
            format!("-> s{}", scene + 1),
            String::new(),
        ];
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if last {
        out.push_str(&format!("## 終わり #s{}\n@エンディング end1\n", scene + 1));
    }
    (out, scene + 1)
}

/// サンプル作品の設定に、合成した本文を `files` ファイル、全体で `lines` 行ほど入れる。
fn synthetic_project(lines: usize, files: usize) -> Project {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let mut project = load_project(&sample).expect("サンプルを読めない");
    project.files.clear();
    let mut next_scene = 1;
    for i in 0..files {
        let (text, next) = synthetic_source(lines / files, next_scene, i + 1 == files);
        next_scene = next;
        project.files.push(SourceFile {
            path: format!("story/synthetic{i:02}.scn"),
            text,
        });
    }
    // 合成データがエラー無く読めることを確かめてから測る。
    let ir = analyze(&project);
    assert!(
        ir.diagnostics.iter().all(|d| d.severity != "error"),
        "合成データにエラーがある"
    );
    eprintln!(
        "合成データ：{} 行、{} ファイル、{} シーン、診断 {} 件",
        project
            .files
            .iter()
            .map(|f| f.text.lines().count())
            .sum::<usize>(),
        project.files.len(),
        ir.scenes.len(),
        ir.diagnostics.len()
    );
    project
}

fn bench(c: &mut Criterion) {
    let project = synthetic_project(10_000, 1);
    let options = project.parse_options();
    let source = project.files[0].text.clone();

    c.bench_function("parse 1万行", |b| {
        b.iter(|| parse(black_box(&source), &options))
    });
    c.bench_function(
        "analyze 1万行（構文解析・意味解析・IR）",
        |b| b.iter(|| analyze(black_box(&project))),
    );

    let large = synthetic_project(100_000, 20);
    let ir = analyze(&large);
    let json = serde_json::to_string(&ir).expect("IRをJSONにできない");
    eprintln!("10万行のIRのJSON：{} バイト", json.len());
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ir-100k.json");
    std::fs::write(&out, &json).expect("JSONを書き出せない");
    eprintln!("書き出した：{}", out.display());

    let mut group = c.benchmark_group("10万行");
    group.sample_size(20);
    group.bench_function("analyze（構文解析・意味解析・IR）", |b| {
        b.iter(|| analyze(black_box(&large)))
    });
    group.bench_function("索引（IRから）", |b| {
        b.iter(|| build_index(black_box(&large), black_box(&ir)))
    });
    group.bench_function("IRをJSONにする", |b| {
        b.iter(|| serde_json::to_string(black_box(&ir)))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
