# ターミナルのデータベース管理

`View / 表示` メニューの `Database Management / データベース管理` でSQLタブを開きます。
接続は「データベース」、実行操作は「SQL」、グリッドと転送は「検索結果」、計画は「実行計画」、設計は「ER図」のメニューに分類しています。これらのメニューはデータベース管理画面の枠内に表示します。画面内ではマウスまたはF10で開き、左右キーで分類を切り替え、上下キーとEnterで実行、Escで閉じます。F1の全コマンド一覧も利用できます。通常のテキスト・表計算・ターミナルのタブも利用できます。

## 接続

- 初期設定にはSQLiteとDuckDBのメモリDBがあります。選択して「接続」します。両エンジンは実行ファイルに組み込まれます。
- 「接続先を追加」で名前、DB種類、ファイルまたはホスト・ポート・DB名・ユーザー・TLS・パスワード環境変数・読み取り専用指定を順に入力します。Ctrl+Aで初期値を消せます。
- PostgreSQL、MySQL、SQL Server、Oracle、Db2はYYdbmanagerと同じRustの直接接続を使用します。Oracle Instant ClientやJVMは必要ありません。DB側の権限や対応プロトコルの条件はYYdbmanagerと同じです。
- パスワード環境変数を空にした直接接続では、接続時に伏字のパスワード入力を表示します。パスワードを設定ファイルへ保存しません。TLSの既定値は `require` です。
- 接続設定は `YYTE_CONFIG_DIR/databases.json`、未指定なら通常の利用者設定フォルダーに保存します。YYdbmanagerのWindows暗号化設定を直接読み込むものではありません。
- 保存済み接続の編集・削除、接続先の選択、切断が可能です。接続を切り替えても、切断するまでは各接続のセッションを維持します。
- ODBCは使用しません。WindowsのODBCドライバーマネージャー、LinuxのunixODBC（`libodbc.so.2`）、DB別のODBCドライバーはいずれも不要です。Linuxの配布バイナリはOpenSSLを静的リンクしており、システムのOpenSSLは不要です。TLSの証明書検証にはOSのCA証明書（`ca-certificates` 等）を使います。CA証明書がない場合は、検証に失敗して接続しません。
- **制限:** ODBC接続文字列（`Driver=...;Server=...` など）による接続には対応しません。外部DBは接続ウィザードが作る直接接続（`yynative:`）だけを使用します。YYdbmanager等で作成したODBC形式の接続先は、保存時に拒否し、接続時にもエラーとします。「接続先を編集」で直接接続として登録し直してください。
- 詳細なCAファイル指定などはプロファイルの `yynative:` JSONの `ca_file`、`trust_server_certificate` に対応します。通常は証明書検証を有効にします。

オブジェクトエクスプローラーは「接続 → スキーマ → オブジェクト種別 → テーブル等」を表示します。スキーマの [+] をクリックして展開し、テーブル左の [+] で列名と型を展開します。テーブル名をクリックするとSELECTのテンプレートを新しいSQLタブへ作り、実行は人が指示します。ツリー上の右クリックからデータベースメニューを開き、オブジェクト一覧を更新できます。

## SQLと検索結果

SQLの通常編集・保存・シンタックスハイライトには既存エディタを使います。選択したSQLがあれば選択部分、なければ文書全体（最大32MiB）を実行します。F5または「SQLを実行」を使います。複数の結果は結果セットのタブで切り替えます。トランザクションの開始はSQLで行い、コミット／ロールバックはメニューからも実行できます。停止はDBとドライバーが提供するキャンセル機構を利用します。

結果は閲覧専用のスプレッドシート風グリッドです。

- 列記号・列名・行番号、罫線、交互の行背景、数値の右寄せ、NULLの色を表示します。
- 列幅は内容から自動調整し、「結果の列幅」で4～80セル、「結果の行高」で1～8行に変更できます。
- 矢印、PageUp/Down、Home/End、ホイールで移動します。左右ホイールで表示列を切り替えます。
- マウスまたは矢印でセルを選択し、Shift併用で矩形選択します。Ctrl+Cまたは「結果セルをコピー」で内部クリップボードへコピーします。複数セルはTSV形式です。1MiBを超えるコピーには書き出しを使います。
- 検索結果は浮動小数点のセルへ一律変換せず、64bit整数やDECIMALの文字列表現を保持します。結果全体をメモリへ読み込まず、YYdbmanagerのディスク退避結果から128行ずつ取得します。
- 表示する結果は最後に成功したSQL実行の結果です。SQLタブごとの独立した結果履歴ではありません。

「性能分析SQLのテンプレート」は、選択したDB種類に対応するテンプレートの一覧を表示します。番号を入力するとSQLタブに挿入します。自動実行はしません。

## 実行計画

`yyte --open-plan PLAN_FILE` でDB接続なしに計画を開けます。通常のファイル一覧から `.yyplan`／`.sqlplan`／`.db2plan` を開いた場合も計画として表示します。

「推定実行計画」「実測実行計画」または「実行計画を開く」を使います。**実測計画は対象SQLを実際に実行します。** DB側の機能・権限によって取得できる情報は異なります。

**Db2の実測計画:** YYdbmanagerと同じRust直接接続で取得します。DB管理者による次の準備が必要です。準備がない場合は、対象SQLを実行する前にエラーになります。

- EXPLAIN表、USER TEMPORARY表スペースとそのUSE権限、稼働中の `YYDBMANAGER_ACTIVITY` イベントモニター。
- `実行計画 > Db2実測収集の準備SQL` で、必要な文を新しいSQLタブに表示します。表示するだけで実行しません。DB管理者が必要な文だけを選び、他の未確定変更を含めない専用の接続で実行してください。モニターを新規作成した場合はCOMMIT後に起動します。
- 接続ユーザーには、対象SQLの権限に加えて、EXPLAIN表の利用、`SYSCAT.EVENTTABLES` とモニター出力表の参照、`SYSPROC.WLM_GET_CONN_ENV`・`WLM_SET_CONN_ENV`・`EXPLAIN_FROM_ACTIVITY` の実行、`YYDBMANAGER_ACTIVITY` のFLUSHの権限が必要です。
- 接続単位の収集設定を一時的に変更してSQLを1回実行し、元の設定へ戻します。DB全体の設定は変更しません。既存ワークロードのactivityもこのモニターへ記録され得るため、モニターの運用と記録の削除はDB管理者が行います。
- 図には行編成表のsection actualsによる実測行数を表示します。演算子ごとの実測時間は取得しません。列編成表など統計を提供しない計画では取得できない場合があります。

SQLiteのノード別実測は共有エンジンの対象外です。

- YYdbmanagerの解析器・レイアウト・ラベル・コスト計算・警告判定を共有します。結果を左、入力を右に置き、入力から結果への左向き矢印で接続します。
- 演算子を箱と分類記号で表示し、対象、コスト、実測／推定行数、提供される時間を表示します。推定コストを実測時間として扱いません。
- 接続線の太さは行数を反映します。警告は `!`、並列処理は `∥` で区別します。色付きの選択枠を表示します。
- ノードのクリックで演算子の詳細、接続線のクリックでデータフローの詳細をプロパティパネルに表示します。元のDB固有の属性も保持します。
- 矢印で図を移動し、Tabまたは「次の演算子」で選択を進めます。「次の警告」で警告のある演算子を探せます。PageUp/Downでプロパティをスクロールします。
- 横幅が狭いとプロパティを下側へ移します。GUIのピクセル画像を端末へ貼るのではなく、文字セルで図形と配置を再現します。アイコンの絵柄やピクセル単位の外観はGUIと異なります。
- 「実行計画の原文を開く」で元のXML／JSON等をエディタへ取り出せます。「実行計画をSVG出力」で図を保存できます。
- `.yyplan` 保存はDB種類、推定／実測、全ノード、プロパティ、原文を保持します。ファイル読み込みだけではSQLを実行しません。

対応する持ち込み形式：SQL Server Showplan XML／`.sqlplan`、PostgreSQL EXPLAIN JSON、MySQL EXPLAIN JSON／TREE、Oracleの所定の列を持つPLAN_TABLE等のCSV、Db2のEXPLAIN表8列DEL／CSV、DuckDB JSONプロファイル、SQLite EXPLAIN QUERY PLANのCSV／行オブジェクトJSON、全DB共通の`.yyplan`。計画は32MiB・1万ノードまでです。`demo/database` に7種類のサンプルがあります。

## 取り込み・書き出し

データベースメニューからCSV、TSV、任意区切り、Parquet、固定長の取り込み／書き出しを選びます。設定は操作時だけ入力します。出力先は新しいパスを指定します。取り込み時は `テーブル名;create` または `テーブル名;append` を指定します。

CSV／TSV／任意区切りは引用符・フィールド内改行を扱うUTF-8形式で、見出し行あり、NULLは `\N` です。固定長は `.yyl` の単一レコード定義・文字コード・改行指定を使用し、COMP-3等を共有COBOLコーデックで処理します。DB結果からの固定長出力には複数レイアウトを指定できません。

## ER図

「DBからER図を取得」「新規ER図」「ER YAMLを開く」で表示します。PK・FK・主要項目を表示し、主要項目だけに絞る切り替えもできます。物理リレーションは実線、論理リレーションは点線の矢印で示します。

- 「ERにテーブルを追加」で主キーidを持つテーブルを作れます。
- 「ER YAMLを編集」でエディタへ全定義を取り出し、列・型・キー・主要項目フラグ・論理名・サイズ・位置・リレーションを編集できます。別名保存後に「ER YAMLを開く」で反映します。
- 「論理リレーションを推薦」で候補を一覧にし、「推薦リレーションを適用」で番号を選んで追加します。推薦だけでは定義を書き換えません。
- DDLはSQLタブへ生成し、人が確認して実行します。論理リレーションはDBの外部キー制約へ変換しません。
- ER図のSVG出力、YAML保存、全テーブルの設計とMermaid ER・リンクを含むMarkdown出力を共有エンジンで行います。YAMLのモデルと表示状態を保持します。

「ERの列定義を表編集」でテーブル名を選ぶと、yysheetの編集エンジンを使う定義表を開きます。物理名・論理名・型・NULL可否・主要項目・既定値をまとめて編集し、矩形コピー／貼り付け、行の追加・削除を利用できます。「ERの列型を選択」はDB種類に対応した選択一覧を表示し、現在行の型へ反映します。型を直接入力・貼り付けしても構いません。「ERの列定義を反映」で検証して一括反映します。主キー・関係で使う列の名前変更は元のindex列で追跡します。新しい列は元indexを空にします。定義表の見出し行と列順序は維持してください。

ERは端末のテーブル箱と関係一覧で表示します。GUIのドラッグ配置は移植しておらず、位置やサイズ・細かな制約はYAMLから編集できます。

## デモ

SQLite / memoryに接続し、`demo/database/sqlite.sql` を開いて実行します。次に `demo/database/query.sql` を開き、推定計画を取得できます。オフラインでの描画確認には `demo/database/*.yyplan` と `.sqlplan` を開きます。

## 接続先から計画ファイルを取得する

以下はYYdbmanagerと共通の解析形式です。取得操作は元DBへ接続できる環境で行います。ANALYZEや実測・プロファイルはSQLを実行するため、取得したい計画の種類を確認してください。

### SQL Server：SSMSで保存

SSMSで推定計画（Ctrl+L）を表示、または「実際の実行プランを含める」（Ctrl+M）を有効にして対象SQLを実行します。計画の背景を右クリックし「実行プランに名前を付けて保存」で`.sqlplan`に保存します。画像や結果一覧のCSVではなく、Showplan XMLを持ち込みます。取得には対象SQLの参照権限等と、参照するDBのSHOWPLAN権限が必要です。実測統計のないXMLを実測値付きには変換しません。

公式：[XML形式での実行計画の保存](https://learn.microsoft.com/en-us/sql/relational-databases/performance/save-an-execution-plan-in-xml-format)。

### PostgreSQL：psqlのJSON出力

psqlで次を実行します。`\o`の出力先はpsqlを動かしているPCです。データを更新しないSELECTの例ですが、ANALYZEはそのSELECTを実行します。推定のみならオプションから`ANALYZE, BUFFERS,`を削除します。

```sql
\encoding UTF8
\pset format unaligned
\pset tuples_only on
\o postgresql-plan.json
EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)
SELECT customer_id, COUNT(*) FROM orders GROUP BY customer_id;
\o
```

JSON本体だけを保存します。列名`QUERY PLAN`、行数フッター、端末のプロンプト、SQL文は含めません。通常のSQLを実行できる権限が必要です。追加拡張は不要です。

公式：[EXPLAINのFORMAT／ANALYZE](https://www.postgresql.org/docs/17/sql-explain.html)、[psqlの出力設定](https://www.postgresql.org/docs/17/app-psql.html)。

### MySQL：mysqlクライアントのJSON／TREE出力

PowerShellで、ヘッダーとエスケープを抑制して保存します。`-p`はパスワード入力用です。MySQL 8.4の例です。

```powershell
mysql --default-character-set=utf8mb4 --batch --raw --skip-column-names -h 127.0.0.1 -P 23306 -u yy_demo -p yydemo -e "EXPLAIN FORMAT=JSON SELECT customer_id, COUNT(*) FROM orders GROUP BY customer_id" | Set-Content -Encoding utf8 mysql-plan.json
mysql --default-character-set=utf8mb4 --batch --raw --skip-column-names -h 127.0.0.1 -P 23306 -u yy_demo -p yydemo -e "EXPLAIN ANALYZE SELECT customer_id, COUNT(*) FROM orders GROUP BY customer_id" | Set-Content -Encoding utf8 mysql-actual.txt
```

2行目は対象SQLを実行します。JSON v2は取得側MySQLの対応版で`explain_json_format_version`を設定した場合も取り込めます。TREEは各ノードが`->`で始まる原文を保存します。`\n`等が文字として並ぶ場合は`--raw`を確認してください。MySQLの通常の表形式EXPLAINやHTMLをJSONに拡張子だけ変更しても開けません。権限は対象SQLの実行に必要なものと、ビュー等へのSHOW VIEW権限を確認します。

公式：[MySQL EXPLAIN](https://dev.mysql.com/doc/refman/8.4/en/explain.html)、[mysqlのbatch／rawオプション](https://dev.mysql.com/doc/refman/8.4/en/mysql-command-options.html)。

### Oracle：SQL*PlusからPLAN_TABLEのCSVを保存

対象SQLのEXPLAINに必要な権限と、利用可能なPLAN_TABLEを準備します。SQL*Plus 12.2以降のCSV出力を使用します。日本語を含む場合はSQL*Plus起動前に`NLS_LANG=.AL32UTF8`を設定します。以下をUTF-8の`export-oracle.sql`として保存し、対象ユーザーで接続したSQL*Plusから`@export-oracle.sql`を実行します。SPOOLはSQL*Plusを動かしている環境のファイルへ書き込みます。

```sql
{{ORACLE_PLAN_EXPORT}}
```

このスクリプトは対象SELECTを実行せず、PLAN_TABLEの作業用STATEMENT_IDだけを削除・作成します。既存の作業と衝突しないSTATEMENT_IDを使います。例は専用接続で使用し、末尾でロールバックします。CSVにSQL文・メッセージ・行数フッターを混ぜないでください。必須列は`ID,PARENT_ID,OPERATION,CARDINALITY,COST`、任意列は`OPTIONS,OBJECT_OWNER,OBJECT_NAME`と述語等です。取り込んだ追加列もプロパティに保持します。

既に実測統計を収集したカーソルがある場合は、同じCSV設定で`V$SQL_PLAN_STATISTICS_ALL`をSQL_IDとCHILD_NUMBERで1計画に絞り、`ID,PARENT_ID,OPERATION,OPTIONS,OBJECT_OWNER,OBJECT_NAME,CARDINALITY,COST,LAST_STARTS,LAST_OUTPUT_ROWS,LAST_ELAPSED_TIME`等をSELECTして保存できます。該当するV_$ビューのSELECT権限が必要です。LAST_ELAPSED_TIMEはマイクロ秒からミリ秒へ変換して表示します。統計収集の設定は[Oracleの準備](#oracle)を参照してください。DBMS_XPLAN.DISPLAYの罫線付きテキストは直接取り込めません。

実測CSVは`LAST_OUTPUT_ROWS`と`LAST_STARTS`の両方が必要です。LAST_STARTSが全行0の場合は未収集としてエラーにします。計測のある計画内で実行されなかった枝の0行は保持します。

公式：[SQL*Plus SET MARKUP等](https://docs.oracle.com/en/database/oracle/oracle-database/19/sqpug/SET-system-variable-summary.html)、[PLAN_TABLE](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/PLAN_TABLE.html)、[V$SQL_PLAN_STATISTICS_ALL](https://docs.oracle.com/en/database/oracle/oracle-database/19/refrn/V-SQL_PLAN_STATISTICS_ALL.html)。

### Db2 LUW：EXPLAIN表から8列DELを保存

Db2の8列DELは、下記のSELECTを`EXPORT ... OF DEL`で出力した**見出しなしの8列CSV**です。拡張子`.db2plan`を使います。Db2 for z/OSのファイルやdb2exfmtの帳票ではありません。対象ユーザーのEXPLAIN表・必要なEXPLAIN権限・表の参照権限を準備します。EXPLAIN表がなければ管理者と[Db2の準備](#db2)を確認してください。

下記を`export-db2.sql`として保存し、Db2 CLPで`db2 -stvf export-db2.sql`を実行します。DB名`YYDEMO`と対象SELECTを変更してください。出力ファイルはCLPを動かしている環境に作成します。

```sql
{{DB2_PLAN_EXPORT}}
```

8列の順序は`OPERATOR_ID,PARENT_ID,OPERATOR_TYPE,OBJECT_NAME,ESTIMATED_ROWS,TOTAL_COST,IO_COST,CPU_COST`です。同名の見出しを付けた通常の`.csv`も開けます。別の列順や一般のDb2データDELを`.db2plan`に変更しても対応しません。QUERYNOは作業と衝突しない値に変更し、複数ステートメントや異なるEXPLAIN時刻の行を混ぜないでください。例は最新の対象ステートメントを全キーで結合し、共有ノードに複数の親がある場合は最小TARGET_IDを表示上の親に選びます。そのため一般のDAGの全接続を再現する形式ではありません。実測情報を保持して共有する場合は、yydbmanagerで取得した`.yyplan`を利用してください。

公式：[Db2 EXPORT](https://www.ibm.com/docs/en/db2/11.5.x?topic=commands-export)、[EXPLAIN表](https://www.ibm.com/docs/en/db2/11.5.x?topic=information-explain-tables)。

### DuckDB：JSONプロファイルを保存

DuckDB CLI等で対象DBへ接続し、次を実行します。この例はSELECTを実行し、そのプロファイルをDuckDBを動かしている環境へ保存します。全結果を取得し終えてからファイルをコピーしてください。途中で次のSQLをプロファイルすると上書きされるため、対象SQLの直後に収集を停止します。普段の接続とは別の調査用接続で実行してください。

```sql
PRAGMA enable_profiling='json';
PRAGMA profiling_output='duckdb-plan.json';
SELECT customer_id, COUNT(*) FROM orders GROUP BY customer_id;
PRAGMA disable_profiling;
```

JSONの`children`／`operator_name`から演算子を復元します。CLIが表として表示したJSONのセルや、EXPLAINの罫線付き結果をそのまま保存したファイルは対象外です。演算子の時間は秒です。推定行数や実測行数がない場合は欠測として扱います。追加のサーバー権限設定は不要です。

公式：[DuckDBのプロファイリング](https://duckdb.org/docs/stable/dev/profiling)。

### SQLite：CLIでEXPLAIN QUERY PLANをCSV／JSON保存

sqlite3 CLIで対象ファイルを開き、次を実行します。`.explain off`で図式表示を抑止することが必要です。`.once`は次の1クエリだけをファイルへ出力します。CSVは`id,parent,notused,detail`の見出しを含めます。

```sql
.explain off
.headers on
.mode csv
.once sqlite-plan.csv
EXPLAIN QUERY PLAN SELECT order_id, ordered_on FROM orders
WHERE customer_id=1 ORDER BY ordered_on;
```

JSONで保存する場合は`.mode json`と`.once sqlite-plan.json`に変更します。`[{"id":3,"parent":0,"notused":0,"detail":"SCAN orders"}]`のような行配列を取り込みます。`parent=0`は根として扱います。これらはアクセス方法で、CPUや実測時間・行数を取得するものではありません。SQLiteの通常のファイル参照権限があれば使用できます。

公式：[SQLite CLIの出力形式と.once](https://www.sqlite.org/cli.html)、[EXPLAIN QUERY PLAN](https://www.sqlite.org/eqp.html)。


DBドライバーとDuckDBのライセンス表示は [database-driver-notices.md](database-driver-notices.md) を参照してください。

## 実際の画面例

5DBの実接続から取得した計画の[実行計画ペインの出力例](database-plan-examples.md)を掲載しています。色付き画像と同じ内容のテキストを確認できます。

### SQLiteとMySQLの表示用グループ

SQLiteのEXPLAIN QUERY PLANは、同じSELECTの結合アクセスや一時ソートを複数の親なしノードとして返すことがあります。この場合は`SQLite query (display group)`へ接続し、同じSQLの計画として表示します。このノードは画面上の分類であり、DBが報告したJOIN演算子ではありません。詳細の`Display.Synthetic=true`で区別できます。元のEQP・既存の親子関係・計測値は保持し、DBが返さないコストや実測値は追加しません。

MySQLの実測TREEが主問い合わせとSELECT句内サブクエリを別の親なしノードとして返す場合も、同じSQLなら表示用グループにまとめます。推定JSONの`select_list_subqueries`も表示します。保存した`.yyplan`とSVGには表示用グループを含み、原文表示では元のDB出力を確認できます。
