//! 素材カタログ（`assets`、T2-3、D054）：サンプルの素材の一覧と発注リストのスナップショット（完了条件）、
//! カタログへの追記とファイルの割り当てで、ほかの行が変わらないこと（完了条件）。

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use scn_core::assets::{
    AssetList, AssetState, CatalogEditError, add_entry, asset_list, asset_location, orders,
    orders_csv, set_file,
};
use scn_core::index::build_index;
use scn_core::{analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

fn list_of(root: &Path) -> AssetList {
    let project = load_project(root).unwrap();
    let ir = analyze(&project);
    let index = build_index(&project, &ir);
    asset_list(&project, root, &index)
}

/// 一覧を、人が読める形にする（実在するファイルの絶対パスは、あるかどうかだけ）。
fn dump(list: &AssetList) -> String {
    let mut out = String::new();
    for r in &list.rows {
        let _ = writeln!(
            out,
            "{} {}{} {:?} file={} path={} uses={} scenes=[{}] addable={}",
            r.kind,
            r.character_name
                .as_deref()
                .map(|c| format!("{c}/"))
                .unwrap_or_default(),
            r.name,
            r.state,
            r.file.as_deref().unwrap_or("-"),
            r.path.is_some(),
            r.uses.len(),
            r.scenes.join(" "),
            r.addable
        );
    }
    out.push_str("characters:");
    for c in &list.characters {
        let _ = write!(
            out,
            " {}({}{})",
            c.id,
            c.name,
            if c.registered { "" } else { ", 未登録" }
        );
    }
    out.push('\n');
    out
}

#[test]
fn houkago() {
    let list = list_of(&sample("houkago"));
    insta::assert_snapshot!("houkago_list", dump(&list));
    let csv = orders_csv(&orders(&list));
    assert!(csv.starts_with("\u{feff}種別,名前,キャラ,差分,使用回数,初出,使用シーン,備考\r\n"));
    insta::assert_snapshot!(
        "houkago_orders",
        csv.trim_start_matches('\u{feff}').replace("\r\n", "\n")
    );
}

#[test]
fn amayadori() {
    let list = list_of(&sample("amayadori"));
    insta::assert_snapshot!("amayadori_list", dump(&list));
    let csv = orders_csv(&orders(&list));
    insta::assert_snapshot!(
        "amayadori_orders",
        csv.trim_start_matches('\u{feff}').replace("\r\n", "\n")
    );
}

/// `before` の行が、順を変えずにすべて `after` に残り、増えたのが `added` 行だけか。
fn only_added(before: &str, after: &str, added: usize) -> bool {
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    if a.len() != b.len() + added {
        return false;
    }
    let mut it = a.iter();
    b.iter().all(|line| it.any(|x| x == line))
}

fn parse(text: &str) -> serde_json::Value {
    let yaml: yaml_serde::Value = yaml_serde::from_str(text).unwrap();
    serde_json::to_value(yaml).unwrap()
}

/// サンプルのカタログに、本文で使われている未登録の意味名をすべて足す。足したのは1つにつき1行で、
/// ほかの行は変わらず、読み直すと仮素材（`{}`）として入っている。
#[test]
fn add_unregistered_to_samples() {
    // 本文で使われている未登録の意味名に加えて、新しい意味名も足す（amayadori には未登録が無い）。
    for (name, character) in [("houkago", "akari"), ("amayadori", "shino")] {
        let root = sample(name);
        let list = list_of(&root);
        let mut items: Vec<(&str, Option<String>, String)> = list
            .rows
            .iter()
            .filter(|r| r.addable)
            .map(|r| (r.kind, r.character.clone(), r.name.clone()))
            .collect();
        items.extend([
            ("background", None, "駅/朝".to_owned()),
            ("expression", Some(character.to_owned()), "驚き".to_owned()),
            ("bgm", None, "朝の曲".to_owned()),
            ("se", None, "汽笛".to_owned()),
        ]);
        let text = fs::read_to_string(root.join("assets/catalog.yaml")).unwrap();
        let mut current = text.clone();
        for (added, (kind, character, entry)) in items.iter().enumerate() {
            current = add_entry(&current, kind, character.as_deref(), entry).unwrap();
            assert!(
                only_added(&text, &current, added + 1),
                "{name}: {entry}\n{current}"
            );
        }
        let value = parse(&current);
        for (kind, character, entry) in &items {
            let section = match *kind {
                "background" => &value["backgrounds"],
                "bgm" => &value["bgm"],
                "se" => &value["se"],
                _ => &value["characters"][character.as_deref().unwrap()]["expressions"],
            };
            assert_eq!(section[entry], serde_json::json!({}), "{name}: {entry}");
        }
        // もうあれば、そのまま。
        let again = add_entry(&current, "background", None, "駅/朝").unwrap();
        assert_eq!(again, current);
    }
}

/// 節やキャラクターが無ければ作る。書き方（コメント、桁そろえ、CRLF、終わりの改行の無いファイル）は残す。
#[test]
fn add_creates_sections() {
    // 空のファイル。
    let out = add_entry("", "background", None, "屋上/夜").unwrap();
    assert_eq!(out, "backgrounds:\n  屋上/夜: {}\n");
    let out = add_entry(&out, "expression", Some("akari"), "笑顔").unwrap();
    assert_eq!(
        out,
        "backgrounds:\n  屋上/夜: {}\ncharacters:\n  akari:\n    expressions:\n      笑顔: {}\n"
    );

    // コメントと桁そろえ、CRLF、終わりの改行が無い。
    let text = "# カタログ\r\nbackgrounds:\r\n    教室:   { file: files/a.png }   # あり\r\n\r\n# 音\r\nse:\r\n    ドア: {}";
    let out = add_entry(text, "background", None, "廊下").unwrap();
    assert_eq!(
        out,
        "# カタログ\r\nbackgrounds:\r\n    教室:   { file: files/a.png }   # あり\r\n    廊下: {}\r\n\r\n# 音\r\nse:\r\n    ドア: {}"
    );
    let out = add_entry(&out, "se", None, "風").unwrap();
    assert!(
        out.ends_with("se:\r\n    ドア: {}\r\n    風: {}\r\n"),
        "{out:?}"
    );
    let out = add_entry(&out, "bgm", None, "日常").unwrap();
    assert!(
        out.ends_with("    風: {}\r\nbgm:\r\n    日常: {}\r\n"),
        "{out:?}"
    );

    // キャラクターはあるが expressions が無い。dir の後ろに足す。
    let text = "characters:\n  akari:\n    dir: files/chara/akari/\n  tsumugi:\n    expressions:\n      笑顔: smile.png\n";
    let out = add_entry(text, "expression", Some("akari"), "照れ").unwrap();
    assert_eq!(
        out,
        "characters:\n  akari:\n    dir: files/chara/akari/\n    expressions:\n      照れ: {}\n  tsumugi:\n    expressions:\n      笑顔: smile.png\n"
    );
    let out = add_entry(&out, "expression", Some("miura"), "真顔").unwrap();
    assert!(out.ends_with("      笑顔: smile.png\n  miura:\n    expressions:\n      真顔: {}\n"));

    // `{}` の節は、`{}` を消して下に書く（行末のコメントは残す）。
    let out = add_entry(
        "backgrounds: {}  # まだ無い\nse:\n  ドア: {}\n",
        "background",
        None,
        "屋上",
    )
    .unwrap();
    assert_eq!(
        out,
        "backgrounds:  # まだ無い\n  屋上: {}\nse:\n  ドア: {}\n"
    );

    // そのままで読めない名前は引用符で囲む。
    // YAML が改行と読む文字や、印字できない文字は `\uXXXX` にする（proptest で見つけた U+2028 など）。
    for name in [
        "1",
        "true",
        "a: b",
        "#c",
        " 空白",
        "[x]",
        "null",
        "~",
        "\u{2028}",
        "a\u{2029}b",
        "a\u{85}b",
        "a\u{7f}b",
        "a\u{9f}b",
    ] {
        let out = add_entry("se:\n  ドア: {}\n", "se", None, name).unwrap();
        assert_eq!(
            parse(&out)["se"][name],
            serde_json::json!({}),
            "{name}: {out}"
        );
        assert!(only_added("se:\n  ドア: {}\n", &out, 1), "{name}: {out}");
    }
}

/// 画面からは書き足せない書き方。
#[test]
fn add_unsupported() {
    let flow = "backgrounds: { 教室: {} }\n";
    assert!(matches!(
        add_entry(flow, "background", None, "廊下"),
        Err(CatalogEditError::Unsupported(_))
    ));
    assert!(matches!(
        add_entry("backgrounds: [a]\n", "background", None, "廊下"),
        Err(CatalogEditError::Unsupported(_))
    ));
    assert!(matches!(
        add_entry("backgrounds:\n  a: {\n", "background", None, "廊下"),
        Err(CatalogEditError::Unreadable(_))
    ));
    assert!(matches!(
        add_entry("se: {}\n", "expression", None, "笑顔"),
        Err(CatalogEditError::Unsupported(_))
    ));
}

/// ファイルの割り当て：値だけを差し替え、キーの書き方・ほかのキー（`loop`）・行末のコメントを残す。
#[test]
fn assign_file() {
    let text = "backgrounds:\n  屋上/夜:   {}   # 未作成\n  教室: { file: files/a.png }\nbgm:\n  切ない: { loop: false }\nse:\n  ドア:\n";
    let out = set_file(text, "background", None, "屋上/夜", "files/bg/night.png").unwrap();
    assert_eq!(
        out,
        "backgrounds:\n  屋上/夜: { file: files/bg/night.png }   # 未作成\n  教室: { file: files/a.png }\nbgm:\n  切ない: { loop: false }\nse:\n  ドア:\n"
    );
    let out = set_file(&out, "background", None, "教室", "files/b c.png").unwrap();
    assert!(out.contains("  教室: { file: files/b c.png }\n"), "{out}");
    let out = set_file(&out, "bgm", None, "切ない", "files/bgm/s.ogg").unwrap();
    assert!(
        out.contains("  切ない: { file: files/bgm/s.ogg, loop: false }\n"),
        "{out}"
    );
    let out = set_file(&out, "se", None, "ドア", "files/se/door.ogg").unwrap();
    assert!(
        out.ends_with("  ドア: { file: files/se/door.ogg }\n"),
        "{out}"
    );
    let quoted = set_file(&out, "se", None, "ドア", "files/se/a, b#c.ogg").unwrap();
    assert_eq!(
        parse(&quoted)["se"]["ドア"]["file"],
        serde_json::json!("files/se/a, b#c.ogg")
    );

    // 値の中の「空白＋引用符」は引用符の始まりではない（行末のコメントを残す）。
    let odd = "se:\n  a: { file: x 'y.png }  # c\n";
    let out = set_file(odd, "se", None, "a", "files/z.ogg").unwrap();
    assert_eq!(out, "se:\n  a: { file: files/z.ogg }  # c\n");
    let out = set_file(&out, "se", None, "a", "files/a 'b.ogg").unwrap();
    let out = set_file(&out, "se", None, "a", "files/c.ogg").unwrap();
    assert_eq!(out, "se:\n  a: { file: files/c.ogg }  # c\n");
    let quoted_key = "se:\n  'a b': {}  # 引用符のキー\n";
    let out = set_file(quoted_key, "se", None, "a b", "files/c.ogg").unwrap();
    assert_eq!(out, "se:\n  'a b': { file: files/c.ogg }  # 引用符のキー\n");

    // 立ち絵の差分は dir から数える。dir の外は割り当てない。dir が無ければ assets/ から。
    let chara = "characters:\n  akari:\n    dir: files/chara/akari/\n    expressions:\n      笑顔: smile.png\n      照れ: {}\n  miura:\n    expressions:\n      真顔: {}\n";
    let out = set_file(
        chara,
        "expression",
        Some("akari"),
        "照れ",
        "files/chara/akari/tere.png",
    )
    .unwrap();
    assert!(out.contains("      照れ: tere.png\n"), "{out}");
    assert!(matches!(
        set_file(
            chara,
            "expression",
            Some("akari"),
            "照れ",
            "files/other/tere.png"
        ),
        Err(CatalogEditError::Unsupported(_))
    ));
    let out = set_file(chara, "expression", Some("miura"), "真顔", "files/m.png").unwrap();
    assert!(out.ends_with("      真顔: files/m.png\n"), "{out}");

    // カタログに無い意味名。
    assert!(matches!(
        set_file(text, "background", None, "廊下", "files/x.png"),
        Err(CatalogEditError::Unsupported(_))
    ));
}

/// 素材の状態：ファイルあり・仮素材（`{}` と、書いたファイルが見つからないもの）・未登録。
#[test]
fn states() {
    let dir = std::env::temp_dir().join(format!("scn-core-assets-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::create_dir_all(dir.join("assets/files")).unwrap();
    fs::create_dir_all(dir.join("bible/entries/character")).unwrap();
    fs::write(dir.join("project.toml"), "format_version = 0\n").unwrap();
    fs::write(
        dir.join("bible/entries/character/akari.md"),
        "---\nid: akari\nname: 灯\n---\n",
    )
    .unwrap();
    fs::write(dir.join("assets/files/a.png"), "png").unwrap();
    fs::write(
        dir.join("assets/catalog.yaml"),
        "backgrounds:\n  あり: { file: files/a.png }\n  仮: {}\n  無い: { file: files/none.png }\n  未使用: {}\n",
    )
    .unwrap();
    fs::write(
        dir.join("story/a.scn"),
        "## a #s1\n@背景 あり\n@背景 仮\n@背景 無い\n@背景 新しい\n@立ち絵 灯 笑顔\n@立ち絵 だれか 真顔\n## b #s2\n@背景 仮\n@SE 鐘\n地の文{se 鐘}。\n",
    )
    .unwrap();
    let list = list_of(&dir);
    let state = |name: &str| list.rows.iter().find(|r| r.name == name).map(|r| r.state);
    assert_eq!(state("あり"), Some(AssetState::File));
    assert_eq!(state("仮"), Some(AssetState::Placeholder));
    assert_eq!(state("無い"), Some(AssetState::Placeholder));
    assert_eq!(state("新しい"), Some(AssetState::Unregistered));
    let row = |name: &str| list.rows.iter().find(|r| r.name == name).unwrap();
    assert_eq!(row("仮").scenes, ["s1", "s2"]);
    assert_eq!(row("鐘").uses.len(), 2);
    assert!(
        row("笑顔").addable,
        "設定DBにあるキャラクターの差分は足せる"
    );
    assert!(
        !row("真顔").addable,
        "解決できないキャラクターの差分は足せない"
    );
    assert!(row("あり").path.is_some());
    let csv = orders_csv(&orders(&list));
    insta::assert_snapshot!(
        "states_orders",
        csv.trim_start_matches('\u{feff}').replace("\r\n", "\n")
    );
    let _ = fs::remove_dir_all(&dir);
}

proptest! {
    /// 任意の名前を足しても、ほかの行は変わらず、読み直すと入っている（画面から足せない名前は、断る）。
    #[test]
    fn add_any_name(name in "\\PC{1,12}", kind in prop::sample::select(vec!["background", "expression", "bgm", "se"])) {
        let root = sample("houkago");
        let text = fs::read_to_string(root.join("assets/catalog.yaml")).unwrap();
        let character = (kind == "expression").then_some("akari");
        match add_entry(&text, kind, character, &name) {
            Ok(out) => {
                let value = parse(&out);
                let section = match kind {
                    "background" => &value["backgrounds"],
                    "bgm" => &value["bgm"],
                    "se" => &value["se"],
                    _ => &value["characters"]["akari"]["expressions"],
                };
                prop_assert!(section.get(&name).is_some());
                if out != text {
                    prop_assert!(only_added(&text, &out, 1), "{}", out);
                }
            }
            Err(e) => prop_assert!(false, "{name:?}: {e:?}"),
        }
    }
}

/// 素材の場所（T3-1、R191）：中のファイル、無いファイルは近いフォルダ、外を指すパスとリンクは断る。
#[test]
fn asset_locations_stay_inside() {
    let dir = std::env::temp_dir().join(format!("scn-assets-location-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let assets = dir.join("assets");
    fs::create_dir_all(assets.join("bg")).unwrap();
    fs::write(assets.join("bg").join("a.png"), b"x").unwrap();
    assert_eq!(asset_location(&assets, None).unwrap(), assets);
    // カタログの `/` 区切りは、OS の区切りにする。
    assert_eq!(
        asset_location(&assets, Some("bg/a.png")).unwrap(),
        assets.join("bg").join("a.png")
    );
    // まだ無いファイルは、いちばん近いある親のフォルダ。
    assert_eq!(
        asset_location(&assets, Some("bg/new/b.png")).unwrap(),
        assets.join("bg")
    );
    assert_eq!(asset_location(&assets, Some("se/x.ogg")).unwrap(), assets);
    assert!(asset_location(&assets, Some("../project.toml")).is_err());
    assert!(asset_location(&assets, Some("/etc/hosts")).is_err());
    assert!(asset_location(&assets, Some("")).is_err());
    #[cfg(unix)]
    {
        // 外を指すリンクは断る（たどった先が素材のフォルダの中に戻るものはよい）。
        fs::create_dir_all(dir.join("outside")).unwrap();
        fs::write(dir.join("outside").join("x.png"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.join("outside"), assets.join("out")).unwrap();
        assert!(asset_location(&assets, Some("out")).is_err());
        assert!(asset_location(&assets, Some("out/x.png")).is_err());
        assert!(asset_location(&assets, Some("out/missing.png")).is_err());
        std::os::unix::fs::symlink(&dir, assets.join("up")).unwrap();
        assert!(asset_location(&assets, Some("up/assets/bg/a.png")).is_ok());
    }
    let _ = fs::remove_dir_all(&dir);
}
