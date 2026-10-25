# 外部5DBの実接続検証

DBワーカーに対して、yydbmanagerと同じデモ・SQL・直接接続シナリオを実行します。Composeやデータセットは複製せず、依存クレートと同じコミットのyyeditorチェックアウトにある `demo/yydbmanager/compose.yaml` をそのまま参照します。

## 実行

x86_64のDocker Linuxコンテナー、Python 3.11以降、Rustと通常のDBビルド依存を用意します。WindowsではPowerShell 7も使用します。yyeditorの必要なSHAは本リポジトリの `Cargo.toml` の `yy-db-core.rev` にあります。

```powershell
python tools/test_database_live.py --yyeditor ../yyeditor --up
```

既存の `yydbmanager-demo` プロジェクトを利用し、PostgreSQL・MySQL・SQL Server・Oracle・Db2の全プロファイルを起動します。元のComposeのヘルスチェックがデータ投入完了を判定します。OracleとDb2の初回起動には時間がかかります。既存デモボリュームを削除・初期化する処理はありません。他のComposeプロジェクトには触れません。起動済みなら `--up` を省略できます。

公開ポート・ユーザー・パスワードは元のデモの既定値を使います（25432、23306、21433、21521、25000）。試験先を間違えないよう、カスタムポートはこの試験では使いません。LinuxコンテナーからDocker Desktopの公開ポートへ接続する場合は `--host host.docker.internal` を指定します。DBクライアントライブラリ・ODBCドライバーを使わずRust直接接続を検証します。

Windowsでは元の `verify.ps1 -Services postgres,mysql,sqlserver,oracle,db2` も実行します。このスクリプトはコンテナー内のベンダークライアントを使い、アプリの試験と独立にデータの正しさを確認します。

## シナリオ

- 元の `data/expected-counts.json` と `sql/00-verify-counts.sql`：9テーブルの件数が完全一致すること。
- 元の `sql/01-overview.sql` ～ `04-exact-values-and-logical-relation.sql`、`sql/performance/<DB>.sql`：結合・集計・ソート、選択率、日本語・NULL・正確な数値、性能分析SQL。
- 元の `native_live.rs` と同じ値：BIGINT上限、DECIMAL(20,4)、日本語・引用符・バックスラッシュ、バイナリ、全NULL行。ターミナル側ではParquet経由でインポートし、SELECT・DELETE・ROLLBACKと再SELECTを確認。
- オブジェクト・列・ERの主外部キー取得、ER YAMLの保存・再読込。
- デモの5表結合に対する推定計画・実測計画、端末の演算子配置、SVG、yyplan保存・再読込。
- 1200行のページングとページ境界をまたぐTSVコピー。

Db2実測計画は、yydbmanagerの `native_live.rs` と同じく対象外です。既定Composeではactivity event monitorを構成しません。推定計画は検証し、未対応CLOB取得後にカーソルが閉じられ、その接続でDDLが継続できることも確認します。対象外を実測計画の成功とは扱いません。

## 成果物と終了

`target/database-live/<実行時刻>/` に終了コード・イメージIDを含む `verification.json`、実行ログ、DB別の計画・SVG・ER YAMLを保存します。失敗時もレポートを残します。通常の `cargo test` はDBの起動も接続も行いません。この5試験は明示した `--ignored` でのみ実行します。

試験用に作った `yyte_probe_<プロセスID>` テーブルだけを後始末し、デモ表は変更しません。共有されるデモなので終了時の自動停止は行いません。不要な場合は元のComposeで `stop` してください（`down -v` はデータ削除になるため使用しません）。

```powershell
docker compose -p yydbmanager-demo -f ../yyeditor/demo/yydbmanager/compose.yaml --profile "*" stop
```

Linuxで試験だけを直接実行する場合も同じ資材を指定します。通常の `cargo test` とは別に必ず `--ignored` を指定します。

```sh
YYDBMANAGER_DEMO=/path/to/yyeditor/demo/yydbmanager \
  cargo test --locked --test database_live -- --ignored --nocapture --test-threads=1
```

ComposeとSQLの修正、独自の初期データや実測収集モニターの追加は、この共通シナリオでは行いません。
