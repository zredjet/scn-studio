"""リリースの版を読む（T4-32）。Cargo.toml・tauri.conf.json・package.json の版がそろっているかと、タグで
動いたときはタグ（v<版>）と合うかを確かめ、`version=<版>` を出す（GitHub Actions の出力に書く）。

使い方：python3 .github/scripts/version.py <ref_type> <ref_name>
"""

import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    ref_type, ref_name = sys.argv[1], sys.argv[2]
    found = {
        "Cargo.toml": tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))[
            "workspace"
        ]["package"]["version"],
        "app/src-tauri/tauri.conf.json": json.loads(
            (ROOT / "app/src-tauri/tauri.conf.json").read_text(encoding="utf-8")
        )["version"],
    }
    for package in ["app", "packages/lezer-scn", "packages/vscode-scn"]:
        path = f"{package}/package.json"
        found[path] = json.loads((ROOT / path).read_text(encoding="utf-8"))["version"]
    versions = set(found.values())
    if len(versions) != 1:
        for path, version in found.items():
            print(f"{path}: {version}", file=sys.stderr)
        print("版がそろっていません", file=sys.stderr)
        return 1
    version = versions.pop()
    if ref_type == "tag" and ref_name != f"v{version}":
        print(f"タグ {ref_name} が版 {version} と合いません（v{version} にする）", file=sys.stderr)
        return 1
    print(f"version={version}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
