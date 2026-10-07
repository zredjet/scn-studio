//! 索引（T1-3、仕様書17節）：IRとプロジェクトの宣言から導く一覧。

use std::path::PathBuf;

use scn_core::index::{Index, build_index};
use scn_core::project::{Project, SourceFile};
use scn_core::{analyze, load_project};

fn index_of(project: &Project) -> Index {
    build_index(project, &analyze(project))
}

/// 完了条件：サンプル作品の索引のスナップショット。人が中身を見る。
#[test]
fn sample_index() {
    let project =
        load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"))
            .expect("サンプルを読めない");
    let index = index_of(&project);
    insta::assert_snapshot!(serde_json::to_string_pretty(&index).unwrap());
}

/// 読む位置と書く位置、素材の種類と登録の有無、宣言されていない変数、キャラクターの出現。
#[test]
fn reads_writes_and_assets() {
    let mut project =
        load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project"))
            .expect("共通のプロジェクトを読めない");
    let text = concat!(
        "## a #s1\n",
        "@立ち絵 灯 笑顔 左\n",
        "@立ち絵 あかり 泣き顔\n",
        "灯「{=flag.名前}、{se 風}来て」 ^aaaa\n",
        "@設定 flag.勇気 += flag.勇気 + 1\n",
        "@if flag.勇気 >= 2 and flag.迷い\n",
        "  @SE 雷\n",
        "@end\n",
        "@選択 どうする ^aaab\n",
        "- 行く if flag.未宣言 ^aaac\n",
        "  @退場 灯\n",
        "@end\n",
    );
    project.files = vec![SourceFile {
        path: "story/a.scn".into(),
        text: text.into(),
    }];
    let index = index_of(&project);
    let line_of = |span: scn_core::Span| text[..span.start].matches('\n').count() + 1;

    let var = |key: &str| index.variables.iter().find(|v| v.key == key).unwrap();
    let lines = |locations: &[scn_core::index::Location]| -> Vec<usize> {
        locations.iter().map(|l| line_of(l.span)).collect()
    };
    let courage = var("flag.勇気");
    assert!(courage.declared);
    assert_eq!(courage.ty, Some("int"));
    assert_eq!(lines(&courage.writes), [5]);
    // 同じ行で何度読んでも1つ。
    assert_eq!(lines(&courage.reads), [5, 6]);
    assert_eq!(lines(&var("flag.迷い").reads), [6]);
    assert_eq!(lines(&var("flag.名前").reads), [4]);
    let undeclared = var("flag.未宣言");
    assert!(!undeclared.declared);
    assert_eq!(undeclared.ty, None);
    assert_eq!(lines(&undeclared.reads), [10]);
    assert_eq!(var("flag.ルート").values, ["共通", "灯", "紬"]);
    assert!(var("flag.ルート").reads.is_empty());

    let akari = index.characters.iter().find(|c| c.id == "akari").unwrap();
    let uses: Vec<(&str, usize)> = akari
        .uses
        .iter()
        .map(|u| (u.kind, line_of(u.location.span)))
        .collect();
    assert_eq!(
        uses,
        [("show", 2), ("show", 3), ("speaker", 4), ("hide", 11)]
    );
    assert!(akari.uses.iter().all(|u| u.location.scene == "s1"));

    let asset = |kind: &str, name: &str| {
        index
            .assets
            .iter()
            .find(|a| a.kind == kind && a.name == name)
            .unwrap()
    };
    assert!(asset("expression", "笑顔").registered);
    assert_eq!(
        asset("expression", "笑顔").character.as_deref(),
        Some("akari")
    );
    assert!(!asset("expression", "泣き顔").registered);
    assert!(asset("se", "風").registered);
    assert_eq!(lines(&asset("se", "風").uses), [4]);
    assert!(!asset("se", "雷").registered);
    assert!(asset("bgm", "日常").uses.is_empty());

    let ids: Vec<(&str, usize)> = index
        .line_ids
        .iter()
        .map(|l| (l.id.as_str(), line_of(l.location.span)))
        .collect();
    assert_eq!(ids, [("aaaa", 4), ("aaab", 9), ("aaac", 10)]);
}
