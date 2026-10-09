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
import { Icon } from "../shell/icons.tsx";
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

/** キャラ×差分の表のマスのクラス（見本 v1 の .mx：あり・仮素材は琥珀の点線・未登録は琥珀の実線）。 */
const MX_CLASS: Readonly<Record<AssetRow["state"], string>> = {
  file: "has",
  placeholder: "ph",
  unregistered: "unreg",
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
  return (
    <span className={TAG_CLASS[row.state]}>
      {row.state === "unregistered" && (
        <Icon name="warning" size={11} stroke={2.2} />
      )}
      {STATE_LABEL[row.state]}
    </span>
  );
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

  const firstUse = row === null ? null : (usesByScene(row)[0] ?? null);
  return (
    <div className="assets-screen-body">
      <main className="assets-main" aria-label="素材の一覧">
        <header className="work-head assets-head">
          <h1 className="h1">素材カタログ</h1>
          <span className="bible-path">{FILE}</span>
          <span className="tag">
            ファイルあり {counts.registered - counts.placeholders}
          </span>
          <span className="tag tag-ph">仮素材 {counts.placeholders}</span>
          <span className="tag tag-warn">
            <Icon name="warning" size={11} stroke={2.2} />
            未登録 {counts.unregistered}
          </span>
          <span className="grow" />
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
            未登録をカタログに足す
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
            発注リストを書き出す
          </button>
        </header>
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
          <label className="check">
            <input
              type="checkbox"
              checked={missingOnly}
              onChange={(event) => setMissingOnly(event.target.checked)}
            />
            仮素材と未登録だけ
          </label>
          <span className="grow" />
          <span className="assets-folder" title={folder ?? undefined}>
            <Icon name="folder" size={14} />
            <span className="assets-folder-path">{folder ?? "assets/"}</span>
          </span>
          <button
            type="button"
            className="btn btn-sm"
            title="素材のフォルダを Finder（Windows はエクスプローラー）で開く。無ければ作る"
            onClick={() => reveal(null)}
          >
            フォルダを開く
          </button>
          <button
            type="button"
            className="btn btn-sm"
            title="素材のフォルダのパスをクリップボードにコピーする"
            disabled={folder === null}
            onClick={() => copyPath(folder)}
          >
            パスをコピー
          </button>
          <label className="search assets-search">
            <Icon name="search" size={14} />
            <input
              type="search"
              aria-label="意味名で絞り込む"
              placeholder="意味名で絞り込む"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
        </div>
        {notice !== null && (
          <p className="notice assets-band" role="status">
            {notice}
          </p>
        )}
        {error !== null && (
          <p className="notice notice-danger assets-band" role="alert">
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
            <table className="table assets-table">
              <thead>
                <tr>
                  <th>種別</th>
                  <th>意味名</th>
                  <th>ファイル</th>
                  <th>状態</th>
                  <th className="num">使用</th>
                  <th>初出</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const id = rowId(r);
                  const classes = [
                    id === selected ? "is-selected" : "",
                    r.state === "unregistered" ? "is-unregistered" : "",
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
                      <td className="num">{r.uses.length}</td>
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
      <aside className="assets-side ctx-column" aria-label="選んだ素材と立ち絵">
        <div className="panel-head">
          <h2 className="h-label">選んだ素材</h2>
        </div>
        <section className="section assets-detail" aria-label="選んだ素材">
          {row === null ? (
            <p className="empty">
              素材を選ぶと、使っているシーンとファイルの割り当てが出ます。
            </p>
          ) : (
            <>
              <div className="assets-detail-head">
                <span className="t-2">{KIND_LABEL[row.kind]}</span>
                <strong className="assets-detail-name">
                  {displayName(row)}
                </strong>
                <span className="grow" />
                <StateBadge row={row} />
              </div>
              <dl className="kv">
                <dt>ファイル</dt>
                <dd>
                  {row.state === "file" && row.file !== null ? (
                    <span className="mono">{row.file}</span>
                  ) : row.state === "placeholder" ? (
                    row.file === null ? (
                      `まだ無い（${row.kind === "bgm" || row.kind === "se" ? "プレビューでは無音" : "プレビューでは名前入りの色の矩形"}）`
                    ) : (
                      `${row.file} が見つかりません`
                    )
                  ) : (
                    "カタログに無い意味名です"
                  )}
                </dd>
                {rowLocation !== null && (
                  <>
                    <dt>{row.path !== null ? "場所" : "置く場所"}</dt>
                    <dd className="mono assets-location" title={rowLocation}>
                      {rowLocation}
                    </dd>
                  </>
                )}
                <dt>使っている</dt>
                <dd>
                  {usesByScene(row).length === 0
                    ? "本文で使われていません"
                    : usesByScene(row).map((s, i) => (
                        <span key={s.scene}>
                          {i > 0 && "・"}
                          <button
                            type="button"
                            className="btn-link-inline"
                            onClick={() => onOpenLocation(s.file, s.offset)}
                          >
                            {s.scene} {titles.get(s.scene) ?? ""}
                          </button>
                          （{s.count}回）
                        </span>
                      ))}
                </dd>
              </dl>
              {row.path !== null &&
                (row.kind === "background" || row.kind === "expression") && (
                  <img
                    className="assets-thumb"
                    src={assetUrl(row.path)}
                    alt={displayName(row)}
                  />
                )}
              {row.path !== null &&
                (row.kind === "bgm" || row.kind === "se") && (
                  // biome-ignore lint/a11y/useMediaCaption: 素材の音を確かめるだけで、字幕にする言葉が無い
                  <audio
                    className="assets-audio"
                    controls
                    src={assetUrl(row.path)}
                  />
                )}
              <div className="assets-actions">
                {row.addable && (
                  <button
                    type="button"
                    className="btn btn-sm"
                    disabled={busy}
                    onClick={() =>
                      write(
                        () => addCatalogEntries([assetKey(row)]),
                        `${displayName(row)} をカタログに足しました（仮素材）`,
                      )
                    }
                  >
                    カタログに足す
                  </button>
                )}
                {row.state !== "unregistered" && (
                  <button
                    type="button"
                    className="btn btn-sm"
                    disabled={busy}
                    onClick={() => assign(row)}
                  >
                    ファイルを割り当てる
                  </button>
                )}
                {firstUse !== null && (
                  <button
                    type="button"
                    className="btn btn-link btn-sm"
                    onClick={() =>
                      onOpenLocation(firstUse.file, firstUse.offset)
                    }
                  >
                    本文で見る
                  </button>
                )}
                <button
                  type="button"
                  className="btn btn-sm"
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
                  className="btn btn-sm"
                  title="このファイル（無ければ、置く場所のフォルダ）のパスをコピーする"
                  disabled={rowLocation === null}
                  onClick={() => copyPath(rowLocation)}
                >
                  パスをコピー
                </button>
              </div>
              {row.state === "unregistered" && !row.addable && (
                <p className="empty">
                  キャラクターが設定DBに無いので、カタログに足せません。先に設定DBにキャラクターを登録してください。
                </p>
              )}
            </>
          )}
        </section>
        <section className="section" aria-label="立ち絵・キャラ×差分">
          <h3 className="h-label">立ち絵・キャラ×差分</h3>
          {matrix === null || matrix.columns.length === 0 ? (
            <p className="empty">立ち絵の差分がありません。</p>
          ) : (
            <>
              <p className="t-caption t-3 assets-side-note">
                数字は本文で使っている回数
              </p>
              <div className="assets-matrix-scroll">
                <table className="mx">
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
                              <td key={column} className="none">
                                —
                              </td>
                            );
                          }
                          const id = rowId(cell.row);
                          return (
                            <td key={column} className="mx-cell">
                              <button
                                type="button"
                                className={`mx-button ${MX_CLASS[cell.row.state]}${id === selected ? " is-selected" : ""}`}
                                title={`${displayName(cell.row)}：${STATE_LABEL[cell.row.state]}、${cell.uses}回`}
                                onClick={() => setSelected(id)}
                              >
                                {cell.row.state === "unregistered" && (
                                  <Icon name="warning" size={10} stroke={2.4} />
                                )}
                                {cell.uses}
                              </button>
                            </td>
                          );
                        })}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div className="assets-legend t-caption t-3">
                <span>
                  <span className="mx-swatch has" aria-hidden="true" />
                  あり
                </span>
                <span>
                  <span className="mx-swatch ph" aria-hidden="true" />
                  仮素材
                </span>
                <span>
                  <span className="mx-swatch unreg" aria-hidden="true" />
                  未登録
                </span>
                <span>— 差分なし</span>
              </div>
            </>
          )}
        </section>
        <section className="section" aria-label="発注リスト">
          <h3 className="h-label">発注リスト</h3>
          <p className="t-small t-2 assets-orders-lead">
            仮素材 {counts.placeholders}件と未登録 {counts.unregistered}
            件を、1行1件で書き出す。
          </p>
          <dl className="kv">
            <dt>書き出し先</dt>
            <dd className="mono">export/orders.csv</dd>
            <dt>列</dt>
            <dd>{ORDER_COLUMNS.join("・")}</dd>
          </dl>
        </section>
      </aside>
    </div>
  );
}
