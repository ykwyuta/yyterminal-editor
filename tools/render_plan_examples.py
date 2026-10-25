#!/usr/bin/env python3
"""Capture the production plan pane using plans obtained by database_live."""
import argparse
import html
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
NAMES = {"postgres": "PostgreSQL", "sqlserver": "SQL Server", "mysql": "MySQL", "oracle": "Oracle", "db2": "Db2"}

def svg(rows):
    cw, rh, pad = 9, 18, 12
    content = ['<svg xmlns="http://www.w3.org/2000/svg" width="1644" height="672" viewBox="0 0 1644 672" font-family="Cascadia Mono,DejaVu Sans Mono,monospace" font-size="15"><title>yyterminal-editor execution plan pane</title><rect width="100%" height="100%" fill="#0d1117"/>']
    for y, row in enumerate(rows):
        groups = {}
        for i, cell in enumerate(row):
            end = row[i + 1]["x"] if i + 1 < len(row) else 180
            x, top = pad + cell["x"] * cw, pad + y * rh
            if cell["bg"] != "#0d1117":
                content.append(f'<rect x="{x}" y="{top}" width="{(end-cell["x"])*cw}" height="{rh}" fill="{cell["bg"]}"/>')
            if cell["text"].strip():
                groups.setdefault((cell["fg"], cell["bold"]), []).append(f'<tspan x="{x}">{html.escape(cell["text"])}</tspan>')
        # Explicit cell positions keep wide Japanese text and box borders aligned,
        # including SVG engines that do not implement textLength/lengthAdjust.
        for (fg, bold), spans in groups.items():
            weight = "bold" if bold else "normal"
            content.append(f'<text y="{pad+y*rh+14}" fill="{fg}" font-weight="{weight}">{"".join(spans)}</text>')
    content.append("</svg>")
    return "\n".join(content)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="Successful tools/test_database_live.py output directory")
    args = parser.parse_args()
    report = args.report.resolve()
    if json.loads((report / "verification.json").read_text(encoding="utf-8"))["status"] != "passed":
        parser.error("Use a successful live validation report")
    env = os.environ.copy()
    env["YYTE_PLAN_EXAMPLES_INPUT"] = str(report)
    subprocess.run(["cargo", "test", "--locked", "--bin", "yyte", "capture_live_plan_documentation", "--", "--ignored", "--nocapture"], cwd=ROOT, env=env, check=True)
    assets = ROOT / "docs/user/plan-examples"
    assets.mkdir(parents=True, exist_ok=True)
    doc = ["# 実行計画画面の出力例", "", "yydbmanagerと同じDocker Composeの受注データに対する5表結合・集計・ソートの実行計画です。アプリ本体の実行計画ペインの描画関数を180×36セルの端末バッファへ出力し、色・文字・位置をSVGへ変換しています。", "", "SQLは共通デモの `sql/02-join-and-plan.sql`。PostgreSQL・MySQL・SQL Server・Oracleは実測計画、Db2は推定計画です。実測行数や経過時間はこの取得時の値で、実行条件によって変わります。", "", "## 読み方", "", "入力は右側、結果は左側です。枠は演算子、矢印は行の流れ、Cost %は文内の推定コスト比率、推定コストはDBごとの単位で表示します。黄色の枠は選択中の演算子で、右側に型・コスト・実測行数・DB固有の統計を表示します。画面外の演算子は矢印キーやホイールで移動し、Tabで選択を切り替えます。", "", "全演算子を同時に縮小表示せず、文字の大きさを維持してスクロールする画面です。狭い端末ではプロパティを下側に配置します。", ""]
    for service, name in NAMES.items():
        capture = ROOT / "target/plan-examples" / f"{service}.json"
        (assets / f"{service}.svg").write_text(svg(json.loads(capture.read_text(encoding="utf-8"))), encoding="utf-8")
        shutil.copyfile(capture.with_suffix(".txt"), assets / f"{service}.txt")
        mode = "推定計画" if service == "db2" else "実測計画"
        doc += [f"## {name}：{mode}", "", f"![{name}の実行計画ペイン](plan-examples/{service}.svg)", "", f"[同じ出力をテキストで読む](plan-examples/{service}.txt)", ""]
    doc += ["## 再現する", "", "共通デモを起動して実接続試験を実行し、成功したレポートフォルダーを指定します。DBやSQLを複製せずに同じ例を再取得できます。", "", "```sh", "python tools/test_database_live.py --yyeditor ../yyeditor --up", "python tools/render_plan_examples.py target/database-live/<成功した実行時刻>", "```", "", "Db2実測計画には別途activityモニター等の準備が必要です。この共通デモの既存シナリオには含まれず、上のDb2画面を実測計画として扱わないでください。操作と取得形式は [データベース機能](database.md) を参照してください。", ""]
    (ROOT / "docs/user/database-plan-examples.md").write_text("\n".join(doc), encoding="utf-8")

if __name__ == "__main__":
    main()
