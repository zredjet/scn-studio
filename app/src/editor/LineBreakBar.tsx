// 改行の種類が混ざったファイルの知らせ。エディタは改行を1種類しか持てないので、多いほうにそろえて
// 読み込んでいる。編集しなければファイルは変わらないが、保存するとそろう（D045）。

interface Props {
  readonly lineBreak: "\r\n" | "\n";
}

export function LineBreakBar({ lineBreak }: Props) {
  return (
    <div className="notice-bar" role="status">
      このファイルは改行の種類が混ざっています。保存すると、改行をすべて
      {lineBreak === "\r\n" ? " CRLF " : " LF "}
      にそろえます。
    </div>
  );
}
