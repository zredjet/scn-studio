// 外部の変更と、保存していない変更がぶつかったときの知らせ。読み込み直すか、自分の変更を残すかを選ぶ
// （マージの画面はフェーズ2）。

interface Props {
  readonly file: string;
  /** 外で消されたか。 */
  readonly deleted: boolean;
  readonly onReload: () => void;
  readonly onKeep: () => void;
}

export function ConflictBar({ file, deleted, onReload, onKeep }: Props) {
  return (
    <div className="conflict-bar" role="alert">
      <span>
        {file} が外部で{deleted ? "削除" : "変更"}
        されました。このファイルには保存していない変更があります。
      </span>
      <button type="button" className="small-button" onClick={onReload}>
        {deleted ? "閉じる" : "読み込み直す"}
      </button>
      <button type="button" className="small-button" onClick={onKeep}>
        自分の変更を残す
      </button>
    </div>
  );
}
