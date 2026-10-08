// 拡張の起動のテスト（T3-16）：VS Code を開いて、houkago の写しで拡張を動かし、test-vscode/suite.cjs を
// 走らせる。`scripts/check-all` には入らない（VS Code が要り、窓が短い間開く）。
//
// - VS Code：環境変数 VSCODE_EXECUTABLE、無ければ macOS の /Applications/Visual Studio Code.app、それも無ければ
//   @vscode/test-electron が安定版をダウンロードする。
// - scn：環境変数 SCN_BIN、無ければリポジトリの target/debug/scn（先に `cargo build -p scn-cli`）。

import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { runTests } from "@vscode/test-electron";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const repo = here("../../../");
const exe = process.platform === "win32" ? "scn.exe" : "scn";
const scn = process.env.SCN_BIN ?? join(repo, "target", "debug", exe);
if (!existsSync(scn)) {
  console.error(
    `${scn} がありません。先に cargo build -p scn-cli を実行してください`,
  );
  process.exit(2);
}
const mac = "/Applications/Visual Studio Code.app/Contents/MacOS/Code";
const vscodeExecutablePath =
  process.env.VSCODE_EXECUTABLE ??
  (process.platform === "darwin" && existsSync(mac) ? mac : undefined);

// houkago の写しに、拡張が使う scn の場所を書く。
const workspace = mkdtempSync(join(tmpdir(), "scn-vscode-"));
cpSync(join(repo, "samples", "houkago"), workspace, { recursive: true });
mkdirSync(join(workspace, ".vscode"), { recursive: true });
writeFileSync(
  join(workspace, ".vscode", "settings.json"),
  JSON.stringify({ "scn.path": scn, "scn.lineIds": "dim" }, null, 2),
);

try {
  await runTests({
    vscodeExecutablePath,
    extensionDevelopmentPath: here("../"),
    extensionTestsPath: here("./suite.cjs"),
    launchArgs: [
      workspace,
      "--disable-extensions",
      "--disable-workspace-trust",
    ],
    extensionTestsEnv: { SCN_WORKSPACE: workspace },
  });
  console.log("拡張の起動のテストに成功しました");
} catch (e) {
  console.error(`拡張の起動のテストに失敗しました：${e}`);
  process.exit(1);
}
