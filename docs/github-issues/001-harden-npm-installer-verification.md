# npm installer の配布検証を fail-closed にする

## 背景

`npm install -g ccmux-fork` 時に実行される `npm/scripts/install.js` は、GitHub Releases からプラットフォーム別バイナリを取得して `bin/` に配置する。

現状の実装では次の 2 点が残っている。

- リダイレクト先 URL をそのまま追跡しており、許可ホストの制限がない
- `checksums.txt` の取得失敗や対象行未発見時に警告だけで続行し、未検証バイナリをインストールしてしまう

これにより、配布経路に異常があった場合に checksum 未検証のバイナリがそのまま実行可能状態で配置される。

## 対象コード

- `npm/scripts/install.js`

## 対応内容

- ダウンロード URL / リダイレクト先を `github.com` および正当な Releases 配布先に限定する
- `checksums.txt` の取得失敗時は警告継続ではなく install 全体を失敗させる
- `checksums.txt` 内に対象バイナリの行が見つからない場合も install を失敗させる
- checksum 検証成功時のみ `bin/ccmux` / `bin/ccmux.exe` を最終配置する
- 可能なら一時ファイルに保存して検証後に rename する

## 受け入れ条件

- checksum が取得できない状態では `npm install -g ccmux-fork` が失敗する
- checksum 不一致時に未検証バイナリが `bin/` に残らない
- 想定外ホストへのリダイレクトは拒否される
- 正常系では従来通り各対応プラットフォームでインストールできる

## 補足

セキュリティレビューでは High 扱い。ユーザーに配布する実行ファイルの取得経路なので、警告継続ではなく fail-closed が妥当。
