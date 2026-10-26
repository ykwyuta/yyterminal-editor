# ユースケースとテストシナリオ

2026-10-25時点の作業ツリー（v0.7.2相当）を対象に、`yyte` と `markdownViewer` を使う場面を洗い出し、ユースケースごとにテストシナリオを作成した。ここに書いたのは**試験設計**である。まだ試験は実施していない。合否や実施日は記録していない。

## 根拠と対象範囲

洗い出しには次の資料とコードを使った。

- 利用説明: `README.md`、`docs/user/help.md`、`docs/user/markdown-viewer.md`、`docs/user/database.md`
- 内部仕様: `docs/developer/README.md`、`docs/developer/database-live-tests.md`
- コマンド定義: `src/commands.rs`（`COMMANDS`、`MENUS`、`DATABASE_MENUS`）、CLI解析: `src/main.rs` の `main`、`src/bin/markdown_viewer.rs`
- 既存試験: `tests/cui.rs`、`tests/markdown_viewer.rs`、`tests/plan_scenarios.rs`、`tests/database_live.rs`、各 `src/*.rs` の単体試験

対象は**実装済みと文書に書かれている機能**に限る。次の機能は未実装と明記されているため、ユースケースから除外した。ただし「未実装であることを利用者に誤認させない」確認だけは非機能シナリオ（[13](13-nonfunctional.md)）に含めた。

- SSHリモート編集、言語サーバー
- `.yyworkspace` の作成・保存（読み込みのみ対応）
- OSクリップボードの読み取り（OSC 52による書き込みのみ対応）
- yymerge比較でのマージ・行内強調・比較のキャンセル
- 開く・保存の進捗割合とキャンセル、クラッシュ後の編集復旧、同時書き込みの排他ロック
- 表モードの全件行番号索引・総件数・任意行へのジャンプ

## アクター

| アクター | 説明 |
|---|---|
| 編集者 | 端末でテキスト・Markdown・設定ファイルを編集する開発者 |
| データ担当者 | CSV/TSV・固定長（COBOLコピーブック）・`.yys` ブックを扱う担当者 |
| DB利用者 | SQL実行、結果確認、実行計画・ER図の確認を行う担当者 |
| 閲覧者 | `markdownViewer` でMarkdownを読む、またはテキストへ出力する利用者 |
| 外部プロセス | 同じファイルを書き換える別プログラム、Git、シェル、DBサーバー、HTTPサーバー |
| 配布担当者 | CIの資材から配布物を作る担当者 |

## ファイル構成

| ファイル | 領域 | ユースケース |
|---|---|---|
| [01-launch-settings.md](01-launch-settings.md) | 起動・CLI・終了・設定 | UC-01-01〜UC-01-07 |
| [02-explorer-menu-tabs.md](02-explorer-menu-tabs.md) | エクスプローラー・メニュー・タブ・言語・ワークスペース | UC-02-01〜UC-02-09 |
| [03-text-editing.md](03-text-editing.md) | ファイルを開く・編集・保存・文字コード・検索・クリップボード・構文色 | UC-03-01〜UC-03-12 |
| [04-table-editing.md](04-table-editing.md) | CSV/TSV/任意区切り・固定長の表編集 | UC-04-01〜UC-04-09 |
| [05-workbook-layout.md](05-workbook-layout.md) | `.yys` ブック・`.yyl` レイアウト・固定長エクスポート | UC-05-01〜UC-05-12 |
| [06-compare.md](06-compare.md) | yymerge比較 | UC-06-01〜UC-06-03 |
| [07-terminal.md](07-terminal.md) | 統合ターミナル | UC-07-01〜UC-07-05 |
| [08-git.md](08-git.md) | Git操作 | UC-08-01〜UC-08-04 |
| [09-markdown-preview.md](09-markdown-preview.md) | Markdownプレビュー・単独ビューアー | UC-09-01〜UC-09-12 |
| [10-database.md](10-database.md) | DB接続・SQL・結果・取り込み/書き出し | UC-10-01〜UC-10-11 |
| [11-plan.md](11-plan.md) | 実行計画 | UC-11-01〜UC-11-07 |
| [12-er.md](12-er.md) | ER図 | UC-12-01〜UC-12-07 |
| [13-nonfunctional.md](13-nonfunctional.md) | 性能・大容量・安全性・画面サイズ・異常系・配布 | UC-13-01〜UC-13-10 |
| [traceability.md](traceability.md) | コマンド・CLIオプションとユースケースの対応、既存試験との対応 | — |

## シナリオの書式

各ユースケースは次の形で記載する。

- **ID・名称**、**アクター**、**目的**、**事前条件**、**基本フロー**
- **シナリオ表**: `ID`、`分類`、`手順`、`期待結果`、`レベル`、`優先度`、`既存試験`

分類:

- 正常: 基本フローどおりに完了する
- 代替: 別の操作経路（マウス・メニュー・F1・キーボード）で同じ結果になる
- 異常: 誤入力・外部要因で失敗し、データを壊さずに理由を示す
- 境界: 上限・下限・空・最小画面など

レベル:

| 記号 | 意味 | 実施手段 |
|---|---|---|
| U | 単体・モデル試験 | `cargo test --workspace --locked`（`src/*.rs` の試験、`TestBackend` 描画） |
| P | Linux実PTY試験 | `tests/cui.rs` の方式。SGRマウス・キー列を送り、期待画面を期限付きで待つ |
| CLI | 非TTYのコマンド実行 | `tests/markdown_viewer.rs` の方式。終了コード・標準出力・標準エラーを照合 |
| L | 外部DB実接続 | `--ignored`／`--include-ignored`。`docs/developer/database-live-tests.md` の環境 |
| R | 配布・CI | `tools/test_release.py`、`tools/release.py verify`、CIの4環境 |
| M | 手動確認 | 端末の色・フォント・OSC 52など、自動判定が難しい見た目 |

優先度:

- A: データ破損・データ喪失・誤実行に関わる、または主要機能の基本フロー
- B: 主要機能の代替経路・主要な異常系
- C: 表示の細部・補助機能

既存試験の欄は、関数名が同じ観点を一部でも確認していれば記載した。全手順を満たすとは限らない。空欄（—）は現時点で対応する自動試験が見当たらないもの。

## 共通の事前条件・試験実施上の約束

`AGENTS.md` と `docs/developer/README.md` に従う。

1. 試験前に `.github/workflows/ci.yml` を確認し、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、該当する試験を実行する。
2. 設定は `YYTE_CONFIG_DIR` を一時ディレクトリに隔離し、利用者の `ui.conf`・`databases.json` を読み書きしない。
3. 非同期処理は期待する画面・文書・ファイル内容を**期限付きで待つ**。固定sleepを完了条件にしない。失敗時は画面内容を出力する。
4. 自分で起動した `yyte`・シェル・DBワーカー・HTTPサーバーは試験の終わりに終了・回収する。
5. PTYの実端末試験はLinuxで行う。Windowsの試験件数を代わりにしない。
6. ファイルの検証は**バイト列**で照合する（文字コード・改行・不正バイト・パック十進数を含む）。
7. 外部ネットワークへ接続しない。HTTP画像はローカルの期限付きHTTPサーバーを使う。
8. 外部DBの共有デモ表は変更しない。試験用の表は `yyte_probe_<PID>` などの専用名にして後始末する。

## 試験データ

既存の `demo/` を優先して使う。追加が必要なデータは各ファイルの「試験データ」に記載した。主なもの:

| データ | 用途 |
|---|---|
| `demo/data.csv` | 引用符内改行・日本語・カンマ・二重引用符を含むCSV |
| `demo/records.dat` + `demo/record.cpy` | 13バイト固定長、COMP-3、未定義領域 `DE AD`／`BE EF` |
| `demo/markdown-preview.md` + `demo/preview-pixels.png` | 画像・Mermaid・表・コード |
| `demo/demo.yyworkspace` | ワークスペース読み込み |
| `demo/database/*.yyplan`、`*.sqlplan`、`sqlite.sql`、`query.sql`、`plan-scenarios.json` | 実行計画・SQL |

## 規模

13領域・108ユースケース・575シナリオ（2026-10-26更新）。ODBCの削除に合わせ、ODBC形式の接続先とDb2実測計画を拒否するシナリオを追加した。
