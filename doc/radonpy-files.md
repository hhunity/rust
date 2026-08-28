
# RadonPy (git) を使うのに必要なファイル

RadonPy (https://github.com/RadonPy/RadonPy) はポリマー物性の全自動計算を行う
Python製ライブラリ。本体はGit/PyPIで配布されるが、実行には外部の分子動力学(MD)・
量子化学計算(QM)エンジンが別途必要になる。ここではそれらをまとめる。

## 1. RadonPy本体 (git)

```
git clone https://github.com/RadonPy/RadonPy.git
```

オフライン環境へは `.git` ごとコピーするか、`pip download radonpy-pypi` で
取得したwheel/sdistを転送する。

対応Pythonバージョン: 3.9 〜 3.13 (バージョンによって上限が変わるためREADME要確認)。
ライセンスはBSD-3。

## 2. 必須の外部ツール・依存パッケージ

conda (`conda-forge` / `psi4` チャンネル) 経由でのインストールが推奨。

| パッケージ | 用途 |
|---|---|
| rdkit (>=2020.03) | 化学構造処理・ポリマー鎖生成 |
| psi4 (>=1.5) | 量子化学計算(QM、構造最適化・電荷計算) |
| resp | RESP電荷計算 |
| dftd3-python | 分散力補正(DFT-D3) |
| lammps (>=2020.03.03) | 分子動力学(MD)計算本体 |
| mdtraj (>=1.9) | MD軌跡の解析 |
| numpy / scipy | 数値計算 |
| pandas | データ処理 |
| matplotlib | 可視化 |
| psutil | プロセス/リソース監視 |

これはRadonPyリポジトリ同梱の `requirements.txt` の内容と対応:
```
pandas
numpy
scipy
psutil
matplotlib
rdkit>=2020.03
mdtraj>=1.9
lammps>=2020.03.03
```

### オプション(生体高分子: ペプチド・多糖類を扱う場合のみ)

- ambertools
- intermol

## 3. Conda環境ファイル (リポジトリ同梱: yaml/rnpy37.yml, rnpy38.yml, rnpy39.yml)

RadonPyリポジトリの `yaml/` ディレクトリに、Pythonバージョンごとの
Conda環境定義ファイルが同梱されている。チャンネルは `psi4`, `conda-forge`, `defaults`。
これをそのまま使えば依存関係を一括solveできる:
```
conda env create -f yaml/rnpy39.yml
```

## 4. インストール手順(ネットに繋がる環境)

```
conda create -n radonpy python=3.11 -c conda-forge
conda activate radonpy
conda install -c conda-forge -c psi4 rdkit psi4 dftd3-python resp mdtraj psutil scipy pandas matplotlib pip
conda install -c conda-forge lammps
pip install radonpy-pypi
```

LAMMPSを含めてPyPIだけで揃えたい場合(Psi4がインストールできない環境向けの制限構成):
```
pip install radonpy-pypi[lammps]
```

## 5. オフライン環境へ持っていく方法

condaパッケージはaptの `.deb` と違い単純なURL一覧ダウンロードでは揃わないため、
以下のいずれかを使う。

- **conda-pack** (推奨): オンライン環境で作った環境を丸ごと固めてコピー
  ```
  conda install -c conda-forge conda-pack
  conda-pack -n radonpy -o radonpy_env.tar.gz
  ```
  オフライン環境側では展開して `conda-unpack` を実行するだけで使える。

- **conda create --download-only**: パッケージ本体のみキャッシュ (`pkgs/`) に
  落として、そのディレクトリごとオフライン環境へ転送し、
  `conda install --offline` でインストールする。

- **RadonPy本体(pip)側**は
  `pip download radonpy-pypi -d ./radonpy_wheels` でwheelを集め、
  オフライン側で
  `pip install --no-index --find-links=./radonpy_wheels radonpy-pypi` する
  (`doc/rust-urls.txt` のvendor手法と同じ考え方)。

## 6. まとめ(オフライン転送時に必要な4点セット)

USBメモリ等でまとめてコピーする対象:
- RadonPy本体の `.git` (または wheel/sdist)
- `yaml/rnpyXX.yml` (使うPythonバージョンのもの)
- `conda-pack` で固めた環境 tarball (`radonpy_env.tar.gz`)、
  もしくは `conda create --download-only` の `pkgs/` 一式
- RadonPy本体のwheel (`pip download` した分、conda環境にRadonPy自体が
  含まれない場合)
