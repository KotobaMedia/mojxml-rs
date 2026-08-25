# 全国データ変換ベンチマーク

このベンチマークは、同一の法務省地図XMLデータ一式を一つの出力ファイルへ
変換する end-to-end 性能を、Linux（WSL2を含む）と macOS
で同じ手順により測定するものです。単発の実行時間ではなく、ウォームアップ1回と
測定5回を標準とし、中央値、平均値の95%信頼区間、標準偏差、変動係数（CV）、
peak RSS を記録します。

## 測定対象

測定時間には、入力ZIPの読み込み・展開、XMLの解析、座標変換、指定形式の
生成・flushを含みます。次の処理は測定時間に含めません。

- release binary のビルド
- 入力ファイルの SHA-256 fingerprint の作成
- 実行結果JSONの集計

入力ファイルはパス順に固定されます。各ファイルの相対パス、サイズ、SHA-256から
dataset fingerprint を作るため、別のマシンで同一データを使ったか確認できます。
各回は新しい出力ファイルを使い、正常終了、入力read/XML parse/write error が0件であること、
全回のXML数・feature数が一致すること、および選択した形式に応じて出力を検証します。
GeoParquetではheader/footer magic、FlatGeobufではmagic bytesとheader、newline-delimited
GeoJSONでは先頭・末尾のfeatureと改行終端を確認します。出力本体は標準では検証後に削除します。

標準の結果は「入力を一度最後まで読み込んだ後の steady-state（warm-cache）性能」です。
OSのページキャッシュを強制削除する操作はOSごとに権限と意味が異なるため行いません。
ウォームアップは集計から除外されます。測定5回の wall time の CV が3%を超える場合、
runner は結果を保存した上で exit code 2 を返し、公開値として採用しないよう通知します。

## 測定前の条件

両環境で次を揃えてください。

- 同じ clean git commit、`Cargo.lock`、Rust compiler version を使う
- `RUSTFLAGS` と `CARGO_BUILD_TARGET` を未設定にし、標準の release profile を使う
- AC電源に接続し、省電力モードを無効にする
- OS update、バックアップ、indexingなどの負荷がない状態にする
- 入力、作業ディレクトリともローカルSSDに置き、十分な空き容量を確保する
- 同じ mojxml-rs option を使う

WSL2 では入力と作業ディレクトリを Linux filesystem（例: `~/data`）に置き、
`/mnt/c` は使わないでください。Windows側の `.wslconfig` でCPU数・メモリ量を固定し、
変更後は PowerShell から `wsl --shutdown` を実行します。割り当てられた論理CPU数と
メモリ量は結果JSONにも記録されます。

macOS では入力と作業ディレクトリをローカル APFS volume に置き、iCloud Driveや
network/external volumeを避けてください。MacBookではAC電源に接続し、低電力モードを
無効にします。

peak RSS はOSが対象プロセスに報告する最大resident set sizeです。WSL VM全体の消費量や
macOS/Linuxそれぞれのファイルキャッシュは含まれないため、異なるOS間では参考値として
扱ってください。

## 実行方法

Pythonの外部packageは不要です。データが1ディレクトリにある場合は、リポジトリrootで
次を実行します。`--work-dir` にはリポジトリ外のローカルSSD上の場所を推奨します。

```bash
unset RUSTFLAGS CARGO_BUILD_TARGET
python3 benchmarks/run.py \
  --input-dir "$HOME/data/moj-2025" \
  --pattern '*.zip' \
  --work-dir "$HOME/benchmarks/mojxml-rs" \
  --output-format fgb \
  --label ryzen-9950x-wsl2
```

`--output-format` は `fgb`（標準）、`geoparquet`、`geojson` から選びます。`fgb` は標準で
空間indexを生成します。indexなしを測定するときは `--cli-arg=--fgb-no-index` も指定します。

macOSも同じコマンドを使い、識別しやすいlabelを指定します。

```bash
python3 benchmarks/run.py \
  --input-dir "$HOME/data/moj-2025" \
  --pattern '*.zip' \
  --work-dir "$HOME/benchmarks/mojxml-rs" \
  --label macbook-pro-m4-max
```

ZIPが下位ディレクトリにもある場合は `--pattern '**/*.zip'` を使います。変換optionを
追加するときは、たとえば `--cli-arg=--arbitrary` とします。標準の1+5回は全国データで
長時間を要します。動作確認に限り `--warmups 0 --runs 2` へ減らせますが、その値を
性能値として公開しないでください。

各測定の標準出力・標準エラー、CLI内部stage metrics、host/toolchain情報、個別run、
統計値は新規ディレクトリの `result.json` に保存されます。途中で失敗した場合も、完了済み
runは `result.partial.json` に残ります。入力の詳細は `dataset-manifest.json` に保存されます。

## 環境間の比較

2台から `result.json` を集め、先頭を相対速度1.00倍のbaselineとして比較します。

```bash
python3 benchmarks/compare.py \
  /path/to/ryzen-9950x-wsl2/result.json \
  /path/to/macos/result.json
```

dataset fingerprint、変換option、cache policy、出力件数、git commit、binary versionが
一致しない結果は標準では比較できません。出力は README や issue に貼れる Markdown
tableです。比較結果とともに両方の `result.json` および `dataset-manifest.json` を保存して
おくと、後から測定条件を監査できます。

コード変更前後を意図的に比較する場合のみ `--allow-code-mismatch` を指定できます。
異なる環境の比較では、cleanな同一commitを使うのが標準protocolです。
