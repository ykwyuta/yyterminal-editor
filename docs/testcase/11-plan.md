# 11 実行計画

## 試験データ

- `demo/database/*-complex.yyplan`（7DB）、`sqlserver-hash-sort.sqlplan`
- `demo/database/plan-scenarios.json` の15SQL
- 各DBの持ち込み形式（SQL Server Showplan XML、PostgreSQL EXPLAIN JSON、MySQL JSON/TREE、Oracle PLAN_TABLE CSV・V$SQL_PLAN_STATISTICS_ALL CSV、Db2 8列DEL、DuckDB JSONプロファイル、SQLite EQP CSV/JSON）
- 32MiB超・1万ノード超の計画、形式の壊れた計画

## UC-11-01 計画ファイルをオフラインで開く

- アクター: DB利用者
- 目的: DBに接続せずに保存済みの計画を確認する
- 基本フロー: `yyte --open-plan PLAN_FILE`、エクスプローラーから計画ファイルを開く、または `DB Open plan`

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-01-01 | 正常 | `yyte --open-plan demo/database/postgresql-complex.yyplan` | DB画面で計画が表示される。SQLは実行されない | P | A | `database_query_grid_plan_mouse_and_resize_use_real_terminal` |
| TS-11-01-02 | 異常 | `--open-plan` だけでファイルなし | `--open-plan requires a plan file` で非0終了 | CLI | C | — |
| TS-11-01-03 | 正常 | エクスプローラーで `.yyplan`／`.sqlplan`／`.db2plan` をクリック | 計画として表示 | P | B | — |
| TS-11-01-04 | 正常 | `DB Open plan` で各DBの持ち込み形式（8形式） | 全形式が開ける | U | A | 単体（`database_plan.rs`）、`plan_scenarios` |
| TS-11-01-05 | 異常 | 通常の表形式EXPLAINの拡張子だけ `.json` に変えたもの、DBMS_XPLANの罫線テキスト、別列順の `.db2plan` | 対応外のエラーで開かない（推測で描かない） | U | B | — |
| TS-11-01-06 | 異常 | Oracle実測CSVで `LAST_STARTS` が全行0 | 未収集としてエラー | U | B | — |
| TS-11-01-07 | 境界 | 32MiB超・1万ノード超 | 上限超過のエラー | U | C | — |

## UC-11-02 推定・実測の計画を取得する

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-02-01 | 正常 | SQLiteで `demo/database/query.sql` → `DB Estimated plan` | 計画が表示される。データは変更されない | U/P | A | `sqlite_join_plan_connects_in_real_terminal` |
| TS-11-02-02 | 正常 | 15シナリオ×SQLite/DuckDBの推定、DuckDBの実測 | 全計画が取得でき、元DBのノード・接続と表示カード・接続線が一致 | U | A | `sqlite_fifteen_plans`、`duckdb_fifteen_plans` |
| TS-11-02-03 | 正常 | 外部5DBの15シナリオ（推定、Db2以外は実測も） | 同上 | L | A | `plan_scenarios`（ignored） |
| TS-11-02-04 | 正常 | 試験用の表に対する更新SQLで `DB Actual plan` | 利用者文書どおり**SQLが実際に実行され**、更新が起きる。実行前に画面上の注意表示があるかを記録する | U | A | — |
| TS-11-02-05 | 境界 | SQLite・Db2で実測計画 | 対象外である旨を示し、成功扱いにしない | U/L | B | — |

## UC-11-03 計画図を読み、ノード・接続線の詳細を見る

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-03-01 | 正常 | 計画を表示 | 結果が左、入力が右。入力から結果へ左向きの矢印。演算子の箱・分類記号・対象・コスト・推定/実測行数・時間 | U/P | A | `plan_scenarios` |
| TS-11-03-02 | 正常 | 行数の多い接続線 | 線の太さが行数を反映 | U | C | — |
| TS-11-03-03 | 正常 | 警告のある演算子、並列演算子 | `!`、`∥` で区別 | U | B | — |
| TS-11-03-04 | 正常 | ノードをクリック | プロパティパネルに演算子の詳細（DB固有属性を含む） | U/P | A | `database_query_grid_plan_mouse_and_resize_use_real_terminal` |
| TS-11-03-05 | 正常 | 接続線をクリック | データフローの詳細を表示 | U | B | `plan_scenarios`（全接続線のクリック領域） |
| TS-11-03-06 | 正常 | Tab／`DB Next operator`、`DB Previous operator`、`DB Next warning` | 選択が進む・戻る・警告へ移る | U | B | — |
| TS-11-03-07 | 正常 | 矢印でパン、PageUp/Downでプロパティのスクロール | 画面外のノードへ移動できる | U | B | `plan_scenarios`（画面外パン） |
| TS-11-03-08 | 境界 | 狭い端末 | プロパティが下側に移る | U/P | B | 単体 |
| TS-11-03-09 | 正常 | 推定コストのみの計画 | コストを実測時間として表示しない | U | B | — |

## UC-11-04 表示用グループ（SQLite・MySQL）を確認する

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-04-01 | 正常 | SQLiteの2表結合（親なしノードが複数） | `SQLite query (display group)` に接続した3カード・2辺。詳細に `Display.Synthetic=true` | U/P | A | `sqlite_join_plan_connects_in_real_terminal` |
| TS-11-04-02 | 正常 | MySQLの相関スカラー副問い合わせ（推定JSONの `select_list_subqueries`、実測TREEの複数ルート） | SELECT句のサブクエリが表示され、表示用親に接続される | U/L | A | 単体、`plan_scenarios` |
| TS-11-04-03 | 正常 | 補正済み計画を保存して再読込 | ノードが増えない（再適用で安定） | U | A | 単体 |
| TS-11-04-04 | 正常 | 異なる `Query.Text` のルート | 別グループのまま | U | B | 単体 |
| TS-11-04-05 | 正常 | 表示用親 | 推定行数・実測行数・コストを持たない | U | B | 単体 |

## UC-11-05 計画を保存する

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-05-01 | 正常 | `DB Save plan` → 新しい `.yyplan` → 開き直す | DB種類・推定/実測・全ノード・プロパティ・原文が一致 | U/L | A | `plan_scenarios`（保存往復） |
| TS-11-05-02 | 異常 | 既存パスへ保存 | 上書きを拒否 | U | A | — |

## UC-11-06 原文・SVGを取り出す

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-06-01 | 正常 | `DB Plan raw` | 元のXML／JSON等がエディタの新タブに出る（表示用グループの補正前） | U | B | — |
| TS-11-06-02 | 正常 | `DB Plan SVG` → 新しいパス | 図のSVGが保存される。表示用グループを含む | U | B | `plan_scenarios` |

## UC-11-07 計画画面を戻る・切り替える

| ID | 分類 | 手順 | 期待結果 | レベル | 優先度 | 既存試験 |
|---|---|---|---|---|---|---|
| TS-11-07-01 | 正常 | `DB Plan`／`DB Results`／`DB Query`／`DB ER` | 各画面へ切り替わる | U | C | — |
