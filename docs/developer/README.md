# 開発・検証

## ビルド

`rust-toolchain.toml` の Rust 1.99.0 を利用する。LinuxではGitとシェルが実行できること。端末にはUTF-8と通常のANSIカーソル操作が必要。アイコンフォント、WebView、X11は必須としない。

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --locked
cargo run --locked -- --help
```

`.github/workflows/ci.yml` は独立したLinux/Windows CI。LinuxではPTYの実接続と実際のCUI操作を試験する。WindowsでLinux専用試験が0件になることをLinux検証の代わりにしない。CIが未実行の場合、ローカル試験の成功だけでGitHub CI成功と表現しない。

## 内部構成

- `src/main.rs`: 状態、キーマップ、コマンド、ワーカーとのメッセージ、CLI。
- `src/editor.rs`: YYのDocumentをラップし、専用ディレクトリへストリームコピーして原本の可変mmapを避ける。読み込み前後のSHA-256を照合する。
- `src/shell.rs`: portable-ptyで生成したシェルの入出力をyy-termに接続。出力キューとスクロールバックは有界。子のANSIを外側端末へ直接出力しない。
- `src/git.rs`: 引数をシェル展開せずGitへ渡す。操作はワーカー上で実施。現在は出力全体をメモリへ取得するため大規模diffは未最適化。
- `src/ui.rs`: Ratatuiの画面描画、Markdownから端末テキストへの変換。待機中は変更があった場合だけ再描画する。
- `tests/cui.rs`: Linuxの実PTYで日本語貼り付け、保存、シェル通信、リサイズ、画面切り替え、終了を確認する。

保存は共有モデルのスナップショット・元文字コード・不正バイト復元規則を使用する。原本とのハッシュ照合後、共有IOによる置換保存を行う。新規保存は同一ディレクトリ上の一時ファイルから上書き禁止で確定する。保存中に続けて行った編集は、保存した版と現在版を区別して未保存のまま残す。タブ終了・アプリ終了は処理中に許可しない。

ハッシュ照合と置換の間の他プロセスによる変更を排他的にロックする仕組みはまだない。他プロセスと同じ原本を同時保存する用途は未対応。開く・保存する処理の進捗割合、キャンセル、クラッシュ時の編集復旧も今後の実装対象。

可視行は1行あたり最大16KiB、画面幅で分割し、巨大な単一行全体をStringにしない。Markdownは先頭1MiB、検索はカーソルの次から16MiBに制限する。検索・プレビューも全容量に対応する段階ではワーカー化と専用索引が必要。

## 共有コード

`yy-core`、`yy-buffer`、`yy-io`、`yy-jobs`、`yy-config`、`yy-term` は yyeditor の完全なGit SHAで固定。`yy-encoding` はその依存先であるYYLibraryの固定SHAを利用する。実装や文字コード表をこのリポジトリへ複製しない。依存の更新時にはCargo.lockと互換性試験を更新する。

Linux検証に使用したコンテナは `rust:1.99.0-bookworm`。実PTY試験にはホスト画面やX11は必要ない。非TTY環境で対話UIを直接起動せず、`cargo test` のPTY試験または `--help` を使用する。
