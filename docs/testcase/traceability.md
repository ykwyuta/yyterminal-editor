# 対応表

## コマンドとユースケース

`src/commands.rs` の `COMMANDS` 全125件が、いずれかのユースケースで試験されるようにした。

### 通常メニュー

| コマンド | ユースケース |
|---|---|
| New file | UC-03-02 |
| New workbook | UC-05-02 |
| Open file | UC-03-01 |
| Open encoding | UC-03-08、UC-06-01 |
| Workspace folders | UC-02-08 |
| Save | UC-03-06、UC-04-03、UC-04-08、UC-05-02 |
| Save as | UC-03-07、UC-05-02 |
| Export CSV | UC-05-09 |
| Export fixed-length | UC-05-12 |
| Close tab | UC-02-07 |
| Quit | UC-01-05 |
| Undo / Redo | UC-03-04、UC-04-04、UC-04-05、UC-04-09、UC-05-03 |
| Cut / Copy / Paste | UC-03-05、UC-04-04、UC-05-04、UC-07-04 |
| Select all | UC-03-03 |
| Copy to system clipboard | UC-03-05、UC-05-04 |
| Edit cell | UC-04-03、UC-05-03 |
| Find text | UC-03-09 |
| Compare file (yymerge) | UC-06-01〜UC-06-03 |
| Explorer | UC-02-01 |
| DB Manager | UC-10-01 |
| Syntax language | UC-03-10 |
| Folder tree / Folder navigation / Refresh explorer | UC-02-02 |
| Next tab | UC-02-07 |
| Markdown preview | UC-09-01 |
| Split Markdown preview | UC-09-02 |
| Preview remote images | UC-09-05 |
| Insert rows / Delete rows / Insert columns / Delete columns | UC-05-05 |
| Append record / Delete record | UC-04-05、UC-04-08 |
| Add sheet / Next sheet | UC-05-08 |
| Column width / Row height | UC-04-06、UC-05-07 |
| Bold / Number format | UC-05-07 |
| Spreadsheet mode | UC-05-01 |
| CSV table / TSV table / Delimited table | UC-04-01、UC-04-02 |
| Fixed-length table | UC-04-07 |
| Apply layout (.yyl) / Save layout (.yyl) | UC-05-10 |
| Infer fixed layout | UC-05-11 |
| Row layout | UC-05-12 |
| Move column | UC-05-05 |
| Sort ascending / Sort descending / Filter column / Clear sort/filter | UC-05-06 |
| Autofill down | UC-05-03 |
| Terminal / New terminal | UC-07-01、UC-07-02 |
| Git status / Git diff / Git staged diff | UC-08-01 |
| Git stage all / Git unstage all | UC-08-02 |
| Git commit | UC-08-03 |
| Language: English / Language: Japanese | UC-01-06、UC-02-06 |
| Help | UC-01-04 |

### DB管理画面のメニュー

| コマンド | ユースケース |
|---|---|
| DB New connection / DB Edit connection / DB Delete connection | UC-10-02 |
| DB Select connection / DB Connect / DB Disconnect | UC-10-03 |
| DB Refresh objects | UC-10-04 |
| DB Messages | UC-10-05 |
| DB Performance SQL | UC-10-09 |
| DB New SQL / DB Execute / DB Query | UC-10-05 |
| DB Cancel | UC-10-07 |
| DB Commit / DB Rollback | UC-10-06 |
| DB Results / DB Next result | UC-10-05、UC-10-08 |
| DB Column width / DB Row height / DB Copy cell | UC-10-08 |
| DB Export CSV / DB Export TSV / DB Export delimited / DB Export Parquet / DB Export fixed | UC-10-10 |
| DB Import CSV / DB Import TSV / DB Import delimited / DB Import Parquet / DB Import fixed | UC-10-11 |
| DB Open plan | UC-11-01 |
| DB Estimated plan / DB Actual plan / DB Db2 actual setup SQL | UC-11-02 |
| DB Plan / DB Next operator / DB Previous operator / DB Next warning | UC-11-03、UC-11-07 |
| DB Save plan | UC-11-05 |
| DB Plan raw / DB Plan SVG | UC-11-06 |
| DB ER / DB ER from database / DB Important fields | UC-12-01 |
| DB New ER / DB Add ER table | UC-12-02 |
| DB Open ER / DB Save ER / DB Edit ER YAML | UC-12-03 |
| DB Recommend relations / DB Apply relation | UC-12-04 |
| DB Edit ER columns / DB Column types / DB Apply ER columns | UC-12-05 |
| DB ER SVG / DB ER Markdown / DB ER DDL | UC-12-06 |

## CLIオプションとユースケース

| コマンド | オプション | ユースケース |
|---|---|---|
| yyte | `[FILE\|DIRECTORY\|WORKSPACE.yyworkspace]` | UC-01-01、UC-01-02 |
| yyte | `--encoding NAME`、`--list-encodings` | UC-01-03 |
| yyte | `--open-plan PLAN_FILE` | UC-11-01 |
| yyte | `--help`／`-h`、`--version` | UC-01-04 |
| markdownViewer | `FILE`、`-`（標準入力） | UC-09-03 |
| markdownViewer | `--plain`、`--print`、`--color`、`--width N`、`--image-height N`、`--base-dir DIR` | UC-09-03 |
| markdownViewer | `--remote-images` | UC-09-05 |
| markdownViewer | `--help`／`-h`、`--version` | UC-09-03 |

## 環境変数・設定ファイル

| 対象 | ユースケース |
|---|---|
| `YYTE_CONFIG_DIR`、`XDG_CONFIG_HOME`、`ui.conf` | UC-01-06 |
| `databases.json`、パスワード環境変数 | UC-10-02、UC-13-04 |
| `SHELL` | UC-07-01 |

## 既存の自動試験とユースケース

| 既存試験 | 主なユースケース |
|---|---|
| `tests/cui.rs::interactive_editor_save_preview_shell_resize_and_exit` | UC-01-02、UC-01-05、UC-01-07、UC-03-03、UC-03-06、UC-07-01、UC-09-01 |
| `tests/cui.rs::interactive_csv_table_edits_and_saves_quoted_multiline_cell` | UC-04-01、UC-04-03 |
| `tests/cui.rs::interactive_fixed_table_edits_packed_decimal_and_preserves_other_bytes` | UC-04-07、UC-04-08 |
| `tests/cui.rs::interactive_workbook_sizes_formulas_save_and_yymerge_diff` | UC-05-01〜UC-05-03、UC-05-07、UC-06-01、UC-06-03 |
| `tests/cui.rs::interactive_preview_renders_local_pixels_and_mermaid` | UC-09-04、UC-09-06 |
| `tests/cui.rs::standalone_interactive_preview_resizes_and_exits` | UC-09-03 |
| `tests/cui.rs::interactive_mouse_menu_folder_and_file_navigation` | UC-01-01、UC-02-01、UC-02-05 |
| `tests/cui.rs::mouse_terminal_tabs_and_live_split_preview` | UC-02-07、UC-07-02、UC-07-05、UC-09-02 |
| `tests/cui.rs::mouse_confirms_save_as_and_viewer_buttons_work` | UC-03-02、UC-03-03、UC-03-07、UC-09-03 |
| `tests/cui.rs::categorized_menus_tree_wrapping_open_modes_and_language_persist` | UC-01-06、UC-02-02〜UC-02-06 |
| `tests/cui.rs::large_preview_end_navigation_and_spreadsheet_table_render_in_both_apps` | UC-09-01、UC-09-07、UC-09-10 |
| `tests/cui.rs::editor_and_markdown_preview_highlight_languages_and_allow_manual_override` | UC-03-10、UC-09-09 |
| `tests/cui.rs::preview_block_spacing_is_visible_in_editor_and_standalone_viewer` | UC-09-08 |
| `tests/cui.rs::database_query_grid_plan_mouse_and_resize_use_real_terminal` | UC-10-03、UC-10-05、UC-10-08、UC-11-01、UC-11-03 |
| `tests/cui.rs::database_management_menus_are_local_in_real_terminal` | UC-01-07、UC-02-09、UC-10-01 |
| `tests/cui.rs::sqlite_join_plan_connects_in_real_terminal` | UC-11-02、UC-11-04 |
| `tests/markdown_viewer.rs`（4試験） | UC-09-03、UC-09-07、UC-09-09、UC-09-10 |
| `tests/plan_scenarios.rs`（SQLite/DuckDB 2試験、外部5DBはignored） | UC-11-02〜UC-11-06 |
| `tests/db2_actual_plan.rs`（ignored、yydbmanager検証用ComposeのDb2） | UC-11-02 |
| `tests/database_live.rs`（外部5DB、ignored） | UC-10-03、UC-10-05〜UC-10-11、UC-12-01、UC-12-03 |
| `src/*.rs` の単体試験（約100件） | 各ファイルの「単体」と記載した行 |

## 自動試験が見当たらない主な領域

試験開始時に優先して自動化を検討する候補（優先度A・Bで既存試験の欄が「—」のもの）。

- Git操作全般（UC-08-01〜UC-08-04）。特に引数のシェル展開がないこと（TS-08-04-01）
- 未保存ありの終了で `discard` 以外を拒否する実端末確認（TS-01-05-03）、処理中の終了拒否（TS-01-05-06）
- CLIの異常系（TS-01-01-04、TS-01-03-01、TS-01-03-04、TS-01-03-05、TS-11-01-02）
- ワークスペースファイルを書き換えないこと（TS-02-08-05）
- 外部変更後の保存拒否をCSV・ブックで確認（TS-04-03-08、TS-05-02-05）
- 固定長エクスポートの出力先上書き拒否と部分出力なし（TS-05-12-10、TS-05-12-11）
- パスワードがどのファイルにも残らないこと（TS-13-04-01）
- 実測計画が実行を伴うことの確認（TS-11-02-04）
- 保存中の強制終了・ディスク満杯（TS-13-02-08、TS-13-02-09）
