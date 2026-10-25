# 2026-10-25 データベース管理メニューの整理

- 親メニューのViewにDatabase Management（表示 > データベース管理）を配置した。
- Database・SQL・Results・Plan・ERの5分類を管理画面の枠内へ移動した。操作一覧のポップアップも枠内に収める。既存のDB操作は各分類とF1の全コマンド一覧から利用できる。
- DB画面のF10は画面内メニューを開き、左右キーはDB分類内を循環する。マウスと右クリックも対応する。日本語と狭い画面での配置を検証した。
- FileIndexはこのエディタで利用していない。`cargo tree -i libduckdb-sys --locked`で、DuckDBは固定SHAの`yy-db-core`から直接依存していることを確認した。共有クレートにはDuckDBを除外するfeatureがなく、FileIndex無効化でビルド時間を短縮できる構成ではないため、依存関係は変更していない。

検証結果：

- Windows x64：fmt、全ターゲットClippy（`-D warnings`）、`cargo test --workspace --locked --lib --bins`の90試験が成功。全試験の通常ビルドは使用中の`target/debug/yyte.exe`を上書きできなかったため、単体試験用バイナリで実行した。
- Linux x64：既存のDocker検証環境・エディタ用キャッシュでfmt、全ターゲットClippy（`-D warnings`）、`cargo test --workspace --locked`が成功。111試験のうち実PTYのCUI試験は15件。外部5DBの明示実行試験と手動計測・文書生成試験は対象外。
- 追加した実PTY試験ではViewからの到達、親バーからのDB分類除去、画面内のF10・左右キー・マウス選択を確認した。画面待機には期限を設け、失敗時に画面を出力し、起動したプロセスは回収する。

公開バージョンはv0.7.1（Cargoパッケージ0.7.1）。4環境のCIで検証した当該コミットの資材を配布する。
