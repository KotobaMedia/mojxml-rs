# mojxml-rs

法務省登記所備付地図データ（地図XML）を高速でGISデータ形式（現在は FlatGeobuf を対応しています）に変換するコマンドラインツールです。

このツールは Rust で書いていますが、 [`mojxml-py`](https://github.com/ciscorn/mojxml-py) や[デジタル庁が提供している `mojxml2geojson`](https://github.com/digital-go-jp/mojxml2geojson) ツールを参考に作成しています。

