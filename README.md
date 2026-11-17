# Kindle for Linux

Arch Linux などの Linux デスクトップで Kindle の本を読むための、**非公式の軽量クライアント**です。

Amazon 公式の [Kindle Cloud Reader](https://read.amazon.co.jp/)（ブラウザ版 Kindle）を、GTK4 / libadwaita / WebKitGTK で作った専用ウィンドウで動かします。普通のブラウザのタブで開くのと比べて、次の点が便利です。

- ログイン状態が専用プロファイルに保存され、毎回ログインし直す必要がない
- 起動すると前回読んでいた本・ページから再開できる
- 読書に集中できる全画面モード（F11 / Esc）
- ズーム倍率・ウィンドウサイズを記憶
- Amazon 以外のリンクは普段使いのブラウザで開く
- 日本・米国・英国・ドイツなど 12 のストアに対応（メニューまたは `--region` で切り替え）
- 依存しているのは Arch の公式リポジトリのパッケージだけ。Electron は使っていません

> Amazon、Kindle は Amazon.com, Inc. またはその関連会社の商標です。本ソフトウェアは Amazon とは関係ありません。
> DRM の解除は行いません。表示しているのは Amazon 公式の Web リーダーそのものです。

## インストール (Arch Linux)

### PKGBUILD でインストール（おすすめ）

```sh
git clone https://github.com/TarouZirou/Kindle-for-Linux.git
cd Kindle-for-Linux/packaging/arch
makepkg -si
```

### 手動でインストール

```sh
sudo pacman -S --needed python python-gobject gtk4 libadwaita webkitgtk-6.0 gst-plugins-base gst-plugins-good
git clone https://github.com/TarouZirou/Kindle-for-Linux.git
cd Kindle-for-Linux
sudo make install          # アンインストールは sudo make uninstall
```

### インストールせずに試す

```sh
./bin/kindle-for-linux
```

## 使い方

アプリメニューの「Kindle for Linux」から起動するか、端末で `kindle-for-linux` を実行します。初回だけ Amazon アカウントでログインしてください。

```
kindle-for-linux                # 起動
kindle-for-linux --region us    # amazon.com のライブラリを使う（設定は保存されます）
kindle-for-linux --reset        # ログイン情報・キャッシュ・設定をすべて削除
```

ストア: `jp` `us` `uk` `de` `fr` `es` `it` `ca` `au` `in` `br` `mx`
（日本語ロケールのときの既定は `jp`、それ以外は `us`）

### キーボードショートカット

| キー | 動作 |
| --- | --- |
| F11 | 全画面の切り替え |
| Esc | 全画面を終了 |
| Ctrl + L | ライブラリへ戻る |
| Alt + ← / → | 戻る / 進む |
| Ctrl + R / F5 | 再読み込み |
| Ctrl + + / - / 0 | 拡大 / 縮小 / 等倍 |
| Ctrl + Shift + I | 開発者ツール |
| Ctrl + Q | 終了 |

ページ送り（← / →）、しおり、ハイライト、文字サイズの変更などは Kindle Cloud Reader 側の機能をそのまま使えます。

### データの保存場所

| 内容 | 場所 |
| --- | --- |
| 設定 | `~/.config/kindle-for-linux/config.json` |
| ログイン情報 (Cookie) など | `~/.local/share/kindle-for-linux/` |
| キャッシュ | `~/.cache/kindle-for-linux/` |

## 困ったとき

- **「このブラウザには対応していません」と表示される**: User-Agent を変えてみてください。
  ```sh
  kindle-for-linux --user-agent "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"
  ```
  元に戻すには `--user-agent ""` を指定します。
- **画面が真っ白になる / 描画がおかしい**（主に NVIDIA の環境）:
  `WEBKIT_DISABLE_DMABUF_RENDERER=1 kindle-for-linux` で起動してみてください。
- **ログインがおかしくなった**: メニューの「ログアウト / データ消去…」を使うか、`kindle-for-linux --reset` を実行します。

## 制限事項

Kindle Cloud Reader の仕様をそのまま受け継ぎます。

- オフラインでは読めません（Cloud Reader にオフライン機能がないため）
- Cloud Reader で読めない一部の本（一部のコミック・雑誌・固定レイアウト作品など）は、このアプリでも読めません
- 自分で送った個人ドキュメント (Send to Kindle) が表示されない場合があります

### ほかの方法

- **Kindle for PC を Wine で動かす**: オフラインで読めますが、バージョンによって動かないことがあり、動作は不安定になりがちです。
- **DRM のない電子書籍**（自分で送った PDF / EPUB など）: Calibre、Foliate、Okular といった Linux ネイティブのリーダーで読めます。

## 開発

```
kindle_for_linux/
  app.py      # Adw.Application とコマンドライン引数
  window.py   # メインウィンドウ（WebKitGTK のビュー、メニュー、ショートカット）
  config.py   # 設定と XDG パス、ストア一覧
bin/kindle-for-linux     # 起動スクリプト
data/                    # .desktop ファイルとアイコン
packaging/arch/PKGBUILD
```

ライセンス: MIT
