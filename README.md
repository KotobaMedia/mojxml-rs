# mojxml-rs

法務省登記所備付地図データ（地図XML）を高速でGISデータ形式（現在は FlatGeobuf を対応しています）に変換するコマンドラインツールです。

このツールは Rust で書いていますが、 [`mojxml-py`](https://github.com/ciscorn/mojxml-py) や[デジタル庁が提供している `mojxml2geojson`](https://github.com/digital-go-jp/mojxml2geojson) ツールを参考に作成しています。

## このツールの特徴

* 効率的に利用可能のプロセッサーをすべて並列で使うことができる。（FlatGeobuf出力は1スレッドに限られているため、上限はあるが、32スレッドでそのボトルネックは現れなかった）
* 高速で処理できる。（著者の環境: Ryzen 9 9950X 16C/32T 96GB RAM の内、最大約 20GB 使用で全国 2025年度データを73分で一つの FlatGeobuf ファイルに変換できた）
* zip内のzipアーカイブを自動で解凍する
* 複数入力ファイルが統合されて一つの出力ファイルになります
* Windows, Linux, macOS それぞれの OS で実行できるバイナリとして提供しています (work in progress)

## インストール方法

このツールは Rust で書かれていますが、コンパイル済みバイナリも下記のアーキテクチャで用意しています。

* macOS (Apple Silicon)
* Windows
* Linux (x86_64)

[最新の Releases](https://github.com/KotobaMedia/mojxml-rs/releases) から利用環境の zip アーカイブをダウンロードしていただき、解凍したらコマンドラインで実行できます。お困りの方は [GitHub Issues](https://github.com/KotobaMedia/mojxml-rs/issues) で詳細を教えて下さい。

> [!NOTE]
> macOS の場合は Gatekeeper の設定の関係で実行できない場合があります。次リリースには改善する予定ですが、今のところは `xattr -d com.apple.quarantine ./mojxml-rs` を1回実行してたら `./mojxml-rs` を通常通り実行できるようになります。

## 使い方

```
Usage: mojxml-rs [OPTIONS] <DST_FILE> <SRC_FILES>...

Arguments:
  <DST_FILE>      Output FlatGeobuf file path
  <SRC_FILES>...  Input MOJ XML file paths (.xml or .zip)

Options:
  -a, --arbitrary            Include features from arbitrary coordinate systems (unmapped files) ("任意座標系")
  -c, --chikugai             Include features marked as outside district ("地区外") or separate map ("別図"). You probably don't need this
  -d, --disable-fgb-index    Disable FlatGeobuf index creation (turn this off for large exports)
  -v, --verbose              Enable logging. Will log to mojxml.log in the current directory
  -t, --temp-dir <TEMP_DIR>  Optional temporary directory for unzipping files. If not specified, the default temporary directory will be used. Use this option if your /tmp directory doesn't have enough space
  -h, --help                 Print help
  -V, --version              Print version
```

例:

```
mojxml-rs ./moj-2025-46.fgb ../dl-tool/zips/46*.zip
```

上記のコマンドは、 `dl-tool` でダウンロードした鹿児島県のすべてのzipファイルを、 `moj-2025-46.fgb` のFlatGeobuf にまとめて変換します。

> [!TIP]
> Linux のディストリビューションによって `/tmp` ディレクトリは tmpfs (メモリ上のファイルシステム) になっている。 `mojxml-rs` は親ZIPを解凍するときはテンポラリファイルを使うため、メモリをひっ迫する可能性があります。これを防ぐために、 `-t` オプションでディスク上のテンポラリディレクトリを指定してください。

## プログレスバーの説明

```
[unzipping] 00:04:20 #######---------------------------------     309/2006
[XML parse] 00:04:20 ########################################   25160/25201
[FGB write] 00:04:20 ########################################   25159/25160
```

* `unzipping` は入力ZIPファイルを指します。この場合、全部2006個の内309個目は解凍完了。解凍は基本的に1スレッドで行います。（解凍が次のステップより速かったらメモリが圧迫されてしまうため）
* `XML parse` は解凍されたXMLをメモリ上に読み込まれ、必要な情報の抽出やGISデータの変換を指します。親ZIPの数がわかっても、その中のZIPの数は事前にわからないので、解凍が進むと母数値が増えます。
* `FGB write` は FlatGeobuf の書き込みを指します。このツールの場合は、メモリ上に書き込んで、すべての処理が完了してからディスクに書き出します。

より詳細なログがほしい場合は `--verbose` で実行すると `mojxml.log` ファイルに個別ファイルの読み込み・書き込み状況をログ形式で出力します。

## ライセンス

このツールのソースコードは MIT ライセンスで公開しています。
