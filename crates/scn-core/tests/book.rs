//! EPUB と縦書きPDF（`book`、T3-13、D074）：サンプルの EPUB の中のファイルのスナップショット（完了条件）、ZIP の
//! 形（`mimetype` が先頭で圧縮しない）、縦書きと横書き、縦中横、注記、印刷用の HTML、プロファイル。

use std::fs;
use std::path::{Path, PathBuf};

use scn_core::book::{BookProfile, epub, epub_files, print_html, read_profile};
use scn_core::route::{ReadingTarget, Variables, read, target_label};
use scn_core::{Project, analyze, load_project};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../samples")
        .join(name)
}

const MODIFIED: &str = "2026-10-09T00:00:00Z";

fn files_of(project: &Project, target: &ReadingTarget, profile: &BookProfile) -> String {
    let ir = analyze(project);
    let reading = read(project, &ir, target, &Variables::new());
    let label = target_label(&ir, target);
    let mut out = String::new();
    for (path, text) in epub_files(project, &reading, &label, profile, MODIFIED) {
        out.push_str(&format!("==== {path}\n{text}"));
        if !text.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

#[test]
fn houkago_route() {
    let project = load_project(&sample("houkago")).unwrap();
    let target = ReadingTarget::Route {
        name: "灯".to_owned(),
    };
    insta::assert_snapshot!(
        "houkago_akari",
        files_of(&project, &target, &BookProfile::default())
    );
}

#[test]
fn amayadori_common() {
    let project = load_project(&sample("amayadori")).unwrap();
    let ir = analyze(&project);
    let target = scn_core::route::reading_choices(&ir)
        .into_iter()
        .map(|c| c.target)
        .next()
        .unwrap();
    insta::assert_snapshot!(
        "amayadori_first",
        files_of(&project, &target, &BookProfile::default())
    );
}

/// ZIP の形：`mimetype` が先頭で、圧縮せず、余分な欄が無い（EPUB の決まり）。
#[test]
fn zip_layout() {
    let project = load_project(&sample("houkago")).unwrap();
    let ir = analyze(&project);
    let target = ReadingTarget::Route {
        name: "灯".to_owned(),
    };
    let reading = read(&project, &ir, &target, &Variables::new());
    let bytes = epub(
        &project,
        &reading,
        "灯ルート",
        &BookProfile::default(),
        MODIFIED,
    )
    .unwrap();
    // ローカルヘッダ：署名、版、フラグ、方式（0 = stored）…、名前の長さ、余分な欄の長さ、名前、中身。
    assert_eq!(bytes.get(..4), Some(&b"PK\x03\x04"[..]));
    assert_eq!(bytes.get(8..10), Some(&[0u8, 0][..]), "stored");
    assert_eq!(bytes.get(26..28), Some(&[8u8, 0][..]), "名前の長さ");
    assert_eq!(bytes.get(28..30), Some(&[0u8, 0][..]), "余分な欄なし");
    assert_eq!(
        bytes.get(30..58),
        Some(&b"mimetypeapplication/epub+zip"[..])
    );
}

/// 縦書きと横書き、縦中横、ルビと傍点、注記の有無、印刷用の HTML。
#[test]
fn writing_modes_and_markup() {
    let dir = std::env::temp_dir().join(format!("scn-book-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("story")).unwrap();
    fs::write(
        dir.join("project.toml"),
        "format_version = 0\ntitle = \"テスト\"\n[dialogue.brackets]\n\"「」\" = \"normal\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("story/a.scn"),
        "## 始まり #s1\n\
         地の文。12月25日、2026年。本当!?　R2D2 <a> & \"b\"\n\
         |灯《あかり》と《《強調》》、《《第3話》》。\n\
         @選択\n\
         - 行く -> s2\n\
         @end\n\
         ## 終わり #s2\n\
         @エンディング end\n",
    )
    .unwrap();
    let project = load_project(&dir).unwrap();
    let ir = analyze(&project);
    let target = scn_core::route::reading_choices(&ir)
        .into_iter()
        .map(|c| c.target)
        .next()
        .unwrap();
    let vertical = files_of(&project, &target, &BookProfile::default());
    assert!(
        vertical.contains("<span class=\"tcy\">12</span>月<span class=\"tcy\">25</span>日"),
        "{vertical}"
    );
    assert!(
        vertical.contains("2026年"),
        "4桁は縦中横にしない：{vertical}"
    );
    assert!(
        vertical.contains("本当<span class=\"tcy\">!?</span>"),
        "{vertical}"
    );
    assert!(
        vertical.contains("R2D2"),
        "英字に続く数字は囲まない：{vertical}"
    );
    assert!(
        vertical.contains("&lt;a&gt; &amp; &quot;b&quot;"),
        "{vertical}"
    );
    assert!(
        vertical.contains("<ruby>灯<rt>あかり</rt></ruby>と<em class=\"sesame\">強調</em>"),
        "{vertical}"
    );
    assert!(
        vertical.contains("<em class=\"sesame\">第<span class=\"tcy\">3</span>話</em>"),
        "傍点の中も縦中横：{vertical}"
    );
    assert!(
        vertical.contains("@charset \"UTF-8\";\nhtml {"),
        "EPUB の CSS は @charset で始める：{vertical}"
    );
    assert!(
        vertical.contains("page-break-before: always;"),
        "{vertical}"
    );
    assert!(
        vertical.contains("<p class=\"note\">〔選択：行く〕</p>"),
        "{vertical}"
    );
    assert!(
        vertical.contains("<p class=\"note gap\">〔エンディング：end〕</p>"),
        "{vertical}"
    );
    assert!(vertical.contains("page-progression-direction=\"rtl\""));
    assert!(vertical.contains("writing-mode: vertical-rl;"));
    let horizontal = files_of(
        &project,
        &target,
        &BookProfile {
            vertical: false,
            notes: false,
            author: Some("作者".to_owned()),
            ..BookProfile::default()
        },
    );
    assert!(horizontal.contains("12月25日"), "横書きは縦中横にしない");
    assert!(!horizontal.contains("〔選択"), "注記なし");
    assert!(horizontal.contains("page-progression-direction=\"ltr\""));
    assert!(horizontal.contains("<dc:creator>作者</dc:creator>"));
    let reading = read(&project, &ir, &target, &Variables::new());
    let html = print_html(&project, &reading, "通し", &BookProfile::default());
    assert!(
        html.contains("@page {\n  size: A5;\n  margin: 15mm;\n}"),
        "{html}"
    );
    assert!(html.contains("<title>テスト（通し）</title>"), "{html}");
    assert!(html.contains("writing-mode: vertical-rl;"));
    assert!(
        !html.contains("@charset"),
        "印刷用の HTML は meta で文字コードを示す"
    );
    // プロファイル：誤りは読み飛ばす。
    fs::create_dir_all(dir.join("export")).unwrap();
    fs::write(
        dir.join("export/book.yaml"),
        "writing: horizontal\nauthor: 作者\nfont: \"Noto Serif JP\"\npage_size: \"B6; color: red\"\nfont_size: 9\npage_margin: \"10mm /* x\"\nnotes: false\n",
    )
    .unwrap();
    let (profile, problems) = read_profile(&dir);
    assert!(!profile.vertical);
    assert!(!profile.notes);
    assert_eq!(profile.font.as_deref(), Some("Noto Serif JP"));
    assert_eq!(profile.page_size, "A5");
    assert_eq!(profile.font_size, "9pt", "単位の無い数は読み飛ばす");
    assert_eq!(profile.page_margin, "15mm", "注釈の /* は読み飛ばす");
    for key in ["page_size", "font_size", "page_margin"] {
        assert!(problems.iter().any(|p| p.contains(key)), "{problems:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}
