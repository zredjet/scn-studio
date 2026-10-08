// 仮素材（T1-11、D050。docs/design.md「仮素材は名前入りの色付き矩形、音は無音」）：素材カタログにファイルの
// 無い意味名を、名前から決まる色の矩形で見せる。同じ名前はいつも同じ色にする。

/** 名前から色相を決める（FNV-1a の簡単な散らし）。 */
export function hueOf(name: string): number {
  let hash = 0x811c9dc5;
  for (const c of name) {
    hash ^= c.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash % 360;
}

/** 背景の仮素材の色（暗めのグラデーションの2色）。 */
export function placeholderColors(name: string): readonly [string, string] {
  const hue = hueOf(name);
  return [`hsl(${hue} 32% 34%)`, `hsl(${(hue + 40) % 360} 38% 22%)`];
}

/** 素材の状態：ファイルあり、登録済みだがファイルなし、カタログに無い（I403）。 */
export type AssetState =
  | { readonly kind: "file"; readonly path: string }
  | { readonly kind: "placeholder" }
  | { readonly kind: "unregistered" };

/** カタログの表（意味名→ファイル）から、素材の状態を引く。 */
export function assetState(
  table: Readonly<Record<string, string | null>> | undefined,
  name: string,
): AssetState {
  if (table === undefined || !Object.hasOwn(table, name)) {
    return { kind: "unregistered" };
  }
  const path = table[name];
  return path === null || path === undefined
    ? { kind: "placeholder" }
    : { kind: "file", path };
}
