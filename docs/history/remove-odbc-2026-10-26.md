# 2026-10-26 ODBC依存の削除

## 目的

利用者の判断で、ODBCのドライバーマネージャー（Windowsの `odbc32`、Linuxの unixODBC `libodbc.so.2`）がなければ動かない機能を削除し、制限として受け入れた。

## 調査結果

固定していた `yy-db-core`（yyeditor `a5c7fa2a43ec6ff531426610133688ff3875d3ff`、当時の上流masterも同じ依存）は `odbc-api` を機能フラグなしの必須依存にしていた。`Database::open` からODBCの呼び出しへ到達するため、SQLite・DuckDBだけを使う場合もyyteの実行ファイルは常に `libodbc.so.2` へリンクしていた。yyte側の機能を削るだけでは依存は外れない。

ODBCが実際に必要な機能は次の2つだけだった。

- `yynative:` 以外の接続文字列による汎用ODBC接続（接続ウィザードは作らないが、手で編集した `databases.json` やYYdbmanager由来の接続先で使われ得た）
- Db2の実測計画（`EXPLAIN_FROM_ACTIVITY` のODBC出力パラメーター）

7種類のDBへの直接接続、SQLite・DuckDB、計画ファイル、ER、転送はODBCを使わない。

## 対応

上流yyeditorのブランチ `yy-db-core-optional-odbc`（コミット `4d0434d95834adf9173f20ddaa22da56d3658063`）で、`yy-db-core` に既定で有効な機能 `odbc` を追加した。ODBCを使う箇所はこの機能でだけ有効にする。yydbmanagerは既定のまま動作を変えない。同じコミットでDb2の実測計画は、ODBC接続でなければ対象SQLを実行する前に拒否するように変更した。従来は直接接続のDb2でも対象SQLを実行してから失敗していた。

yyterminal-editorでは次を行った。

- 全YYクレートを `4d0434d95834adf9173f20ddaa22da56d3658063` に揃え、`yy-db-core` を `default-features = false` で参照した。`Cargo.lock` から `odbc-api`・`odbc-sys` が消えた。
- ODBC形式の接続先は、保存と接続のどちらでも拒否する（`database.rs`）。Db2の実測計画はSQLを送る前に拒否する（`db_app.rs`）。
- CIのLinux依存から `unixodbc-dev` を外した。`tools/release.py` の検証と梱包処理は、ELFの `DT_NEEDED` とPEのインポート表から `libodbc.*`／`odbc32.dll` へのリンクを検出すると失敗する。最初は実行ファイル全体のバイト列を検索したが、同梱した利用者文書の「`libodbc.so.2` は不要」という文に一致して誤検出したため、リンク情報を読む方式にした。
- README、`docs/user/database.md`、`docs/developer/README.md`、`docs/testcase` に制限を記載した。

## 検証

Linux x86_64のコンテナで実施した。unixODBCの開発パッケージとライブラリを削除し、`ldconfig -p` に `odbc` が出ない状態で行った。

- yyeditor `yy-db-core`: fmt、`odbc` ありとなしの両構成で全ターゲットClippy（警告をエラーとして扱う）と単体試験（ありで55件、なしで56件）が成功した。
- yyterminal-editor: fmt、全ターゲットClippy、`tools/test_release.py`（4件）、通常試験117件（実PTY 16件を含む）が成功した。追加した単体試験は、ODBC形式の接続先の保存・接続拒否と、Db2実測計画でSQLを送らないことを確認する。
- releaseビルドと `tools/release.py verify --target x86_64-unknown-linux-gnu` が成功した。検証スクリプトが読んだリンク先は `readelf -d` と一致した。PEの解析は合成したPEでだけ試験しており、実際のWindows実行ファイルはCIで確認する。
- `readelf -d` によるreleaseビルドの `yyte` のNEEDEDは `libssl.so.3`、`libcrypto.so.3`、`libstdc++.so.6`、`libgcc_s.so.1`、`libm.so.6`、`libc.so.6`、`ld-linux-x86-64.so.2` で、`libodbc` は含まれない。

Windows・ARM64、外部5DBの実接続は今回ローカルで実行していない。プッシュ後のCI結果で確認する。上流のブランチがスカッシュマージ等で別のコミットになった場合は、マージ後のmasterのSHAへ固定し直す。
