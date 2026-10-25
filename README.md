# yyterminal-editor

Linuxのターミナル内で使うYYシリーズのエディタです。コマンド名は `yyte`。VS Codeに近いキー操作で、エクスプローラー、文書・ターミナルの共通タブ、統合ターミナル、Git操作、Markdownプレビューを切り替えます。フォントは同梱せず、利用者の端末設定を使用します。

現在は初期開発版です。テキスト編集・保存、CSV/TSV・任意区切り文字の表編集、COBOLコピーブックを使う固定長編集、Undo/Redo、数式・ソート・フィルターを含む `.yys` ブック編集、列幅・行高調整、yymergeによる巨大ファイル差分、内部クリップボード、PTYシェル、Git操作、画像のドット絵・MermaidのASCII変換を含むMarkdownプレビュー、既存 `.yyworkspace` の読み込みを実装しています。編集モデル・文字コード・端末エミュレーターには既存YYシリーズの共有クレートを使用しています。

```sh
cargo run --release --locked -- /path/to/project
cargo run --release --locked -- README.md
cargo run --release --locked -- --encoding IBM-930/fixed:80 host.dat
```

Rust 1.99.0、UTF-8端末、Git、シェルが必要です。上部メニューとタブはマウスで操作できます。F1/F10・F6・F4も代替ショートカットとして使えます。`表示 > Markdownを並べてプレビュー`（英語では `View > Split Markdown preview`）で編集とプレビューを並べられます。`yyte --help` は端末UIを起動せず操作説明を表示します。Windowsでのビルド・モデル試験にも対応しますが、実際のCUI統合試験はLinuxで行っています。

ファイルは専用の作業用コピーを編集し、描画では画面内の範囲のみ読みます。作業用ディスクがファイルサイズ相当必要です。保存時には外部変更を照合します。64MiBファイルと100万文字の単一行で回帰試験を実施しています。大容量での総合性能評価は今後実施します。

表モードはF7、固定長はメニューの `Fixed-length table` から開きます。セル編集・TSV矩形貼り付け・行追加削除に対応し、固定長ではCOMP-3等を扱います。元のレコードの未編集バイトを保持した固定長エクスポート、`.yyl` の保存・適用・推定、マルチレイアウトにも対応します。SSH、言語サーバーは未実装です。未対応のYYバイナリ文書は通常テキストとしての編集を拒否します。企画案の全機能が実装済みという意味ではありません。

- [CSV・固定長編集の動作デモ](demo/README.md)
- [単独Markdownビューアー・画像・Mermaid](docs/user/markdown-viewer.md) — `markdownViewer FILE.md`
- [データベース管理・SQL・実行計画・検索結果・ER](docs/user/database.md) — YYdbmanagerの共有エンジンを使用
- [利用者向け操作説明](docs/user/help.md) — 同梱ヘルプの原稿
- [ビルド・内部構成・検証](docs/developer/README.md)
- [企画案](docs/proposal/2026-10-25-yyterminal-editor.md)
- [実装状況](docs/history/implementation-status.md)

ライセンス: GPL-3.0-or-later。[COPYING](COPYING) を参照してください。

メニューはyyeditor・yysheetを基に、ファイル、編集、検索、表示、挿入・削除、書式、データ、ツール、設定、ヘルプに分類しています。`Settings > Language: Japanese` で日本語へ、`設定 > English（英語）` で英語へ切り替えられます。`表示` でフォルダーのツリー表示を選べ、長いファイル名は折り返します。ファイルの右クリックからCSV・TSV・固定長などの開くモードを指定できます。設定と操作の詳細は[利用者向けヘルプ](docs/user/help.md)を参照してください。

Markdownプレビューは128MiB・100万表示行へ対応し、表を行番号・列記号・罫線・セル折り返し・数値の右寄せを持つグリッドで表示します。全画面・エディタとの並列表示・単独の `markdownViewer` で共通の描画を利用します。詳しくは[Markdownプレビュー](docs/user/markdown-viewer.md)を参照してください。

エディタとMarkdownプレビューのコードブロックは、yyeditorと共通の38種類のシンタックス定義で色分けします。エディタは自動判定し、`表示 > シンタックスの言語` から言語指定や無効化もできます。Markdown原稿のコードフェンス内も指定言語で表示します。対応言語と巨大ファイルでの解析範囲は[ヘルプ](docs/user/help.md)を参照してください。

リリースビルドはWindows／Linuxのx86_64版とARM64版を用意します。CPUに合う `x86_64` または `arm64` の配布物を選んでください。Markdownプレビューは見出しレベル別の枠・下線、ブロック間の空行、階層別リスト記号・番号・チェックボックス、折り返しの字下げで読みやすく表示します。

データベースメニューからSQLite・DuckDB・PostgreSQL・MySQL・SQL Server・Oracle・Db2を利用できます。結果は行番号・列見出し・罫線・選択・コピー・ページ取得を持つ閲覧専用グリッド、実行計画は元のノード配置・コスト・警告を用いた端末用の図として表示します。LinuxではunixODBCとOpenSSL 3が必要です。詳しくは[データベース機能のヘルプ](docs/user/database.md)を参照してください。

- [実DBから取得した実行計画画面の出力例](docs/user/database-plan-examples.md)
