// 外部の変更と、保存していない変更がぶつかったときの知らせ。読み込み直す、自分の変更を残す、マージする
// （T2-7、D058。外で消されたときはマージできない）から選ぶ。

interface Props {
  readonly file: string;
  /** 外で消されたか。 */
  readonly deleted: boolean;
  readonly onReload: () => void;
  readonly onKeep: () => void;
  readonly onMerge: () => void;
}

export function ConflictBar({
  file,
  deleted,
  onReload,
  onKeep,
  onMerge,
}: Props) {
  return (
    <div className="conflict-bar" role="alert">
      <span>
        {file} が外部で{deleted ? "削除" : "変更"}
        されました。このファイルには保存していない変更があります。
      </span>
      <button type="button" className="btn btn-sm" onClick={onReload}>
        {deleted ? "閉じる" : "読み込み直す"}
      </button>
      <button type="button" className="btn btn-sm" onClick={onKeep}>
        自分の変更を残す
      </button>
      {!deleted && (
        <button type="button" className="btn btn-sm" onClick={onMerge}>
          マージする
        </button>
      )}
    </div>
  );
}
