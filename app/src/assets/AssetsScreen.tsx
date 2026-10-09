// 素材カタログの画面（T2-3、D054。docs/ui/mockups/05-assets.html）：意味名ごとの状態（ファイルあり・
// 仮素材・未登録）、使用回数、初出、立ち絵のキャラ×差分の表、使用シーン。未登録の意味名をカタログに足し、
// `assets/` の中のファイルを割り当て、発注リスト（CSV）を書き出す。カタログはコアがテキストの行を足すか
// 1行を差し替えて書く（ほかの行は変えない）。

import { useEffect, useMemo, useState } from "react";
import {
  addCatalogEntries,
  assetList,
  assetLocation,
  assetUrl,
  assignAssetFile,
  copyText,
  exportOrders,
  pickAssetFile,
} from "../core/api.ts";
import type {
  AssetList,
  AssetRow,
  CatalogWritten,
  SceneSummary,
  Summary,
} from "../core/types.ts";
import { useScreenStatus } from "../shell/StatusBar.tsx";
import { assetsStatus } from "../shell/status.ts";
import {
  type AssetTab,
  addableKeys,
  assetKey,
  countAssets,
  displayName,
  expressionMatrix,
  filterRows,
  KIND_LABEL,
  ORDER_COLUMNS,
  rowId,
  STATE_LABEL,
  TABS,
  usesByScene,
} from "./model.ts";

const FILE = "assets/catalog.yaml";

interface Props {
  /** 作品フォルダ（ファイルを選ぶときの初めの場所）。 */
  readonly root: string;
  /** 本文が変わったら、一覧を取り直す（使用回数）。 */
  readonly summary: Summary;
  readonly scenes: readonly SceneSummary[];
  /** 設定ファイルが外で変わったら増える。 */
  readonly settingsVersion: number;
  /** 画面を見せた回数（`assets/` に外で置いたファイルを、見せるたびに読み直す。T4-13）。 */
  readonly shown: number;
  /** カタログを書いた後（補完の材料と解析の結果を入れる）。 */
  readonly onWritten: (result: CatalogWritten) => void;
  /** 使用箇所へ移る。 */
  readonly onOpenLocation: (file: string, offset: number) => void;
}

/** 状態の見た目のクラス（`placeholder` は全体の段落のクラスと重なるので使わない）。 */
const STATE_CLASS: Readonly<Record<AssetRow["state"], string>> = {
  file: "file",
  placeholder: "provisional",
  unregistered: "unregistered",
};

/** 状態の札の部品のクラス（見本 v1：仮素材は琥珀の点線、未登録は琥珀の実線）。 */
const TAG_CLASS: Readonly<Record<AssetRow["state"], string>> = {
  file: "tag",
  placeholder: "tag tag-ph",
  unregistered: "tag tag-warn",
};

function StateBadge({ row }: { readonly row: AssetRow }) {
  if (row.state === "file") {
    return <span className="assets-state-file">{STATE_LABEL.file}</span>;
  }
  return <span className={TAG_CLASS[row.state]}>{STATE_LABEL[row.state]}</span>;
}

function fileText(row: AssetRow): string {
  if (row.state === "unregistered") return "本文で使用、カタログに無し";
  if (row.file === null) return "—";
  return row.path === null ? `${row.file}（見つからない）` : row.file;
}

export function AssetsScreen({
  root,
  summary,
  scenes,
  settingsVersion,
  shown,
  onWritten,
  onOpenLocation,
}: Props) {
  const [list, setList] = useState<AssetList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** 素材のフォルダ（`assets/`）の絶対のパス（T3-1、H5。Finder で探さずに済むように見せる）。 */
  const [folder, setFolder] = useState<string | null>(null);
  useEffect(() => {
    void root;
    let live = true;
    assetLocation(null, false)
      .then((path) => {
        if (live) setFolder(path);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [root]);

  /** 素材の場所を OS のファイルの画面で開く（`file` が null なら素材のフォルダ）。 */
  const reveal = (file: string | null) => {
    assetLocation(file, true)
      .then(() => setError(null))
      .catch((e: unknown) => setError(String(e)));
  };

  /** パスをコピーする（押したその場で写す。待ってからだと、クリップボードに書かせてもらえないことがある）。 */
  const copyPath = (path: string | null) => {
    if (path === null) return;
    void copyText(path).then((ok) => {
      if (ok) {
        setNotice(`パスをコピーしました：${path}`);
        setError(null);
      } else {
        setError(
          `コピーできませんでした。パスを選んでコピーしてください：${path}`,
        );
      }
    });
  };
  const [tab, setTab] = useState<AssetTab>("all");
  const [missingOnly, setMissingOnly] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(null);

  useEffect(() => {
    void summary;
    void settingsVersion;
    void shown;
    let live = true;
    assetList()
      .then((l) => {
        if (live) setList(l);
      })
      .catch((e: unknown) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [summary, settingsVersion, shown]);

  const rows = useMemo(
    () => filterRows(list?.rows ?? [], { tab, missingOnly, query }),
    [list, tab, missingOnly, query],
  );
  const counts = useMemo(() => countAssets(list?.rows ?? []), [list]);
  useScreenStatus(
    "assets",
    assetsStatus({
      files: counts.registered - counts.placeholders,
      placeholders: counts.placeholders,
      unregistered: counts.unregistered,
    }),
  );
  const matrix = useMemo(
    () => (list === null ? null : expressionMatrix(list)),
    [list],
  );
  const addable = useMemo(() => addableKeys(list?.rows ?? []), [list]);
  const row = list?.rows.find((r) => rowId(r) === selected) ?? null;
  /** 選んだ素材の場所（ファイルか、置く場所のフォルダ。先に求めて見せておき、コピーはその場で写す）。 */
  const rowFile = row?.file ?? null;
  const rowPath = row?.path ?? null;
  const [rowLocation, setRowLocation] = useState<string | null>(null);
  useEffect(() => {
    void root;
    void rowPath; // ファイルが置かれたら（一覧の読み直し）、求め直す。
    setRowLocation(null);
    if (rowFile === null) return;
    let live = true;
    assetLocation(rowFile, false)
      .then((path) => {
        if (live) setRowLocation(path);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [rowFile, rowPath, root]);
  const titles = useMemo(
    () => new Map(scenes.map((s) => [s.id, s.title])),
    [scenes],
  );

  /** カタログを書く。書けなければ、理由を出す。 */
  const write = (run: () => Promise<CatalogWritten>, done: string) => {
    setBusy(true);
    run()
      .then((result) => {
        setList(result.assets);
        onWritten(result);
        switch (result.edit.kind) {
          case "saved":
            setNotice(done);
            setError(null);
            break;
          case "unchanged":
            setNotice("変わりませんでした（もうあります）");
            setError(null);
            break;
          case "unsupported":
            setNotice(null);
            setError(result.edit.message);
            break;
        }
      })
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  };

  const assign = (target: AssetRow) => {
    setBusy(true);
    pickAssetFile(root, target.kind)
      .then((path) => {
        if (path === null) {
          setBusy(false);
          return;
        }
        write(
          () => assignAssetFile(assetKey(target), path),
          `${displayName(target)} にファイルを割り当てました`,
        );
      })
      .catch((e: unknown) => {
        setError(String(e));
        setBusy(false);
      });
  };

  return (
    <div className="assets-screen-body">
      <main className="assets-main" aria-label="素材の一覧">
        <header className="assets-head">
          <div className="assets-title">
            <h1>素材カタログ</h1>
            <span className="bible-path">{FILE}</span>
          </div>
          <div className="assets-counts">
            <span className="assets-count">登録 {counts.registered}</span>
            <span className="assets-count provisional">
              仮素材 {counts.placeholders}
            </span>
            <span className="assets-count unregistered">
              未登録の参照 {counts.unregistered}
            </span>
          </div>
          <span className="flow-spacer" />
          <button
            type="button"
            className="btn"
            disabled={busy || addable.length === 0}
            title={
              addable.length === 0
                ? "カタログに足せる未登録の意味名はありません"
                : undefined
            }
            onClick={() =>
              write(
                () => addCatalogEntries(addable),
                `未登録の意味名をカタログに足しました（${addable.length}件。仮素材）`,
              )
            }
          >
            未登録をカタログに追加
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || list === null}
            onClick={() => {
              setBusy(true);
              exportOrders()
                .then((r) => {
                  setNotice(
                    `${r.path} に発注リストを書き出しました（${r.rows}行）`,
                  );
                  setError(null);
                })
                .catch((e: unknown) => setError(String(e)))
                .finally(() => setBusy(false));
            }}
          >
            発注リストをCSV出力
          </button>
        </header>
        <div className="assets-folder">
          <span className="assets-folder-label">素材のフォルダ</span>
          <code className="assets-folder-path" title={folder ?? undefined}>
            {folder ?? "assets/"}
          </code>
          <button
            type="button"
            className="btn"
            title="素材のフォルダを Finder（Windows はエクスプローラー）で開く。無ければ作る"
            onClick={() => reveal(null)}
          >
            フォルダを開く
          </button>
          <button
            type="button"
            className="btn"
            title="素材のフォルダのパスをクリップボードにコピーする"
            disabled={folder === null}
            onClick={() => copyPath(folder)}
          >
            パスをコピー
          </button>
        </div>
        <div className="assets-toolbar">
          <fieldset className="mode-switch" aria-label="種別">
            {TABS.map((t) => (
              <button
                key={t.tab}
                type="button"
                aria-pressed={tab === t.tab}
                onClick={() => setTab(t.tab)}
              >
                {t.label}
              </button>
            ))}
          </fieldset>
          <label className="assets-check">
            <input
              type="checkbox"
              checked={missingOnly}
              onChange={(event) => setMissingOnly(event.target.checked)}
            />
            仮素材と未登録のみ
          </label>
          <label className="bible-search assets-search">
            <span aria-hidden="true">⌕</span>
            <input
              type="search"
              aria-label="素材を検索"
              placeholder="意味名で検索"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        </div>
        {notice !== null && (
          <p className="assets-notice" role="status">
            {notice}
          </p>
        )}
        {error !== null && (
          <p className="assets-error" role="alert">
            {error}
          </p>
        )}
        {list === null ? (
          <p className="placeholder">素材を読んでいます…</p>
        ) : list.rows.length === 0 ? (
          <p className="placeholder">
            素材がありません。本文の @背景・@立ち絵・@BGM・@SE
            で使うと、ここに出ます。
          </p>
        ) : (
          <div className="assets-table-scroll">
            <table className="bible-table assets-table">
              <thead>
                <tr>
                  <th>種別</th>
                  <th>意味名</th>
                  <th>ファイル</th>
                  <th>状態</th>
                  <th className="number">使用回数</th>
                  <th>初出</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const id = rowId(r);
                  const classes = [
                    id === selected ? "selected" : "",
                    r.state === "unregistered" ? "unregistered" : "",
                  ]
                    .filter((c) => c !== "")
                    .join(" ");
                  return (
                    <tr key={id} className={classes || undefined}>
                      <td className="assets-kind">{KIND_LABEL[r.kind]}</td>
                      <td>
                        <button
                          type="button"
                          className="assets-name"
                          onClick={() => setSelected(id)}
                        >
                          {displayName(r)}
                        </button>
                      </td>
                      <td
                        className={
                          r.file !== null && r.state !== "unregistered"
                            ? "mono assets-file"
                            : "assets-file muted"
                        }
                      >
                        {fileText(r)}
                      </td>
                      <td>
                        <StateBadge row={r} />
                      </td>
                      <td className="number">{r.uses.length}</td>
                      <td className="mono">{r.scenes[0] ?? "—"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {rows.length === 0 && (
              <p className="placeholder">条件に合う素材がありません。</p>
            )}
            <p className="assets-note">
              意味名は本文の <span className="mono">@背景 屋上/夕方</span>{" "}
              などと一致します。ファイル名とエンジン側の名前への対応は、出力プロファイルで決めます。
            </p>
          </div>
        )}
      </main>
      <aside
        className="bible-usage assets-side ctx-column"
        aria-label="立ち絵と選んだ素材"
      >
        <h2 className="bible-usage-heading">立ち絵 · キャラ×差分</h2>
        {matrix === null || matrix.columns.length === 0 ? (
          <p className="context-empty">立ち絵の差分がありません。</p>
        ) : (
          <>
            <p className="assets-side-note">セルの数字は本文での使用回数</p>
            <div className="assets-matrix-scroll">
              <table className="assets-matrix">
                <thead>
                  <tr>
                    <th />
                    {matrix.columns.map((c) => (
                      <th key={c} scope="col">
                        {c}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {matrix.rows.map((m) => (
                    <tr key={m.id}>
                      <th scope="row">
                        {m.name}
                        {!m.registered && (
                          <span className="assets-matrix-unknown">
                            （未登録）
                          </span>
                        )}
                      </th>
                      {m.cells.map((cell, i) => {
                        const column = matrix.columns[i] ?? "";
                        if (cell === null) {
                          return (
                            <td key={column} className="assets-cell-none">
                              —
                            </td>
                          );
                        }
                        const id = rowId(cell.row);
                        return (
                          <td key={column} className="assets-cell-wrap">
                            <button
                              type="button"
                              className={`assets-cell ${STATE_CLASS[cell.row.state]}${id === selected ? " selected" : ""}`}
                              title={`${displayName(cell.row)}：${STATE_LABEL[cell.row.state]}、${cell.uses}回`}
                              onClick={() => setSelected(id)}
                            >
                              {cell.uses}
                              {cell.row.state === "placeholder" && (
                                <span className="assets-cell-mark">仮</span>
                              )}
                              {cell.row.state === "unregistered" && (
                                <span className="assets-cell-mark">未</span>
                              )}
                            </button>
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="assets-legend">
              <span>
                <span className="assets-swatch file" aria-hidden="true" />
                あり
              </span>
              <span>
                <span
                  className="assets-swatch provisional"
                  aria-hidden="true"
                />
                仮素材
              </span>
              <span>
                <span
                  className="assets-swatch unregistered"
                  aria-hidden="true"
                />
                未登録
              </span>
              <span>— 無い差分</span>
            </div>
          </>
        )}

        <h2 className="bible-usage-heading">
          {row === null
            ? "選択中の素材"
            : `選択中：${KIND_LABEL[row.kind]} ${displayName(row)}`}
        </h2>
        {row === null ? (
          <p className="context-empty">
            素材を選ぶと、使用シーンとファイルの割り当てが出ます。
          </p>
        ) : (
          <div className="assets-detail">
            <p>
              <StateBadge row={row} />{" "}
              {row.state === "file" && row.file !== null && (
                <span className="mono">{row.file}</span>
              )}
              {row.state === "placeholder" &&
                (row.file === null
                  ? "ファイルがまだありません"
                  : `${row.file} が見つかりません`)}
              {row.state === "unregistered" && "カタログに無い意味名です"}
            </p>
            {rowLocation !== null && (
              <p className="assets-location" title={rowLocation}>
                {row.path !== null ? "場所" : "置く場所"}：
                <code>{rowLocation}</code>
              </p>
            )}
            {row.path !== null &&
              (row.kind === "background" || row.kind === "expression") && (
                <img
                  className="assets-thumb"
                  src={assetUrl(row.path)}
                  alt={displayName(row)}
                />
              )}
            {row.path !== null && (row.kind === "bgm" || row.kind === "se") && (
              // biome-ignore lint/a11y/useMediaCaption: 素材の音を確かめるだけで、字幕にする言葉が無い
              <audio
                className="assets-audio"
                controls
                src={assetUrl(row.path)}
              />
            )}
            {row.state !== "file" && (
              <p className="assets-muted">
                {row.kind === "bgm" || row.kind === "se"
                  ? "プレビューでは鳴らしません（無音）"
                  : "プレビューでは名前入りの色の矩形で見せます"}
              </p>
            )}
            <div className="assets-actions">
              {row.addable && (
                <button
                  type="button"
                  className="btn"
                  disabled={busy}
                  onClick={() =>
                    write(
                      () => addCatalogEntries([assetKey(row)]),
                      `${displayName(row)} をカタログに足しました（仮素材）`,
                    )
                  }
                >
                  カタログに追加
                </button>
              )}
              {row.state !== "unregistered" && (
                <button
                  type="button"
                  className="btn"
                  disabled={busy}
                  onClick={() => assign(row)}
                >
                  ファイルを割り当てる
                </button>
              )}
              <button
                type="button"
                className="btn"
                title={
                  row.path !== null
                    ? "このファイルを Finder（Windows はエクスプローラー）で選んだ状態で開く"
                    : "このファイルを置く場所（無ければ、いちばん近いフォルダ）を開く"
                }
                onClick={() => reveal(row.file)}
              >
                {row.path !== null ? "場所を開く" : "置く場所を開く"}
              </button>
              <button
                type="button"
                className="btn"
                title="このファイル（無ければ、置く場所のフォルダ）のパスをコピーする"
                disabled={rowLocation === null}
                onClick={() => copyPath(rowLocation)}
              >
                パスをコピー
              </button>
            </div>
            {row.state === "unregistered" && !row.addable && (
              <p className="assets-muted">
                キャラクターが設定DBに無いので、カタログに足せません。先に設定DBにキャラクターを登録してください。
              </p>
            )}
            {usesByScene(row).length === 0 ? (
              <p className="context-empty">本文で使われていません。</p>
            ) : (
              <ul className="bible-usage-scenes">
                {usesByScene(row).map((s) => (
                  <li key={s.scene}>
                    <button
                      type="button"
                      className="bible-usage-scene"
                      onClick={() => onOpenLocation(s.file, s.offset)}
                    >
                      <span className="bible-usage-id">{s.scene}</span>
                      <span className="bible-usage-title">
                        {titles.get(s.scene) ?? ""}
                      </span>
                      <span className="bible-usage-count">{s.count}回</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        <h2 className="bible-usage-heading">発注リスト（CSV）の列</h2>
        <p className="assets-orders mono">{ORDER_COLUMNS.join(",")}</p>
        <p className="assets-side-note">
          仮素材と未登録の素材を1行ずつ書きます。書き出し先：export/orders.csv
        </p>
      </aside>
    </div>
  );
}
