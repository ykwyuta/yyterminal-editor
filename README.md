# yyterminal-editor

Linuxのターミナル内で使うYYシリーズのエディタです。コマンド名は `yyte`。VS Codeに近いキー操作で、エクスプローラー、文書タブ、統合ターミナル、Git操作、Markdownプレビューを切り替えます。フォントは同梱せず、利用者の端末設定を使用します。

現在は初期開発版です。テキスト編集・保存、Undo/Redo、選択、内部クリップボード、PTYシェル、Gitの状態・差分・ステージ・コミット、Markdownのテキストプレビュー、既存 `.yyworkspace` の読み込みを実装しています。編集モデル・文字コード・端末エミュレーターには既存YYシリーズの共有クレートを使用しています。

```sh
cargo run --release --locked -- /path/to/project
cargo run --release --locked -- README.md
cargo run --release --locked -- --encoding IBM-930/fixed:80 host.dat
```

Rust 1.99.0、UTF-8端末、Git、シェルが必要です。F1/F10で全コマンド、F6でエクスプローラー、F4で統合端末を開きます。`yyte --help` は端末UIを起動せず操作説明を表示します。Windowsでのビルド・モデル試験にも対応しますが、実際のCUI統合試験はLinuxで行っています。

ファイルは専用の作業用コピーを編集し、描画では画面内の範囲のみ読みます。作業用ディスクがファイルサイズ相当必要です。保存時には外部変更を照合します。64MiBファイルと100万文字の単一行で回帰試験を実施しています。大容量での総合性能評価は今後実施します。

CSV・固定長の専用編集画面、SSH、`.yyl`・`.yys` 等の専用形式対応、言語サーバーは未実装です。未対応のYYバイナリ文書は通常テキストとしての編集を拒否します。企画案の全機能が実装済みという意味ではありません。

- [利用者向け操作説明](docs/user/help.md) — 同梱ヘルプの原稿
- [ビルド・内部構成・検証](docs/developer/README.md)
- [企画案](docs/proposal/2026-10-25-yyterminal-editor.md)
- [実装状況](docs/history/implementation-status.md)

ライセンス: GPL-3.0-or-later。[COPYING](COPYING) を参照してください。
