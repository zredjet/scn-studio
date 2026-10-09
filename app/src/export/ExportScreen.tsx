// 出力の画面（T1-12、D051）：台本（収録と校正の表）を CSV か Excel 形式（T2-10、D061）で書き出す。書き出す前に、
// 中身を表で見られる。書き出し先は作品フォルダの `export/script.csv`・`export/script.xlsx`（CLI の
// `scn export script` と同じ）。書き出した台本を収録の基準として記録すると、次から変わった台詞が再収録候補になる。
// Ren'Py への書き出し（T2-2、D053）は、出力プロファイル（export/renpy.yaml）に従って、書き出し先の
// game/scn/ を作り直す（CLI の `scn export renpy` と同じ）。ティラノスクリプト（T3-11、D072）も同じく、
// export/tyrano.yaml に従って data/scenario/scn/ と素材の data/*/scn/ を作り直す。宴（T3-12、D073）は
// export/utage.yaml に従って、シナリオと設定のシートの Excel 形式と素材を export/utage/ に書く。EPUB と縦書きPDF
// （T3-13、D074）は、道を選んで export/epub/・export/pdf/（PDF を組むための印刷用の HTML）に書き、export/book.yaml の
// pdf_command があれば PDF も組む（T3-14）。書き出しのプラグイン（T3-14、D075）は、作品フォルダの export/<名前>.yaml
// （テンプレートか外部コマンド）で書き出す。作品フォルダのコマンドは、この端末で初めて実行する前と、コマンドが
// 変わったときに確かめる。プレーンテキスト（T2-5、D056）は、道（共通、ルート、エンディング）を選んで export/text/
// に書く（CLI の `scn export text` と同じ）。

import { useEffect, useState } from "react";
import { keyed } from "../bible/fields.ts";
import type { BookSettings, RenpyResult } from "../core/api.ts";
import type {
  AiLines,
  PluginItem,
  PluginRun,
  ReadingChoice,
  ReadingTarget,
  ScriptFormat,
  ScriptSheet,
  Summary,
} from "../core/types.ts";
import { useScreenStatus } from "../shell/StatusBar.tsx";
import { exportStatus } from "../shell/status.ts";
import {
  outputTail,
  pluginKindLabel,
  pluginRunText,
  runApproved,
} from "./plugin.ts";

interface Props {
  /** 本文が変わったら、台本を取り直す。 */
  readonly summary: Summary;
  /** 保存していないファイルの数（台本は、保存していない本文も含めて作る）。 */
  readonly unsaved: number;
  /** 台本を作る。`aiDrafts` が偽なら AI下書きのシーンの台詞を省く（T3-9）。 */
  readonly load: (aiDrafts: boolean) => Promise<ScriptSheet>;
  /** 書き出す。書いたファイルのパスを返す。 */
  readonly write: (
    format: ScriptFormat,
    baseline: boolean,
    aiDrafts: boolean,
  ) => Promise<{
    readonly path: string;
    readonly rows: number;
  }>;
  /** Ren'Py へ書き出す。 */
  readonly writeRenpy: (aiDrafts: boolean) => Promise<RenpyResult>;
  /** ティラノスクリプトへ書き出す（T3-11）。 */
  readonly writeTyrano: (aiDrafts: boolean) => Promise<RenpyResult>;
  /** 宴へ書き出す（T3-12）。 */
  readonly writeUtage: (aiDrafts: boolean) => Promise<RenpyResult>;
  /** EPUB か印刷用の HTML を書き出す（T3-13）。`typeset` なら PDF も組む（T3-14）。 */
  readonly writeBook: (
    target: ReadingTarget,
    print: boolean,
    horizontal: boolean,
    notes: boolean,
    aiDrafts: boolean,
    typeset: boolean,
    approved: string | null,
  ) => Promise<PluginRun>;
  /** 書き出しのプラグインの一覧（T3-14）。 */
  readonly loadPlugins: () => Promise<PluginItem[]>;
  /** プラグインで書き出す（T3-14）。 */
  readonly runPlugin: (
    name: string,
    aiDrafts: boolean,
    approved: string | null,
  ) => Promise<PluginRun>;
  /** はい・いいえを聞く（作品フォルダのコマンドを実行する前）。 */
  readonly ask: (message: string, title: string) => Promise<boolean>;
  /** EPUB と縦書きPDFの出力プロファイルの書き方と注記（チェックの初めの値）。 */
  readonly loadBookSettings: () => Promise<BookSettings>;
  /** 通読の道の候補。 */
  readonly loadChoices: () => Promise<ReadingChoice[]>;
  /** プレーンテキストを書き出す。 */
  readonly writeText: (
    target: ReadingTarget,
    notes: boolean,
    aiDrafts: boolean,
  ) => Promise<{ readonly path: string }>;
  /** AIが書いた行の数（出自の記録。T3-4）。 */
  readonly loadProvenance: () => Promise<AiLines>;
  /** AIが書いた行の一覧を書き出す。 */
  readonly writeProvenance: () => Promise<{
    readonly path: string;
    readonly rows: number;
  }>;
}

/** AIの出自（T3-4、D065）：AIが書いた行の数と、一覧の書き出し（賞や公募の申告用）。 */
function ProvenanceCard({
  summary,
  load,
  write,
}: {
  readonly summary: Summary;
  readonly load: () => Promise<AiLines>;
  readonly write: () => Promise<{
    readonly path: string;
    readonly rows: number;
  }>;
}) {
  const [data, setData] = useState<AiLines | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void summary;
    let live = true;
    load()
      .then((d) => {
        if (live) setData(d);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, load]);
  return (
    <section className="export-card" aria-label="AIの出自">
      <div className="export-card-head">
        <div>
          <h2>AIの出自</h2>
          <p className="export-note">
            出自の記録（ai/provenance.jsonl）で、AIが書いた行（ai）と、AIが書いた行を人が直した行（ai-edited）の一覧。企画書と設定DBの、AIが書いた項目も続けて並べます。アプリで
            AI の行や項目を直して保存すると、ai-edited
            と記録します。書き出し先：export/provenance.csv
          </p>
        </div>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            write()
              .then((r) => {
                setNotice(`${r.path} に書き出しました（${r.rows}行）`);
                setError(null);
              })
              .catch((e: unknown) => setError(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          書き出す
        </button>
      </div>
      {data !== null && (
        <p className="export-info">
          行IDのある行 {data.counts.lines} のうち、AIが書いた行{" "}
          {data.counts.ai + data.counts.ai_edited}（ai {data.counts.ai}
          、ai-edited {data.counts.ai_edited}）
          {data.field_counts.ai + data.field_counts.ai_edited > 0 &&
            `。企画書と設定DBの、AIが書いた項目 ${
              data.field_counts.ai + data.field_counts.ai_edited
            }（ai ${data.field_counts.ai}、ai-edited ${data.field_counts.ai_edited}）`}
        </p>
      )}
      {data !== null && data.problems > 0 && (
        <p className="notice notice-warn">
          出自の記録に読めない行が {data.problems}{" "}
          あります（読み飛ばしました）。
        </p>
      )}
      {notice !== null && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

/** プレーンテキストの書き出し（ルート単位の通読用。ルビはなろう・カクヨム記法）。 */
function TextCard({
  summary,
  loadChoices,
  writeText,
}: {
  readonly summary: Summary;
  readonly loadChoices: () => Promise<ReadingChoice[]>;
  readonly writeText: (
    target: ReadingTarget,
    notes: boolean,
  ) => Promise<{ readonly path: string }>;
}) {
  const [choices, setChoices] = useState<readonly ReadingChoice[]>([]);
  const [selected, setSelected] = useState(0);
  const [notes, setNotes] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void summary;
    let live = true;
    loadChoices()
      .then((c) => {
        if (live) setChoices(c);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, loadChoices]);
  const index = Math.max(0, Math.min(selected, choices.length - 1));
  const choice = choices[index];
  return (
    <section className="export-card" aria-label="プレーンテキスト">
      <div className="export-card-head">
        <div>
          <h2>プレーンテキスト</h2>
          <p className="export-note">
            道を一本道にした通読用のテキスト。話者名は書かず、台詞は括弧で囲みます。ルビは
            ｜親文字《読み》、傍点は《《傍点》》（なろう・カクヨムの記法）。UTF-8・LF
            で書きます。書き出し先：export/text/（道の名前）.txt
          </p>
        </div>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy || choice === undefined}
          onClick={() => {
            if (choice === undefined) return;
            setBusy(true);
            writeText(choice.target, notes)
              .then((r) => {
                setNotice(`${r.path} に書き出しました`);
                setError(null);
              })
              .catch((e: unknown) => setError(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          書き出す
        </button>
      </div>
      <div className="export-options">
        <label>
          道
          <select
            value={index}
            onChange={(event) => setSelected(Number(event.target.value))}
          >
            {choices.map((c, i) => (
              <option key={c.label} value={i}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            checked={notes}
            onChange={(event) => setNotes(event.target.checked)}
          />
          選んだ選択肢とエンディングを注記する（〔選択：…〕）
        </label>
      </div>
      {choices.length === 0 && (
        <p className="notice notice-warn">
          道がありません（シーンのメタ情報に route
          を書くと、ルートごとに書き出せます）。
        </p>
      )}
      {notice !== null && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

/** EPUB と縦書きPDF（T3-13、D074）：道を選んで、EPUB か、PDF を組むための印刷用の HTML を書き出す。 */
function BookCard({
  summary,
  loadChoices,
  loadSettings,
  writeBook,
  ask,
}: {
  readonly summary: Summary;
  readonly loadChoices: () => Promise<ReadingChoice[]>;
  readonly loadSettings: () => Promise<BookSettings>;
  readonly writeBook: (
    target: ReadingTarget,
    print: boolean,
    horizontal: boolean,
    notes: boolean,
    typeset: boolean,
    approved: string | null,
  ) => Promise<PluginRun>;
  readonly ask: (message: string, title: string) => Promise<boolean>;
}) {
  const [choices, setChoices] = useState<readonly ReadingChoice[]>([]);
  const [selected, setSelected] = useState(0);
  const [notes, setNotes] = useState(true);
  const [horizontal, setHorizontal] = useState(false);
  /** PDF を組むコマンド（export/book.yaml の pdf_command）。 */
  const [pdfCommand, setPdfCommand] = useState<string | null>(null);
  const [typeset, setTypeset] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [problems, setProblems] = useState<readonly string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [output, setOutput] = useState<PluginRun | null>(null);
  const [busy, setBusy] = useState(false);
  // チェックの初めの値は、出力プロファイル（export/book.yaml）の writing と notes。
  useEffect(() => {
    let live = true;
    loadSettings()
      .then((s) => {
        if (!live) return;
        setHorizontal(!s.vertical);
        setNotes(s.notes);
        setPdfCommand(s.pdf_command);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [loadSettings]);
  useEffect(() => {
    void summary;
    let live = true;
    loadChoices()
      .then((c) => {
        if (live) setChoices(c);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, loadChoices]);
  const index = Math.max(0, Math.min(selected, choices.length - 1));
  const choice = choices[index];
  const write = (print: boolean) => {
    if (choice === undefined) return;
    const typesetNow = print && typeset && pdfCommand !== null;
    setBusy(true);
    runApproved(
      (approved) =>
        writeBook(
          choice.target,
          print,
          horizontal,
          notes,
          typesetNow,
          approved,
        ),
      ask,
    )
      .then((r) => {
        if (r === null) {
          setNotice("コマンドを実行しませんでした（何も書いていません）");
          setOutput(null);
          return;
        }
        const path = r.path ?? "";
        setNotice(
          typeof r.pdf === "string"
            ? `${path} に書き出し、${r.pdf} を組みました`
            : print && !typesetNow
              ? `${path} に書き出しました。PDF は CSS 組版の道具（Vivliostyle など）で組みます`
              : `${path} に書き出しました`,
        );
        setProblems(r.problems ?? []);
        setError(r.error ?? null);
        setOutput(typesetNow ? r : null);
      })
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <section className="export-card" aria-label="EPUB と縦書きPDF">
      <div className="export-card-head">
        <div>
          <h2>EPUB・縦書きPDF</h2>
          <p className="export-note">
            道を一本道にした小説の形（話者名なし、台詞は括弧）で、EPUB 3
            と、縦書きPDFを組むための印刷用の HTML
            を書き出します。ルビ・傍点・縦中横を付けます。紙の大きさや書体は
            export/book.yaml
            で決めます。書き出し先：export/epub/・export/pdf/（道の名前）
          </p>
        </div>
        <div className="export-buttons">
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || choice === undefined}
            onClick={() => write(false)}
          >
            EPUB
          </button>
          <button
            type="button"
            className="btn btn-sm"
            disabled={busy || choice === undefined}
            onClick={() => write(true)}
          >
            印刷用 HTML
          </button>
        </div>
      </div>
      <div className="export-options">
        <label>
          道
          <select
            value={index}
            onChange={(event) => setSelected(Number(event.target.value))}
          >
            {choices.map((c, i) => (
              <option key={c.label} value={i}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            checked={horizontal}
            onChange={(event) => setHorizontal(event.target.checked)}
          />
          横書き
        </label>
        <label>
          <input
            type="checkbox"
            checked={notes}
            onChange={(event) => setNotes(event.target.checked)}
          />
          選んだ選択肢とエンディングを注記する
        </label>
        {pdfCommand !== null && (
          <label>
            <input
              type="checkbox"
              checked={typeset}
              onChange={(event) => setTypeset(event.target.checked)}
            />
            印刷用 HTML から PDF も組む（<code>{pdfCommand}</code>）
          </label>
        )}
      </div>
      {notice !== null && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {output !== null && <CommandOutput run={output} />}
      {problems.length > 0 && (
        <div className="notice notice-warn">
          <div>
            <p className="export-list-head">
              出力プロファイルの誤り（読み飛ばして書き出しました）
            </p>
            <ul className="export-list">
              {keyed(problems, (p) => p).map(([key, p]) => (
                <li key={key}>{p}</li>
              ))}
            </ul>
          </div>
        </div>
      )}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

/** 外部コマンドの標準出力と標準エラー（あれば）。 */
function CommandOutput({ run }: { readonly run: PluginRun }) {
  const stdout = run.stdout ?? "";
  const stderr = run.stderr ?? "";
  if (stdout === "" && stderr === "") return null;
  return (
    <details
      className="ai-preview-details"
      open={typeof run.error === "string"}
    >
      <summary>コマンドの出力</summary>
      {stderr !== "" && <pre className="ai-preview">{outputTail(stderr)}</pre>}
      {stdout !== "" && <pre className="ai-preview">{outputTail(stdout)}</pre>}
    </details>
  );
}

/**
 * 書き出しのプラグイン（T3-14、D075）：作品フォルダの export/<名前>.yaml（template か command）で書き出す（CLI の
 * `scn export <名前>` と同じ）。外部コマンドは、この端末で初めて実行する前と、コマンドが変わったときに確かめる。
 */
function PluginCard({
  summary,
  load,
  run,
  ask,
}: {
  readonly summary: Summary;
  readonly load: () => Promise<PluginItem[]>;
  readonly run: (name: string, approved: string | null) => Promise<PluginRun>;
  readonly ask: (message: string, title: string) => Promise<boolean>;
}) {
  const [plugins, setPlugins] = useState<readonly PluginItem[]>([]);
  const [reload, setReload] = useState(0);
  const [results, setResults] = useState<
    Readonly<Record<string, PluginRun | string>>
  >({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void summary;
    void reload;
    let live = true;
    load()
      .then((p) => {
        if (!live) return;
        setPlugins(p);
        setError(null);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, reload, load]);
  const start = (name: string) => {
    setBusy(name);
    runApproved((approved) => run(name, approved), ask)
      .then((r) =>
        setResults((all) => ({
          ...all,
          [name]: r ?? "コマンドを実行しませんでした",
        })),
      )
      .catch((e: unknown) =>
        setResults((all) => ({ ...all, [name]: String(e) })),
      )
      .finally(() => setBusy(null));
  };
  return (
    <section className="export-card" aria-label="書き出しのプラグイン">
      <div className="export-card-head">
        <div>
          <h2>書き出しのプラグイン</h2>
          <p className="export-note">
            作品フォルダの export/&lt;名前&gt;.yaml に template（minijinja
            のテンプレート）か command（IR の JSON
            を標準入力で受け取る外部コマンド）を書くと、ここから書き出せます（CLI
            の scn export &lt;名前&gt;
            と同じ）。外部コマンドは、この端末で初めて実行する前と、コマンドが変わったときに確かめます。
          </p>
        </div>
        <button
          type="button"
          className="btn btn-sm"
          onClick={() => setReload((n) => n + 1)}
        >
          一覧を読み直す
        </button>
      </div>
      {plugins.length === 0 && (
        <p className="export-note">
          プラグインのプロファイルはありません（例：export/cast.yaml に
          template: tools/cast.csv.j2）
        </p>
      )}
      {plugins.length > 0 && (
        <ul className="plugin-list">
          {plugins.map((p) => {
            const result = results[p.name];
            return (
              <li key={p.name} className="plugin-row">
                <div className="plugin-head">
                  <strong>{p.name}</strong>
                  <span className="plugin-kind">{pluginKindLabel(p.kind)}</span>
                  <code className="plugin-detail">
                    {p.kind === "error" ? p.profile : p.detail}
                  </code>
                  <span className="flow-spacer" />
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={busy !== null || p.kind === "error"}
                    onClick={() => start(p.name)}
                  >
                    {busy === p.name ? "書き出しています" : "書き出す"}
                  </button>
                </div>
                {p.kind !== "error" && (
                  <p className="export-note">書き出し先：{p.output}</p>
                )}
                {p.error !== null && (
                  <p className="notice notice-danger" role="alert">
                    {p.error}
                  </p>
                )}
                {typeof result === "string" && (
                  <p className="notice notice-danger" role="alert">
                    {result}
                  </p>
                )}
                {typeof result === "object" && (
                  <>
                    {pluginRunText(result) !== null && (
                      <p className="notice" role="status">
                        {pluginRunText(result)}
                      </p>
                    )}
                    {typeof result.error === "string" && (
                      <p className="notice notice-danger" role="alert">
                        {result.error}
                      </p>
                    )}
                    <CommandOutput run={result} />
                  </>
                )}
                {(typeof result === "object"
                  ? (result.problems ?? [])
                  : p.problems
                ).length > 0 && (
                  <div className="notice notice-warn">
                    <ul className="export-list">
                      {keyed(
                        typeof result === "object"
                          ? (result.problems ?? [])
                          : p.problems,
                        (x) => x,
                      ).map(([key, x]) => (
                        <li key={key}>{x}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

const COLUMNS = [
  "行ID",
  "シーン",
  "キャラ",
  "台詞",
  "直前の地の文",
  "ボイスファイル名",
  "備考",
] as const;

/** ゲームエンジン（Ren'Py、ティラノスクリプト）への書き出し。書き出した後に、知らせとプロファイルの誤りを並べる。 */
function EngineCard({
  name,
  note,
  unsaved,
  write,
}: {
  readonly name: string;
  readonly note: string;
  readonly unsaved: number;
  readonly write: () => Promise<RenpyResult>;
}) {
  const [result, setResult] = useState<RenpyResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  return (
    <section className="export-card" aria-label={name}>
      <div className="export-card-head">
        <div>
          <h2>{name}</h2>
          <p className="export-note">{note}</p>
        </div>
        <button
          type="button"
          className="btn btn-primary"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            write()
              .then((r) => {
                setResult(r);
                setError(null);
              })
              .catch((e: unknown) => setError(String(e)))
              .finally(() => setBusy(false));
          }}
        >
          書き出す
        </button>
      </div>
      {result !== null && (
        <p className="notice" role="status">
          {result.path} に書き出しました（{result.files}ファイル）
        </p>
      )}
      {error !== null && (
        <p className="notice notice-danger" role="alert">
          {error}
        </p>
      )}
      {unsaved > 0 && (
        <p className="notice notice-warn">
          保存していないファイルが {unsaved}{" "}
          あります。保存していない本文も含めて書き出します。
        </p>
      )}
      {result !== null && result.problems.length > 0 && (
        <div className="notice notice-warn">
          <div>
            <p className="export-list-head">
              出力プロファイルの誤り（読み飛ばして書き出しました）
            </p>
            <ul className="export-list">
              {keyed(result.problems, (p) => p).map(([key, p]) => (
                <li key={key}>{p}</li>
              ))}
            </ul>
          </div>
        </div>
      )}
      {result !== null && result.warnings.length > 0 && (
        <div className="export-info">
          <p className="export-list-head">知らせ</p>
          <ul className="export-list">
            {keyed(result.warnings, (w) => w).map(([key, w]) => (
              <li key={key}>{w}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

export function ExportScreen({
  summary,
  unsaved,
  load,
  write,
  writeRenpy,
  writeTyrano,
  writeUtage,
  writeBook,
  loadBookSettings,
  loadPlugins,
  runPlugin,
  ask,
  loadChoices,
  writeText,
  loadProvenance,
  writeProvenance,
}: Props) {
  const [sheet, setSheet] = useState<ScriptSheet | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [format, setFormat] = useState<ScriptFormat>("csv");
  const [baseline, setBaseline] = useState(false);
  /** 書き出した回数（基準を記録したら、台本を取り直す）。 */
  const [written, setWritten] = useState(0);
  /** AI下書き（status: ai-draft）のシーンを含めるか（T3-9）。 */
  const [aiDrafts, setAiDrafts] = useState(true);
  /** この窓で最後に書き出したもの（ステータスバー。T4-17、U05）。 */
  const [last, setLast] = useState<string | null>(null);
  useScreenStatus("export", exportStatus(last));
  /** 書き出しを覚える。何も書かなかった回（外部コマンドの確かめ待ちなど）は覚えない。 */
  const track = <R,>(
    name: string,
    promise: Promise<R>,
    wrote: (r: R) => boolean = () => true,
  ): Promise<R> =>
    promise.then((r) => {
      if (wrote(r)) setLast(`${clockText(new Date())} · ${name}`);
      return r;
    });
  const pluginWrote = (r: PluginRun) =>
    r.needs_approval === undefined && (r.path ?? null) !== null;

  useEffect(() => {
    void summary;
    void written;
    let live = true;
    load(aiDrafts)
      .then((s) => {
        if (live) {
          setSheet(s);
          setError(null);
        }
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, load, written, aiDrafts]);

  return (
    <div className="export-screen-body">
      <header className="export-head">
        <h1>出力</h1>
        <span className="flow-spacer" />
        <label className="export-ai-drafts">
          <input
            type="checkbox"
            checked={aiDrafts}
            onChange={(e) => setAiDrafts(e.target.checked)}
          />
          AI下書き（status: ai-draft）のシーンを含める
        </label>
      </header>
      <EngineCard
        name="Ren'Py"
        note="出力プロファイル（export/renpy.yaml）に従って、Ren'Py のスクリプトを書き出します。書き出し先（既定は export/renpy）の game/scn/ だけを毎回作り直し、ほかのファイルには触れません。書き出したものは手で直さず、本文を直して書き出し直してください。"
        unsaved={unsaved}
        write={() => track("Ren'Py", writeRenpy(aiDrafts))}
      />
      <EngineCard
        name="ティラノスクリプト"
        note={
          '出力プロファイル（export/tyrano.yaml）に従って、ティラノスクリプトのシナリオを書き出します。書き出し先（既定は export/tyrano）の data/scenario/scn/ と素材の data/*/scn/ だけを毎回作り直し、ほかのファイルには触れません。first.ks か title.ks から [jump storage="scn/scn_start.ks"] で始めます（プロファイルの first: true なら first.ks も書きます）。'
        }
        unsaved={unsaved}
        write={() => track("ティラノスクリプト", writeTyrano(aiDrafts))}
      />
      <EngineCard
        name="宴"
        note="出力プロファイル（export/utage.yaml）に従って、宴のシナリオ（シーンごとのシート）と設定のシート（Character・Texture・Sound・Param）を Excel 形式で書き出します。書き出し先（既定は export/utage）をまるごと作り直します。宴のプロジェクトへは、シナリオのブックを足し、設定のシートの行と Texture・Sound のファイルを取り込みます（作品の Start のシートから Jump *scn_start で始めます）。"
        unsaved={unsaved}
        write={() => track("宴", writeUtage(aiDrafts))}
      />
      <TextCard
        summary={summary}
        loadChoices={loadChoices}
        writeText={(target, notes) =>
          track("プレーンテキスト", writeText(target, notes, aiDrafts))
        }
      />
      <BookCard
        summary={summary}
        loadChoices={loadChoices}
        loadSettings={loadBookSettings}
        writeBook={(target, print, horizontal, notes, typeset, approved) =>
          track(
            print ? "縦書きPDF" : "EPUB",
            writeBook(
              target,
              print,
              horizontal,
              notes,
              aiDrafts,
              typeset,
              approved,
            ),
            pluginWrote,
          )
        }
        ask={ask}
      />
      <PluginCard
        summary={summary}
        load={loadPlugins}
        run={(name, approved) =>
          track(name, runPlugin(name, aiDrafts, approved), pluginWrote)
        }
        ask={ask}
      />
      <ProvenanceCard
        summary={summary}
        load={loadProvenance}
        write={writeProvenance}
      />
      <section className="export-card" aria-label="台本">
        <div className="export-card-head">
          <div>
            <h2>台本</h2>
            <p className="export-note">
              収録と校正の表。1行が1つの台詞で、ボイスファイル名は「キャラID_行ID」。CSV
              は UTF-8（BOM付き）と CRLF で書くので、Excel
              でそのまま開けます。Excel
              形式は、欄をすべて文字列にし、見出しの行を固定します。書き出し先：export/script.csv・export/script.xlsx
            </p>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || sheet === null}
            onClick={() => {
              setBusy(true);
              track("収録の台本", write(format, baseline, aiDrafts))
                .then((r) => {
                  setNotice(
                    `${r.path} に書き出しました（${r.rows}行）${baseline ? "。収録の基準として記録しました" : ""}`,
                  );
                  setError(null);
                  if (baseline) {
                    setBaseline(false);
                    setWritten((n) => n + 1);
                  }
                })
                .catch((e: unknown) => setError(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            書き出す
          </button>
        </div>
        <div className="export-options">
          <label>
            形式
            <select
              value={format}
              onChange={(event) =>
                setFormat(event.target.value === "xlsx" ? "xlsx" : "csv")
              }
            >
              <option value="csv">CSV</option>
              <option value="xlsx">Excel（.xlsx）</option>
            </select>
          </label>
          <label>
            <input
              type="checkbox"
              checked={baseline}
              onChange={(event) => setBaseline(event.target.checked)}
            />
            収録の基準として記録する（次から、変わった台詞を再収録候補にする）
          </label>
        </div>
        {notice !== null && (
          <p className="notice" role="status">
            {notice}
          </p>
        )}
        {error !== null && (
          <p className="notice notice-danger" role="alert">
            {error}
          </p>
        )}
        {sheet !== null && sheet.retakes !== null && (
          <p className="export-info">
            収録の基準（export/script-baseline.json）から：変わった台詞{" "}
            {sheet.retakes.changed} 行（備考に「再収録候補」）、足した台詞{" "}
            {sheet.retakes.added} 行、無くなった台詞 {sheet.retakes.removed} 行
            {sheet.retakes.removed_ids.length > 0 &&
              `（${sheet.retakes.removed_ids.join(" ")}）`}
          </p>
        )}
        {sheet !== null && sheet.baseline_problem !== null && (
          <p className="notice notice-warn">
            {sheet.baseline_problem}（再収録候補を付けずに作りました）
          </p>
        )}
        {unsaved > 0 && (
          <p className="notice notice-warn">
            保存していないファイルが {unsaved}{" "}
            あります。台本は、保存していない本文も含めて作ります（保存すると、行IDの無い行に行IDが付きます）。
          </p>
        )}
        {sheet !== null && sheet.missing_line_ids > 0 && (
          <p className="notice notice-warn">
            行IDの無い台詞が {sheet.missing_line_ids}{" "}
            行あります。ボイスファイル名を付けられないので空にします（保存すると行IDが付きます）。
          </p>
        )}
        {sheet === null ? (
          <p className="placeholder">台本を作っています…</p>
        ) : sheet.rows.length === 0 ? (
          <p className="placeholder">台詞がありません。</p>
        ) : (
          <div className="bible-table-wrap export-table-wrap">
            <table className="bible-table export-table">
              <thead>
                <tr>
                  {COLUMNS.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {keyed(
                  sheet.rows,
                  (r) => `${r.line_id}:${r.scene}:${r.text}`,
                ).map(([key, r]) => (
                  <tr key={key}>
                    <td className="mono">{r.line_id}</td>
                    <td>{r.scene}</td>
                    <td>{r.character}</td>
                    <td className="export-text">{r.text}</td>
                    <td className="export-text">{r.narration}</td>
                    <td className="mono">{r.voice}</td>
                    <td>{r.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}

/** 時刻（時:分）。 */
function clockText(d: Date): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
