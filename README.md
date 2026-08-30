# mojxml-rs

法務省登記所備付地図データ（地図XML）を高速でGISデータ形式（現在は GeoParquet, FlatGeobuf, GeoJSON を対応しています）に変換するコマンドラインツールです。

このツールは Rust で書いていますが、 [`mojxml-py`](https://github.com/ciscorn/mojxml-py) や[デジタル庁が提供している `mojxml2geojson`](https://github.com/digital-go-jp/mojxml2geojson) ツールを参考に作成しています。

## このツールの特徴

* 効率的に利用可能のプロセッサーをすべて並列で使うことができる。
* 全国規模の変換性能を、同一データ・複数回実行で比較できるベンチマークを用意
* zip内のzipアーカイブを自動で解凍する
* 複数入力ファイルが統合されて一つの出力ファイルになります
* 代表点座標を計算する
* GeoParquet, FlatGeobuf, GeoJSON (newline-delimited) の出力を対応
* Windows, Linux, macOS それぞれの OS で実行できるバイナリとして提供しています

## 性能

MacBook Pro M5 で全国2026年データを一つの出力ファイルに変換した結果です。

| 出力形式 | 変換時間 | ピークメモリ使用量 |
| --- | ---: | ---: |
| GeoParquet | 13分 | 2.5 GB |
| GeoJSON | 12分 | 2 GB |
| FlatGeobuf | 19分 | 12 GB |

## インストール方法

このツールは Rust で書かれていますが、コンパイル済みバイナリも下記のアーキテクチャで用意しています。

* macOS (Apple Silicon)
* Windows (x86_64)
* Linux (x86_64)

[最新の Release](https://github.com/KotobaMedia/mojxml-rs/releases/latest) から利用環境の zip アーカイブをダウンロードしていただき、解凍したらコマンドラインで実行できます。お困りの方は [GitHub Issues](https://github.com/KotobaMedia/mojxml-rs/issues) で詳細を教えて下さい。

Rust 環境のお持ちの方は、 `cargo install mojxml-rs` でインストールできます。

## 使い方

```
Usage: mojxml-rs [OPTIONS] <DST_FILE> <SRC_FILES>...

Arguments:
  <DST_FILE>      Output file path
  <SRC_FILES>...  Input MOJ XML file paths (.xml or .zip)

Options:
  -a, --arbitrary            Include features from arbitrary coordinate systems (unmapped files) ("任意座標系")
  -A, --only-arbitrary       Include only features from arbitrary coordinate systems ("任意座標系"). This ignores features from globally mapped coordinate systems
  -c, --chikugai             Include features marked as outside district ("地区外") or separate map ("別図"). You probably don't need this
  -v, --verbose              Enable logging. Will log to mojxml.log in the current directory
  -t, --temp-dir <TEMP_DIR>  Optional temporary directory for unzipping files. If not specified, the default temporary directory will be used. Use this option if your /tmp directory doesn't have enough space
      --fgb-no-index          Disable FlatGeobuf spatial index generation. Has effect only when output extension is `.fgb`
      --metrics-json <FILE>   Write machine-readable processing metrics to a JSON file
  -h, --help                 Print help
  -V, --version              Print version
```

例:

```
mojxml-rs ./moj-2025-46.parquet ../dl-tool/zips/46*.zip
```

上記のコマンドは、 `dl-tool` でダウンロードした鹿児島県のすべてのzipファイルを、 `moj-2025-46.parquet` の GeoParquet にまとめて変換します。

> [!TIP]
> Linux のディストリビューションによって `/tmp` ディレクトリは tmpfs (メモリ上のファイルシステム) になっている。 `mojxml-rs` は親ZIPを解凍するときはテンポラリファイルを使うため、メモリをひっ迫する可能性があります。これを防ぐために、 `-t` オプションでディスク上のテンポラリディレクトリを指定してください。

## プログレスバーの説明

```
[unzipping] 00:04:20 #######---------------------------------     309/2006
[XML parse] 00:04:20 ########################################   25160/25201
[  output ] 00:04:20 ########################################   25159/25160
```

* `unzipping` は入力ZIPファイルを指します。この場合、全部2006個の内309個目は解凍完了。解凍は基本的に1スレッドで行います。（解凍が次のステップより速かったらメモリが圧迫されてしまうため）
* `XML parse` は解凍されたXMLをメモリ上に読み込まれ、必要な情報の抽出やGISデータの変換を指します。親ZIPの数がわかっても、その中のZIPの数は事前にわからないので、解凍が進むと母数値が増えます。
* `output` は GeoParquet の書き込みを指します。

より詳細なログがほしい場合は `--verbose` で実行すると `mojxml.log` ファイルに個別ファイルの読み込み・書き込み状況をログ形式で出力します。

## ベンチマーク

以前記載していた「Ryzen 9 9950X 16C/32T、96GB RAMの環境で、最大約20GBを使用し、全国2025年度データを約15分で変換」という値は、単発の非公式な実行結果でした。再現可能な性能値を得るため、現在はdataset fingerprint、ウォームアップ、5回の測定、peak RSS、出力検証、ばらつき判定を含むベンチマークrunnerを用意しています。

WSL2とmacOSでの測定条件、実行コマンド、結果JSONの比較方法は[ベンチマーク手順](benchmarks/README.md)を参照してください。全国データを使わない短い動作確認も可能ですが、公開する性能値には標準の1 warm-up + 5 measured runsを使用してください。

## 開発

[pre-commit](https://pre-commit.com/) でフォーマットやリントなどを実行します。CIエラーを防ぐために導入してください。

## ライセンス

このツールのソースコードは MIT ライセンスで公開しています。
