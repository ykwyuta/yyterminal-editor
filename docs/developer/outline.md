# yyoutline互換編集

`src/outline.rs`がyyeditorの`yyoutline` version 1形式をRatatui上で編集する。参照した仕様・モデルはyyeditor `c5defe2ba36f9113201c2bf5709b5a82bf4c32ec` の `docs/user/outline.md`、`crates/yy-preview/assets/outline-model.js`。上流のモデルはWebView用JavaScriptであり、共有Rustのアウトラインモデルは存在しないため、端末の操作とシリアライズをローカルで実装した。上流のJavaScriptソースは同梱・複製せず、既存YYクレートの完全SHA固定も変更しない。

平坦な先行順配列で枝を連続範囲として扱い、再帰せず移動する。先頭depth=0、以降は直前depth+1以下の非負整数、IDは非空かつ一意、項目の各必須フィールドの型を検証する。JavaScriptで有効な整数値JSON（1.0など）も受け付ける。参照先不存在はエラーにせず維持する。未知フィールドはserdeのflattenで保持する。

各Editorに表示状態とDocumentバージョンを持ち、同じバージョンの再描画では再パースしない。変更はモデルのコピーで検証し、文書の改行コードへ正規化した後も1 MiB以下になることを確認してからDocumentの全体置換を1回行う。元の文書・Undo/Redo・保存時の外部変更検出は共通の経路を使う。入力ダイアログは、開いた時点のタブ・Documentバージョン・選択IDと照合して確定し、非同期処理などで対象が変わった場合は反映を拒否する。JSON原文を編集した後とUndo/Redoではバージョンに従ってモデルを再作成する。折りたたみとブックマークはJSONに、選択ID・ズーム・スクロールはタブ内だけに保持する。描画する行数・字下げ・参照ラベルの総量は画面寸法で制限する。

Markdown出力は全文を上限付きで生成した後、出力先と同じディレクトリの一時ファイルへ書き、persist_noclobberで公開する。既存ファイル・原本は上書きしない。URL起動はHTTP(S)に限定してOSハンドラーへ引数で渡す。シェル展開を行わず、起動したハンドラーを期限付きで待ち、終了または回収する。

モデル試験は階層移動、深い枝、未知フィールド、削除後の参照、Undo/Redo、過大入力のロールバック、不正JSON、狭い画面とMarkdown上限を確認する。`tests/cui.rs`はLinux実PTYでyyoutlineを開き、枝移動、F2での日本語編集、参照、保存・再読込、Undo/Redo、Markdown出力、原文切り替えを検証する。
