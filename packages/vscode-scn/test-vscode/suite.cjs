// 拡張の起動のテストの中身（VS Code の中で動く。test-vscode/run.mjs が呼ぶ）：言語の登録、言語サーバーの診断・
// 補完・定義へ移動、行IDの見せ方の切り替え。
"use strict";

const assert = require("node:assert/strict");
const path = require("node:path");
const vscode = require("vscode");

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(what, check, ms = 30000) {
  const end = Date.now() + ms;
  for (;;) {
    const value = await check();
    if (value) return value;
    if (Date.now() > end) throw new Error(`${what}が来ない`);
    await sleep(200);
  }
}

async function run() {
  const workspace = process.env.SCN_WORKSPACE;
  const uri = vscode.Uri.file(path.join(workspace, "story", "chapter01.scn"));
  const doc = await vscode.workspace.openTextDocument(uri);
  const editor = await vscode.window.showTextDocument(doc);
  assert.equal(doc.languageId, "scn");
  assert.ok((await vscode.languages.getLanguages()).includes("scn"));

  // 診断（言語サーバー）。
  const diagnostics = await until("診断", () => {
    const d = vscode.languages.getDiagnostics(uri);
    return d.length > 0 ? d : null;
  });
  assert.ok(
    diagnostics.some(
      (d) => String(d.code) === "I302" || String(d.code) === "W503",
    ),
    diagnostics.map((d) => d.code).join(" "),
  );

  // 定義へ移動：`-> s011` から見出しへ。
  const text = doc.getText();
  const jump = doc.positionAt(text.indexOf("-> s011") + 4);
  const locations = await vscode.commands.executeCommand(
    "vscode.executeDefinitionProvider",
    uri,
    jump,
  );
  const location = locations[0];
  const target =
    "targetUri" in location ? location.targetRange : location.range;
  assert.equal(doc.lineAt(target.start.line).text, "## 昼休みの教室 #s011");

  // 補完：行末に `@背` を打つと `@背景`。
  const end = doc.positionAt(text.length);
  await editor.edit((b) => b.insert(end, "\n@背"));
  const cursor = doc.positionAt(doc.getText().length);
  const list = await until("補完", async () => {
    const l = await vscode.commands.executeCommand(
      "vscode.executeCompletionItemProvider",
      uri,
      cursor,
    );
    return l?.items.some((i) => (i.label.label ?? i.label) === "@背景")
      ? l
      : null;
  });
  assert.ok(list.items.length > 0);

  // 行IDの見せ方の切り替え：薄く → 隠す。
  await vscode.commands.executeCommand("scn.toggleLineIds");
  assert.equal(vscode.workspace.getConfiguration("scn").get("lineIds"), "hide");
  await vscode.commands.executeCommand("scn.toggleLineIds");
  await vscode.commands.executeCommand("scn.toggleLineIds");
  assert.equal(vscode.workspace.getConfiguration("scn").get("lineIds"), "dim");

  await vscode.commands.executeCommand(
    "workbench.action.revertAndCloseActiveEditor",
  );
}

module.exports = { run };
