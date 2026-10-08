// VS Code 拡張（T3-16）：.scn の言語を登録し（ハイライトは syntaxes/scn.tmLanguage.json）、言語サーバー
// （`scn lsp`。T3-15）につなぎ、行ID（行末の ^xxxx）を目立たなくする。処理は言語サーバー（コア）が受け持ち、
// 拡張は起動と見た目だけ。

import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
} from "vscode-languageclient/node";
import {
  type LineIdMode,
  type LineIdRange,
  lineIdRanges,
  modeOf,
  nextMode,
} from "./lineids.ts";

let client: LanguageClient | undefined;
/** 起動と停止を順に行う（起動の途中に再起動や設定の変更が来ても、前の処理を待ってから行う）。 */
let queue: Promise<void> = Promise.resolve();

function enqueue(step: () => Promise<void>): Promise<void> {
  queue = queue.then(step).catch(() => {});
  return queue;
}

/** 言語サーバーを起動する。`scn` が見つからなければ、設定の場所を知らせる。 */
async function startServer(output: vscode.LogOutputChannel): Promise<void> {
  const command =
    vscode.workspace.getConfiguration("scn").get<string>("path") || "scn";
  const server: ServerOptions = {
    command,
    args: ["lsp"],
    options: { shell: false },
  };
  const options: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "scn" }],
    outputChannel: output,
  };
  const started = new LanguageClient(
    "scn",
    "scn 言語サーバー",
    server,
    options,
  );
  client = started;
  try {
    await started.start();
  } catch (e) {
    client = undefined;
    await started.dispose().catch(() => {});
    void vscode.window.showErrorMessage(
      `scn の言語サーバーを起動できません（${command} lsp）：${String(e)}。設定「scn.path」に scn の場所を書いてください。`,
    );
  }
}

async function stopServer(): Promise<void> {
  const running = client;
  client = undefined;
  if (running === undefined) return;
  try {
    await running.stop();
  } catch {
    // 動いていない（起動の途中、落ちた後の再起動の途中）なら止められないので、捨てる。
    await running.dispose().catch(() => {});
  }
}

/** 止めてから起こす（順に）。 */
function restartServer(output: vscode.LogOutputChannel): Promise<void> {
  return enqueue(async () => {
    await stopServer();
    await startServer(output);
  });
}

/** 行IDの装飾（薄く、隠す）。 */
function decorations() {
  return {
    dim: vscode.window.createTextEditorDecorationType({ opacity: "0.35" }),
    // 文字を消す（CSS の display: none。VS Code の装飾でよく使う書き方）。
    hide: vscode.window.createTextEditorDecorationType({
      textDecoration: "none; display: none;",
    }),
  };
}

function rangeOf(r: LineIdRange, mode: LineIdMode): vscode.Range {
  return new vscode.Range(
    r.line,
    mode === "hide" ? r.gap : r.start,
    r.line,
    r.end,
  );
}

export async function activate(
  context: vscode.ExtensionContext,
): Promise<void> {
  const output = vscode.window.createOutputChannel("scn", { log: true });
  context.subscriptions.push(output);
  const kinds = decorations();
  context.subscriptions.push(kinds.dim, kinds.hide);

  const mode = (): LineIdMode =>
    modeOf(vscode.workspace.getConfiguration("scn").get("lineIds"));

  const decorate = (editor: vscode.TextEditor | undefined) => {
    if (editor === undefined || editor.document.languageId !== "scn") return;
    const current = mode();
    const ranges =
      current === "show" ? [] : lineIdRanges(editor.document.getText());
    // 隠すときも、カーソルや選択のある行は見せる（行IDを消さずに直せるように）。
    const active = new Set(
      editor.selections.flatMap((s) => {
        const lines: number[] = [];
        for (let l = s.start.line; l <= s.end.line; l += 1) lines.push(l);
        return lines;
      }),
    );
    const visible = ranges.filter(
      (r) => current !== "hide" || !active.has(r.line),
    );
    editor.setDecorations(
      kinds.dim,
      current === "dim" ? ranges.map((r) => rangeOf(r, "dim")) : [],
    );
    editor.setDecorations(
      kinds.hide,
      current === "hide" ? visible.map((r) => rangeOf(r, "hide")) : [],
    );
    if (current === "hide") {
      editor.setDecorations(
        kinds.dim,
        ranges.filter((r) => active.has(r.line)).map((r) => rangeOf(r, "dim")),
      );
    }
  };
  const decorateAll = () => {
    for (const editor of vscode.window.visibleTextEditors) decorate(editor);
  };

  context.subscriptions.push(
    vscode.window.onDidChangeVisibleTextEditors(decorateAll),
    vscode.window.onDidChangeTextEditorSelection((e) => decorate(e.textEditor)),
    vscode.workspace.onDidChangeTextDocument((e) => {
      for (const editor of vscode.window.visibleTextEditors) {
        if (editor.document === e.document) decorate(editor);
      }
    }),
    vscode.workspace.onDidChangeConfiguration(async (e) => {
      if (e.affectsConfiguration("scn.lineIds")) decorateAll();
      if (e.affectsConfiguration("scn.path")) await restartServer(output);
    }),
    vscode.commands.registerCommand("scn.toggleLineIds", async () => {
      const next = nextMode(mode());
      // 今の値が書かれている場所（作業フォルダ、ワークスペース、ユーザー）を書き換える（作業フォルダの設定が
      // ユーザーの設定より強いので、ユーザーの設定に書いても変わらないことがある）。
      const config = vscode.workspace.getConfiguration("scn");
      const where = config.inspect("lineIds");
      const target =
        where?.workspaceFolderValue !== undefined
          ? vscode.ConfigurationTarget.WorkspaceFolder
          : where?.workspaceValue !== undefined
            ? vscode.ConfigurationTarget.Workspace
            : vscode.ConfigurationTarget.Global;
      await config.update("lineIds", next, target);
      void vscode.window.setStatusBarMessage(
        `行IDの見せ方：${{ dim: "薄く", hide: "隠す", show: "そのまま" }[next]}`,
        3000,
      );
    }),
    vscode.commands.registerCommand("scn.restartServer", () =>
      restartServer(output),
    ),
  );
  decorateAll();
  await enqueue(() => startServer(output));
}

export async function deactivate(): Promise<void> {
  await enqueue(stopServer);
}
