//! 1万行の合成データの解析時間を測る（`.claude/rules/rust.md` の目安：リリースビルドで100ミリ秒以内）。
//!
//! 実行：`cargo bench -p scn-core --bench analyze`

use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use scn_core::project::SourceFile;
use scn_core::{analyze, load_project, parse};

/// 解析する行数。
const LINES: usize = 10_000;

/// サンプル作品の書き方をまねたシーンを並べて、`LINES` 行の `.scn` を作る。
fn synthetic_source() -> String {
    let mut out = String::from("# 第1章 合成データ\n\n");
    let mut line_id = 0usize;
    let mut scene = 0;
    while out.lines().count() < LINES {
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
    out.push_str(&format!("## 終わり #s{}\n@エンディング end1\n", scene + 1));
    out
}

fn bench(c: &mut Criterion) {
    let source = synthetic_source();
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago");
    let mut project = load_project(&sample).expect("サンプルを読めない");
    let options = project.parse_options();
    project.files = vec![SourceFile {
        path: "story/synthetic.scn".into(),
        text: source.clone(),
    }];

    // 合成データがエラー無く読めることを確かめてから測る。
    let ir = analyze(&project);
    assert!(
        ir.diagnostics.iter().all(|d| d.severity != "error"),
        "合成データにエラーがある"
    );
    eprintln!(
        "合成データ：{} 行、{} シーン、診断 {} 件",
        source.lines().count(),
        ir.scenes.len(),
        ir.diagnostics.len()
    );

    c.bench_function("parse 1万行", |b| {
        b.iter(|| parse(black_box(&source), &options))
    });
    c.bench_function(
        "analyze 1万行（構文解析・意味解析・IR）",
        |b| b.iter(|| analyze(black_box(&project))),
    );
}

criterion_group!(benches, bench);
criterion_main!(benches);
