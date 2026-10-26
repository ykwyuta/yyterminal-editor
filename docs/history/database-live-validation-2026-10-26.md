# 2026-10-26 ODBC廃止後の外部5DB実接続検証

検証対象はyyterminal-editor `85edd25`。[ODBC依存の削除](remove-odbc-2026-10-26.md)（PR #2、yyeditorのブランチコミット `4d0434d` に固定）の上で、全YYクレートをODBCを含まないyyeditor master `5d2dbfc5859afab33791a67a2260907ee4712543` へ固定し直したコミット。ODBC形式の接続先の拒否、Db2実測計画の事前拒否、`tools/release.py` のリンク検査は維持している。検証資材も同じSHAのyyeditorチェックアウトを使用した。

## 共通の資材

`yyeditor/demo/yydbmanager/compose.yaml`、初期化SQL、期待件数、サンプルSQL、`verify.ps1` を変更せずそのまま使用した。Composeプロジェクトは起動済みの `yydbmanager-demo`、5DBとも healthy。デモボリュームの初期化・削除や独自データの追加はしていない。共有デモのため試験後も停止していない。WindowsとLinuxの試験は同時に実行せず、順に実行した。

## Windows x64

- fmt、全ターゲットClippy（警告をエラー）、`tools/test_release.py`（4件）、通常試験99件が成功した。
- 元の `verify.ps1 -Services postgres,mysql,sqlserver,oracle,db2` は5DBすべて成功し、9テーブルの件数と5サンプルSQLが一致した。
- 実行環境にPowerShell 7が無く、`tools/test_database_live.py` は `pwsh` の起動で止まった。そのため同じ `verify.ps1` を変更せずWindows PowerShell 5.1で実行し、続く `database_live` も同じ環境変数で直接実行した。
- `database_live` の明示実行5試験はすべて成功した。
- `plan_scenarios` の `--include-ignored` 実行は7DBすべて成功した。計画180件（推定105件、実測75件）を取得・照合した。

## Linux x64

`rust:1.99-bookworm` コンテナーで実行した。CIと同じく `libssl-dev pkg-config cmake` のみを導入し、unixODBCは未導入（0件）。ソースは `git archive` で取り出した `85edd25` のLF改行のもの。DBへは `host.docker.internal` 経由で既定の公開ポートへ接続した。

- fmt、全ターゲットClippy（警告をエラー）が成功した。
- 通常試験117件が成功した。このうち16件は `tests/cui.rs` のPTY実端末試験。
- `database_live` の明示実行5試験はすべて成功した。
- `plan_scenarios` は7DBすべて成功した。計画180件（推定105件、実測75件）。
- debugビルドの `yyte` の `ldd` 出力にODBCライブラリは無かった。

Db2の実測計画は、yyteがSQL送信前に拒否するため対象外。推定計画は検証した。上流 `5d2dbfc` ではDb2実測をRust直接接続で取得できるが、稼働中の `YYDBMANAGER_ACTIVITY` モニターが必要で、既定Composeには無いため今回は検証していない。

## 成果物

`target/database-live/` 下の `20261026T-win-85edd25`、`20261026T-linux-85edd25` にログとDB別の計画・SVG・ER YAMLを保存した（リポジトリには含めない）。releaseビルドと `tools/release.py verify`、ARM64での実接続は未検証で、CIで確認する。再現手順は [外部5DB実接続試験](../developer/database-live-tests.md) を参照。
