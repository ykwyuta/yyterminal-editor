# 描画性能の測定

本文編集では、貼り付けが通常のキー列として届く場合も、受信済みの文字・改行・Tabを最大64 KiB程度または8 msまでまとめて文書へ反映し、その後に描画する。Linuxではcrosstermの `use-dev-tty` によるpoll方式を使い、読み残した長文入力の通知を取りこぼさないようにする。このバックエンドはゼロタイムアウトでは内部の入力キューも確認しないため、入力確認のタイムアウトを最低1 msにする。Tabは通常入力と同じ4空白に変換する。単独キーは通常の編集経路を通し、キー解放は通常入力と同じく無視し、ショートカット・マウスなどに達したらそこで区切り、次のイベントとして保持する。メニュー・プロンプト・他のビューではこの本文入力の集約を行わない。bracketed pasteは従来どおり貼り付けイベント全体を一括反映する。

```powershell
$env:YYTE_BENCH_OUTPUT='target/render-measurement.json'
cargo test --locked --bin yyte drawing_scenarios_benchmark -- --ignored --nocapture
```

Linuxでは `YYTE_BENCH_OUTPUT=target/render-measurement.json cargo test --locked --bin yyte drawing_scenarios_benchmark -- --ignored --nocapture`。シェルを起動するためPTYが使える環境で実行する。

ASCII長行、Unicode、Markdown、画像、CSV、固定長、シート、長行diff、Unicodeの疎なdiff、密な挿入diff、端末、DBツリー、長いDBセル、ER、計画の15シナリオを2画面サイズで測る。非同期処理完了を期限付きで待ってから、移動と実際の画面描画を各40回測定する。中央値・p95・最大値をJSONに保存する。

初回の読み込みや組版・比較作成・DB応答、実端末への通信は対象外。時間をassertする不安定なCI試験ではなく、同じ環境・ビルド種別で前後比較する手動測定。通常の回帰試験は文字幅、色、編集後の無効化、スクロール位置などの正しさを確認する。

[2026-10-25の測定結果](../history/scroll-performance-2026-10-25.md)
