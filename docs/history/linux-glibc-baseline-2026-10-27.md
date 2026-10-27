# 2026-10-27 Linux版のglibc 2.28対応

## 問題

v0.8・v0.8.1のLinux版は、CIの `ubuntu-24.04` 上でビルドしていた。そのため実行ファイルが `GLIBC_2.39`（`pidfd_spawnp`、`pidfd_getpid`）と `GLIBC_2.38`（`__isoc23_strtol` 等）を要求し、glibc 2.39未満の環境では「GLIBC_2.39 not found」で起動しなかった。`yyte` はさらに `libssl.so.3`・`libcrypto.so.3` と `GLIBCXX_3.4.29`（GCC 11以降のlibstdc++）を要求していた。

## 方針

glibc自体の同梱は採らない。glibcは `ld-linux` と対で読み込まれ、名前解決・ロケール等もOS側と結び付くため、同梱すると専用の起動処理が必要になり、端末やDB接続の一部が壊れやすい。代わりに次を行った。

- 下限をglibc 2.28（RHEL 8系、Debian 10、Ubuntu 20.04以降）とし、LinuxのCIジョブを `quay.io/pypa/manylinux_2_28_x86_64`／`_aarch64` コンテナー内で実行する。gcc-toolsetは新しいC++機能を実行ファイルへ取り込み、システムのlibstdc++（GCC 8相当）だけを参照する。
- Linuxでは `openssl` クレートの `vendored` 機能でOpenSSL 3.6.3（`openssl-src` 300.6.1）を静的リンクする。ビルドに `perl-IPC-Cmd`・`perl-Time-Piece` を導入する。
- `tools/release.py verify` と梱包処理は、ELFの `DT_VERNEED` を読み、`GLIBC_2.28`・`GLIBCXX_3.4.25`・`CXXABI_1.3.11` を超える要求と、`libssl`／`libcrypto` へのリンクがあれば失敗する。公開済みv0.8の資材に適用し、`yyte` は `libssl.so.3`、`markdownViewer` は `GLIBCXX_3.4.29` で拒否されることを確認した。

## 検証

`manylinux_2_28_x86_64`（ldd 2.28、GCC 14.2.1）のコンテナーで、全ターゲットClippy、`tools/test_release.py`（5件）、通常試験120件（実PTY 17件を含む）、releaseビルドと `tools/release.py verify` が成功した。releaseビルドの要求は次のとおり。

- `yyte`：NEEDEDは `libstdc++.so.6`、`libgcc_s.so.1`、`libutil.so.1`、`libpthread.so.0`、`libm.so.6`、`libdl.so.2`、`libc.so.6`、`ld-linux-x86-64.so.2`。最大は `GLIBC_2.28`、`GLIBCXX_3.4.22`、`CXXABI_1.3.8`。
- `markdownViewer`：最大は `GLIBC_2.28`、`GLIBCXX_3.4.21`、`CXXABI_1.3.8`。

このx86_64版をAlmaLinux 8.10（glibc 2.28）、Debian 10（glibc 2.28）、Ubuntu 20.04（glibc 2.31）の各コンテナーで起動し、`--version`、`--help`、`markdownViewer --plain` が動作した。

同じ `native-tls` 0.2.18と静的リンクしたOpenSSLで作った確認用プログラムを同じ3環境で実行した。`native-tls` は `openssl-probe` でOSのCA証明書を探す。github.comへの接続は証明書検証に成功し、self-signed.badssl.comは拒否された。CA証明書のないDebian 10の最小イメージでは、github.comも検証失敗となり接続しなかった。

ARM64版はローカルで実行しておらず、CIの `manylinux_2_28_aarch64` 上のネイティブ検証を用いる。外部DBへのTLS接続は今回試していない。
