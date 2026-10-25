# Markdownの画像・Mermaidプレビュー

`yyte` のF5と、単独コマンド `markdownViewer` は同じRust製描画エンジンを使います。ブラウザーやNode.js、Mermaid CLI、Sixelは不要です。フォントは端末の設定を使います。

```sh
cargo build --release --locked
cargo run --locked --bin markdownViewer -- demo/markdown-preview.md
markdownViewer --plain --width 100 README.md > preview.txt
markdownViewer --print --color demo/markdown-preview.md
cat README.md | markdownViewer --plain -
```

対話モードは上下矢印、PageUp/PageDown、Space、Home/Endで縦移動、左右矢印で横移動、q/Esc/Ctrl+Cで終了します。標準出力をリダイレクトした場合はASCIIの画像表示を使います。`--color` でANSIのTruecolor付き出力にできます。`--plain` は対話モードを使わず、制御コードを含まないテキストを出力します。

## 画像

`![説明](画像.png)` などのMarkdown画像をピクセルへ縮小して表示します。色付き表示は上半分・下半分で色を分けられる `▀` を使用します。端末セルの縦横比を約2:1と仮定し、2ピクセルを1セルへ収めます。通常のカラー端末でも表示できますが、色は端末の能力に依存します。透明部分は暗い背景へ合成します。ASCIIモードでは輝度を濃淡文字へ変換します。

対応形式はPNG/JPEG/WebP/GIF/BMP/ICOです。GIFは最初の画像を表示します。SVG、HTMLのimgタグ、動画は未対応です。失敗した画像には説明を表示し、後の文章や他の画像の描画を継続します。

相対パスの基準はMarkdownファイルのフォルダーです。空白などのパーセントエンコードに対応します。`--base-dir DIR` で基準を変更できます。エディタで未保存の新規ファイルの場合は現在のエクスプローラーフォルダーが基準です。F5で再プレビューすると画像の更新も反映されます。

HTTP/HTTPS画像は `markdownViewer --remote-images FILE.md` で有効化します。yyteではF1の `Preview remote images` を切り替え、F5で再表示します。URLへ接続するため、必要な文書で有効にしてください。base64形式のdata画像も読めます。

上限は画像ファイル20MiB、8192×8192ピクセル、デコーダー割当64MiB、1文書16画像です。HTTPは5秒のタイムアウト、最大3回のリダイレクト、TLS証明書検証を適用します。表示幅は画面幅か160セルの小さい方、高さは通常32行（`--image-height 1..120`）です。

## Mermaidの対応範囲

Mermaidのコードフェンスを解析し、図形・矢印・参加者・関係をASCIIへ変換します。JavaScriptや外部コマンドを実行しません。

|種類|変換する内容|
|---|---|
|flowchart / graph|ノードラベル、連鎖・分岐・循環、有向線、エッジのラベル、TB/TD/BT/LR/RL|
|sequenceDiagram|participant/actorと別名、メッセージ、戻りメッセージ、自己呼出し、その他の行の注記|
|erDiagram|テーブル、関係・カーディナリティ、ブロック内の属性・キー|
|classDiagram|クラス、関係、メンバー定義|
|stateDiagram / stateDiagram-v2|基本遷移、遷移ラベル、開始・終了記号|

非循環フローチャートは階層配置します。循環や画面に収まらない図は、ノード一覧とノードIDを結ぶ矢印カードに変換します。参照先IDは同じオブジェクトを表します。ノード形状は箱に統一します。色・線の装飾は省略した旨を示し、subgraphは名前を注記します。Web表示と同じ位置・形状・装飾にはなりません。

Mermaidの全構文を実装したものではありません。新しいshape属性、複雑なネストや複合構文、その他の図種類は、原文と未対応理由を表示します。意味を確実に解釈できない図を不正確な画像へ置き換えません。1図64ノード・128エッジが上限です。シーケンス図の参加者が画面幅に収まらない場合も原文へ戻します。

構文の参照元：[Mermaid公式フローチャート](https://mermaid.js.org/syntax/flowchart.html)、[シーケンス図](https://mermaid.js.org/syntax/sequenceDiagram.html)、[ER図](https://mermaid.js.org/syntax/entityRelationshipDiagram.html)。実例は [demo/markdown-preview.md](../../demo/markdown-preview.md) にあります。

Markdown全体はUTF-8で1MiBまで、描画結果は20,000行までです。yyteでは上限以降を省略した旨を表示し、単独コマンドは上限超過の入力をエラーにします。HTMLは説明付きの原文として表示し、リンクの自動実行はしません。
