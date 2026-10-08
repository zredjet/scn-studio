//! カーソル位置から再生（`route`、T2-4、D055）：サンプルの各シーンのすべての命令について、開始シーンからの
//! 最短経路と変数を求めたスナップショット（完了条件）。プレビューのランタイム（TS）のテスト
//! （`app/test/route.test.ts`）が、このスナップショットの道を速送りして、同じ変数になることを確かめる。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::ir::{Ir, Op};
use scn_core::route::{Route, Value, Variables, route_at, route_to, target_at};
use scn_core::{analyze, load_project};
use serde::Serialize;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

/// IR のすべての命令（文書の順。選択肢の中身と条件の枝の中も）と、そのシーンとファイル。
fn all_ops(ir: &Ir) -> Vec<(String, String, &Op)> {
    fn walk<'a>(ops: &'a [Op], scene: &str, file: &str, out: &mut Vec<(String, String, &'a Op)>) {
        for op in ops {
            out.push((scene.to_owned(), file.to_owned(), op));
            match op {
                Op::Choice { options, .. } => {
                    for option in options {
                        walk(&option.body, scene, file, out);
                    }
                }
                Op::If {
                    branches,
                    else_body,
                    ..
                } => {
                    for branch in branches {
                        walk(&branch.body, scene, file, out);
                    }
                    if let Some(body) = else_body {
                        walk(body, scene, file, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for scene in &ir.scenes {
        walk(&scene.ops, &scene.id, &scene.file, &mut out);
    }
    out
}

/// TS のテストが読む形（命令ごとの道）。
#[derive(Serialize)]
struct Case {
    /// 命令の種類（読みやすさのため）。
    kind: String,
    route: Route,
}

fn routes(name: &str) -> Vec<Case> {
    let root = sample(name);
    let project = load_project(&root).unwrap();
    let ir = analyze(&project);
    let mut cases = Vec::new();
    for (scene, file, op) in all_ops(&ir) {
        let target = target_at(&project, &ir, &file, op.span().start)
            .unwrap_or_else(|| panic!("{scene} の命令から始められない"));
        assert_eq!(
            target.span,
            op.span(),
            "{scene}：行の始まりから、その命令を選ぶ"
        );
        let kind = serde_json::to_value(op).unwrap()["kind"]
            .as_str()
            .unwrap()
            .to_owned();
        cases.push(Case {
            kind,
            route: route_to(&project, &ir, &target, &Variables::new()),
        });
    }
    cases
}

#[test]
fn houkago() {
    let cases = routes("houkago");
    // 灯の好感度は s012 の選択肢で決まり、s013a（灯ルート）へは「すぐに行く」と「ごめん」で着く。
    let s013a = cases
        .iter()
        .find(|c| c.route.target.scene == "s013a")
        .unwrap();
    assert!(s013a.route.reachable);
    assert_eq!(
        s013a
            .route
            .steps
            .iter()
            .map(|s| (s.scene.as_str(), s.option))
            .collect::<Vec<_>>(),
        [("s011", 0), ("s012", 0)]
    );
    assert_eq!(
        s013a.route.variables.get("akari.好感度"),
        Some(&Value::Int(3))
    );
    // 紬の好感度が0のまま s030 に着く道が無いので、s061（bad_end）には着けない。s060 には着ける。
    let reachable = |scene: &str| {
        cases
            .iter()
            .find(|c| c.route.target.scene == scene)
            .map(|c| c.route.reachable)
    };
    assert_eq!(reachable("s061"), Some(false));
    assert_eq!(reachable("s060"), Some(true));
    // s014（勇気が2で選べる「黙ってうなずく」の先）は、命令の無いスタブなので、始める命令が無い。
    assert_eq!(reachable("s014"), None);
    insta::assert_snapshot!("houkago", serde_json::to_string_pretty(&cases).unwrap());
}

#[test]
fn amayadori() {
    let cases = routes("amayadori");
    // 3つのエンディングは、どれも選択肢だけで着ける。
    for scene in ["s09", "s10", "s11"] {
        let case = cases
            .iter()
            .find(|c| c.route.target.scene == scene)
            .unwrap();
        assert!(case.route.reachable, "{scene}");
    }
    // 構内放送（`@呼出 s90`）の中では、呼び出し元が積まれている。
    let s90 = cases
        .iter()
        .find(|c| c.route.target.scene == "s90")
        .unwrap();
    assert_eq!(s90.route.calls, ["s01"]);
    insta::assert_snapshot!("amayadori", serde_json::to_string_pretty(&cases).unwrap());
}

/// 作業用の作品フォルダ。
fn temp_project(name: &str, story: &str, variables: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-route-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(dir.join("variables.yaml"), variables).unwrap();
    fs::write(dir.join("story/a.scn"), story).unwrap();
    dir
}

/// カーソルの位置から始める命令：見出しの行は最初の命令、空行は次の命令、選択肢の行はその `@選択`、
/// `@else` の行はその中身、シーンの最後の命令より後ろやシーンの外は始められない。
#[test]
fn targets() {
    let story = "地の文の前\n## a #s1\n\n地の文1。\n@選択 どうする\n- 行く\n  中の地の文。\n- 待つ -> s2\n@end\n@if flag.x\n地の文2。\n@else\n地の文3。\n@end\n\n## b #s2\n地の文4。\n";
    let dir = temp_project("targets", story, "flag.x: { type: bool }\n");
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let at = |needle: &str| {
        let offset = story.find(needle).unwrap();
        target_at(&project, &ir, "story/a.scn", offset).map(|t| {
            let line = story
                .get(t.span.start..t.span.end)
                .unwrap()
                .trim()
                .to_owned();
            (t.scene, line)
        })
    };
    assert_eq!(at("地の文の前"), None);
    assert_eq!(at("## a"), Some(("s1".into(), "地の文1。".into())));
    assert_eq!(at("- 行く"), Some(("s1".into(), "@選択 どうする".into())));
    assert_eq!(at("- 待つ"), Some(("s1".into(), "@選択 どうする".into())));
    assert_eq!(
        at("  中の地の文"),
        Some(("s1".into(), "中の地の文。".into()))
    );
    assert_eq!(at("@else"), Some(("s1".into(), "地の文3。".into())));
    assert_eq!(at("@end\n\n## b"), None);
    assert_eq!(at("地の文4"), Some(("s2".into(), "地の文4。".into())));
    // 行の途中でも、その行の命令。
    let offset = story.find("文1").unwrap();
    assert!(
        route_at(&project, &ir, "story/a.scn", offset, &Variables::new())
            .unwrap()
            .reachable
    );
    assert!(route_at(&project, &ir, "story/b.scn", 0, &Variables::new()).is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// 評価と実行の規則（プレビューと同じ）：割り算は0の向きへ切り捨て、宣言の無い変数は書かれるまで無い、
/// 誤りのある `@設定` は書かない、宣言の型に合わない値は書かない、`@呼出` と `@戻る`、循環しても止まる、
/// 選択肢の少ない道を選ぶ。
#[test]
fn evaluation() {
    let story = "## 始まり #s1\n\
        @設定 flag.n = -7 / 2\n\
        @設定 flag.m = flag.n * 2\n\
        @設定 flag.bad = flag.未宣言 + 1\n\
        @設定 flag.e = \"無い値\"\n\
        @設定 flag.k += 1\n\
        @call sub\n\
        @選択\n\
        - 回る -> s1\n\
        - 遠回り -> s2\n\
        - 近道 -> s3\n\
        @end\n\
        ## 呼ばれる #sub\n\
        @設定 flag.called = true\n\
        @return\n\
        ## 遠回り #s2\n\
        @選択\n\
        - 先へ -> s3\n\
        @end\n\
        ## 目的 #s3\n\
        地の文。\n";
    let vars = "flag.n: { type: int }\nflag.m: { type: int }\nflag.e: { type: enum, values: [A, B], default: A }\nflag.called: { type: bool }\n";
    let dir = temp_project("eval", story, vars);
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let offset = story.find("地の文。").unwrap();
    let route = route_at(&project, &ir, "story/a.scn", offset, &Variables::new()).unwrap();
    assert!(route.reachable);
    assert_eq!(route.steps.len(), 1, "近道を選ぶ");
    assert_eq!(route.steps[0].option, 2);
    let v = |k: &str| route.variables.get(k).cloned();
    assert_eq!(v("flag.n"), Some(Value::Int(-3)));
    assert_eq!(v("flag.m"), Some(Value::Int(-6)));
    assert_eq!(v("flag.bad"), None, "宣言の無い変数を読む式は誤り");
    assert_eq!(
        v("flag.e"),
        Some(Value::Str("A".into())),
        "列挙に無い値は書かない"
    );
    assert_eq!(v("flag.k"), None, "値の無い変数への += は誤り");
    assert_eq!(v("flag.called"), Some(Value::Bool(true)));
    assert!(route.calls.is_empty());

    // 着けない：選べる選択肢が無い、行き止まり。
    let story =
        "## a #s1\n@選択\n- だめ if 1 > 2 -> s2\n@end\n## b #s2\n地の文。\n## c #s3\n地の文。\n";
    let dir2 = temp_project("unreachable", story, "");
    let project = load_project(&dir2).unwrap();
    let ir = analyze(&project);
    for needle in ["地の文。\n## c", "地の文。\n"] {
        let offset = story.find(needle).unwrap();
        let route = route_at(&project, &ir, "story/a.scn", offset, &Variables::new()).unwrap();
        assert!(!route.reachable, "{needle}");
    }
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&dir2);
}

/// 永続の変数は、プレビューの今の値から探せる（プレビューは「最初から」でも永続の変数を残す）。
#[test]
fn persistent_variables() {
    let story = "## a #s1\n@選択\n- 2周目 if 周回.済み -> s2\n- 1周目 -> s3\n@end\n## b #s2\n地の文。\n## c #s3\n地の文。\n";
    let dir = temp_project(
        "persistent",
        story,
        "周回.済み: { type: bool, scope: persistent }\nflag.x: { type: bool }\n",
    );
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let offset = story.find("地の文。\n## c").unwrap();
    let none = route_at(&project, &ir, "story/a.scn", offset, &Variables::new()).unwrap();
    assert!(!none.reachable);
    let kept = Variables::from([
        ("周回.済み".to_owned(), Value::Bool(true)),
        // 永続でない変数と、型に合わない値は使わない。
        ("flag.x".to_owned(), Value::Bool(true)),
    ]);
    let route = route_at(&project, &ir, "story/a.scn", offset, &kept).unwrap();
    assert!(route.reachable);
    assert_eq!(route.variables.get("flag.x"), Some(&Value::Bool(false)));
    let wrong = Variables::from([("周回.済み".to_owned(), Value::Int(1))]);
    assert!(
        !route_at(&project, &ir, "story/a.scn", offset, &wrong)
            .unwrap()
            .reachable
    );
    let _ = fs::remove_dir_all(&dir);
}

/// 選んだ結果を別々のフラグに書く2択が続いても、条件で読まない変数は状態の見分けに使わないので、探しきれる。
/// 条件で読む変数（とそれを書く `@設定` で読む変数）は見分けに使う。
#[test]
fn many_choices() {
    let mut story = String::new();
    let mut vars = String::new();
    for i in 0..30 {
        story.push_str(&format!(
            "## 分かれ道{i} #s{i}\n@選択\n- 右\n  @設定 flag.r{i} = true\n- 左\n  @設定 flag.l{i} = true\n@end\n-> s{}\n",
            i + 1
        ));
        vars.push_str(&format!(
            "flag.r{i}: {{ type: bool }}\nflag.l{i}: {{ type: bool }}\n"
        ));
    }
    // 最後は、最初の選択の結果で分かれる（`flag.r0` を `flag.k` 経由で読む）。
    story.push_str("## 終わり #s30\n@設定 flag.k = flag.r0\n@if flag.k\n右の終わり。\n@else\n左の終わり。\n@end\n");
    vars.push_str("flag.k: { type: bool }\n");
    let dir = temp_project("many", &story, &vars);
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    for (needle, first) in [("右の終わり", 0), ("左の終わり", 1)] {
        let offset = story.find(needle).unwrap();
        let route = route_at(&project, &ir, "story/a.scn", offset, &Variables::new()).unwrap();
        assert!(route.reachable, "{needle}：{:?}", route.message);
        assert_eq!(route.steps.len(), 30);
        assert_eq!(route.steps[0].option, first, "{needle}");
    }
    let _ = fs::remove_dir_all(&dir);
}
