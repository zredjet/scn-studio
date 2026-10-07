//! 行IDの付与（仕様書9節、D041）。`scn fmt` の中身。
//!
//! 付けるのは I302 の出る行だけで、行の末尾に ` ^` と新しいIDを足すだけ。ほかのバイトは変えない。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use scn_core::line_id::{AssignedLineId, FileLineIds, LineIdGenerator, assign_line_ids};
use scn_core::project::{Project, SourceFile};
use scn_core::{analyze, load_project};

fn sample() -> Project {
    load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/houkago"))
        .expect("サンプルを読めない")
}

fn fixture_project(files: &[(&str, &str)]) -> Project {
    let mut project =
        load_project(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project"))
            .expect("共通のプロジェクトを読めない");
    project.files = files
        .iter()
        .map(|(path, text)| SourceFile {
            path: (*path).to_owned(),
            text: (*text).to_owned(),
        })
        .collect();
    project
}

/// 元の本文に、付けた行IDを足したもの。
fn insert_ids(original: &str, assigned: &[AssignedLineId]) -> String {
    let mut text = String::new();
    let mut from = 0;
    for a in assigned {
        text.push_str(&original[from..a.offset]);
        text.push_str(" ^");
        text.push_str(&a.id);
        from = a.offset;
    }
    text.push_str(&original[from..]);
    text
}

/// 付けた結果を、元のプロジェクトに当てる。
fn apply(project: &Project, results: &[FileLineIds]) -> Project {
    let mut project = project.clone();
    for result in results {
        let file = project
            .files
            .iter_mut()
            .find(|f| f.path == result.path)
            .expect("無いファイルを返した");
        file.text.clone_from(&result.text);
    }
    project
}

/// I302 の出る位置（スパンの終わり）を、ファイルごとに集める。
fn missing_line_id_ends(project: &Project) -> BTreeMap<String, Vec<usize>> {
    let mut ends: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for diag in analyze(project).diagnostics {
        if diag.code == "I302" {
            ends.entry(diag.file).or_default().push(diag.span.end);
        }
    }
    for list in ends.values_mut() {
        list.sort_unstable();
    }
    ends
}

fn codes_without_i302(project: &Project) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for diag in analyze(project).diagnostics {
        if diag.code != "I302" {
            *counts.entry(diag.code).or_default() += 1;
        }
    }
    counts
}

/// 付けた結果が、仕様書9節の付与のきまりに合っていることを確かめる。
fn assert_assignment(project: &Project, results: &[FileLineIds]) {
    // I302 の出る行に、ちょうど1つずつ付ける。
    let expected = missing_line_id_ends(project);
    let actual: BTreeMap<String, Vec<usize>> = results
        .iter()
        .map(|r| {
            (
                r.path.clone(),
                r.assigned.iter().map(|a| a.offset).collect(),
            )
        })
        .collect();
    assert_eq!(actual, expected, "付けた位置が I302 の行と合わない");

    // 既存のID（行IDを持てない行に付いたものも含む）と重ならない。
    let all_text: String = project.files.iter().map(|f| f.text.as_str()).collect();
    let mut seen = BTreeSet::new();
    for result in results {
        let original = &project
            .files
            .iter()
            .find(|f| f.path == result.path)
            .unwrap()
            .text;
        // 行IDを足した部分のほかは、1バイトも変えない。
        assert_eq!(result.text, insert_ids(original, &result.assigned));
        for a in &result.assigned {
            assert_eq!(a.id.len(), 6, "{}", a.id);
            assert!(
                a.id.chars()
                    .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()),
                "{}",
                a.id
            );
            assert!(
                !all_text.contains(&format!("^{}", a.id)),
                "既存のIDと重なる：{}",
                a.id
            );
            assert!(seen.insert(a.id.clone()), "付けたIDが重なる：{}", a.id);
        }
    }

    // 付けた後は I302 が無くなり、ほかの診断は変わらない。
    let after = apply(project, results);
    assert!(missing_line_id_ends(&after).is_empty());
    assert_eq!(codes_without_i302(&after), codes_without_i302(project));
}

/// 完了条件：サンプルの写しに付けると、I302 の5行（どれも `ai-draft` のシーン）にだけ付く。
#[test]
fn sample_project() {
    let project = sample();
    let results = assign_line_ids(&project, &mut LineIdGenerator::with_seed(1));
    assert_assignment(&project, &results);
    let counts: BTreeMap<&str, usize> = results
        .iter()
        .map(|r| (r.path.as_str(), r.assigned.len()))
        .collect();
    assert_eq!(
        counts,
        BTreeMap::from([("story/chapter01.scn", 3), ("story/chapter02.scn", 2)])
    );
}

/// 完了条件：2回続けて付けても、2回目は何も変えない。
#[test]
fn second_run_changes_nothing() {
    let project = sample();
    let first = assign_line_ids(&project, &mut LineIdGenerator::with_seed(1));
    let after = apply(&project, &first);
    assert!(assign_line_ids(&after, &mut LineIdGenerator::with_seed(2)).is_empty());
}

/// 付ける場所：行の末尾（改行の直前、行末の空白の後ろ）。fixture `fmt/placement.scn` と、
/// 付けた後の形 `fmt/placement.expected`（`{0}`… が付けた行ID）。
#[test]
fn placement() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fmt");
    let source = std::fs::read_to_string(dir.join("placement.scn")).unwrap();
    let template = std::fs::read_to_string(dir.join("placement.expected")).unwrap();
    let project = fixture_project(&[("story/a.scn", &source)]);
    let results = assign_line_ids(&project, &mut LineIdGenerator::with_seed(7));
    assert_assignment(&project, &results);
    let [result] = results.as_slice() else {
        panic!("1つのファイルだけが変わるはず：{results:?}");
    };
    let mut expected = template.replace("{{", "\u{0}").replace("}}", "\u{1}");
    for (i, a) in result.assigned.iter().enumerate() {
        expected = expected.replace(&format!("{{{i}}}"), &a.id);
    }
    let expected = expected.replace("\u{0}", "{").replace("\u{1}", "}");
    assert!(!expected.contains("{0}"), "付けた行IDが足りない");
    assert_eq!(result.assigned.len(), 8);
    assert_eq!(result.text, expected);
}

/// 行IDの無い行が無ければ、何も返さない。ファイルは、プロジェクトの順に返す。
#[test]
fn only_changed_files_in_order() {
    let project = fixture_project(&[
        ("story/a.scn", "## a #s1\n地の文 ^aaaa\n"),
        ("story/b.scn", "## b #s2\n地の文\n"),
        ("story/c.scn", "## c #s3\n地の文\n"),
    ]);
    let results = assign_line_ids(&project, &mut LineIdGenerator::with_seed(3));
    assert_assignment(&project, &results);
    let paths: Vec<&str> = results.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["story/b.scn", "story/c.scn"]);
}

/// 種を決めれば、同じIDになる。種を変えれば、違うIDになる。
#[test]
fn generator_is_deterministic_with_seed() {
    let project = fixture_project(&[("story/a.scn", "## a #s1\n一\n二\n")]);
    let ids = |seed| -> Vec<String> {
        assign_line_ids(&project, &mut LineIdGenerator::with_seed(seed))
            .into_iter()
            .flat_map(|r| r.assigned.into_iter().map(|a| a.id))
            .collect()
    };
    assert_eq!(ids(5), ids(5));
    assert_ne!(ids(5), ids(6));
    assert_ne!(ids(5)[0], ids(5)[1]);
}

// ---- 本文の書き換え（`write_source`） ----

fn temp_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scn-core-write-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("story")).unwrap();
    dir
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// 渡したバイトのまま書き、一時ファイルを残さない。
#[test]
fn write_source_writes_exact_bytes() {
    let root = temp_root("exact");
    let text = "\u{feff}## a #s1\r\n　地の文😀 ^aaaa\r\n";
    std::fs::write(root.join("story/a.scn"), "古い中身").unwrap();
    scn_core::write_source(&root, "story/a.scn", text).unwrap();
    assert_eq!(
        std::fs::read(root.join("story/a.scn")).unwrap(),
        text.as_bytes()
    );
    assert_eq!(names(&root.join("story")), ["a.scn"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// 読み取り専用のファイルは書き換えず、エラーにする。一時ファイルも残さない。
#[test]
fn write_source_refuses_read_only() {
    let root = temp_root("readonly");
    let path = root.join("story/a.scn");
    std::fs::write(&path, "元の中身").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();

    let error = scn_core::write_source(&root, "story/a.scn", "新しい中身").unwrap_err();
    assert!(error.to_string().contains("a.scn"), "{error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "元の中身");
    assert_eq!(names(&root.join("story")), ["a.scn"]);

    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

/// シンボリックリンクは残し、リンク先を書き換える。
#[cfg(unix)]
#[test]
fn write_source_keeps_symlink() {
    let root = temp_root("symlink");
    std::fs::write(root.join("real.scn"), "元の中身").unwrap();
    std::os::unix::fs::symlink(root.join("real.scn"), root.join("story/a.scn")).unwrap();
    scn_core::write_source(&root, "story/a.scn", "新しい中身").unwrap();
    assert!(
        std::fs::symlink_metadata(root.join("story/a.scn"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("real.scn")).unwrap(),
        "新しい中身"
    );
    let _ = std::fs::remove_dir_all(&root);
}
