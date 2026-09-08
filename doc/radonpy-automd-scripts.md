# RadonPyのAutoMD_scripts 各スクリプトの入出力

対象: `AutoMD_scripts/`(RadonPy本体のGitHubリポジトリに同梱されているサンプルスクリプト群)。
このリポジトリ自体には含まれないが、実機で1〜6を実際に動かして得られた知見(バグ・
制約含む)をまとめたもの。

## 入出力の一覧

| # | スクリプト | 主な入力 | 何を求めているか(出力) |
|---|---|---|---|
| 1 | `1_eq.py` | SMILES(`RadonPy_SMILES`)、DBID、鎖長・鎖数(`RadonPy_NAtom`/`RadonPy_NChain`)、初期密度、温度・圧力 | ポリマーの重合・アモルファス構造生成→平衡化MD(eq1〜eq3、デフォルト設定で合計858万ステップ)。**密度・回転半径(Rg)・熱膨張率・等温圧縮率などの基本物性**、および後続計算の可否を示す`check_eq`(平衡化の収束)・`do_TC`(熱伝導率計算の可否)フラグ |
| 2 | `2_rst_eq.py` | 1の出力(`results.csv`+構造データ、同じDBID)、リトライ回数(`RadonPy_RetryEQ`、デフォルト2) | 1が収束しなかった場合の**追加平衡化**。前回の続きの段階番号(eq4, eq5, ...)から積み増す(eq1・eq2は再計算されない)。1と同じ物性値・フラグを再計算・更新 |
| 3 | `3_tc.py` | 1/2で平衡化済みの構造(`do_TC=True`が必要、または`RadonPy_TC_Force=True`) | 非平衡MD(NEMD、Müller-Plathe法)による**熱伝導率**(thermal_conductivity)、およびその内訳(結合・角度・vdW等の寄与分解) |
| 4 | `4_tg.py` | 1/2で平衡化済みの構造(`check_eq=True`が必要、または`RadonPy_Tg_Force=True`) | 高温から低温(デフォルト50K)まで10K刻みで段階的に冷却しながら密度変化を追跡し、**ガラス転移温度(Tg)**を算出 |
| 5 | `5_sp.py` | 1/2で平衡化済みの構造(`check_eq=True`が必要、または`RadonPy_SP_Force=True`)、LAMMPSにTALLYパッケージが必要 | 既存トラジェクトリを再解析(rerun)し、分子間相互作用エネルギーから**溶解度パラメータ(SP)** を算出 |
| 6 | `6_ef_dp.py` | 1/2で平衡化済みの構造(`check_eq=True`が必要、または`RadonPy_EFDP_Force=True`)、印加する電場の周波数・強度(`RadonPy_EFDP_Freq`等) | 交流電場を印加したMDから、指定周波数における**動的誘電率・誘電損失・損失正接(tanδ)** を算出 |

補足: 1(または1+2)が全ての土台で、そこで作った平衡化済み構造を3・4・5・6がそれぞれ
別の切り口で再利用する構成。3・4・6は各自の`check_eq`/`do_TC`判定がFalseだと、デフォルトでは
実行が拒否される(`RadonPy_*_Force=True`で強制続行は可能だが、その場合出てくる物性値は
信頼できない)。

## 実機で遭遇した不具合・制約

### 3_tc.py: LAMMPSの`if`構文エラー

`radonpy/sim/preset/tc.py`(`NEMD_MP`クラス)が生成するLAMMPS入力に、以下のような
文字列比較の`if`文が含まれる。

```
if "${dumpf} != None" then &
    "dump            1 all custom 1000 ${dumpf} id type mol xs ys zs ix iy iz"
if "${xtcf} != None" then &
    "dump            2 all xtc 1000 ${xtcf}" &
    "dump_modify     2 unwrap yes"
```

`dumpf`/`xtcf`が実際のファイル名(`None`という文字列ではない)であっても、
`error: invalid boolean syntax in if command (src/variable.cpp)` になることを実機で確認した。
LAMMPSの`if`コマンドの構文解釈が、この文字列比較の書き方と噛み合っていないと考えられる。

対処: `if`/`then`の条件分岐を外し、素の(引用符なしの)LAMMPSコマンドに書き換える。

```python
# 修正後(条件分岐を外して常に実行)
dump            1 all custom 1000 ${dumpf} id type mol xs ys zs ix iy iz
dump            2 all xtc 1000 ${xtcf}
dump_modify     2 unwrap yes
```

`dump 2`と`dump_modify 2`は必ずセットで扱うこと(`dump_modify`は対応する`dump`が
無いとエラーになる)。同じ壊れたパターンは`tc.py`内に他3箇所(`NEMD_MP_Additional`,
`NEMD_Langevin`, `EMD_GK`)にもあるが、`3_tc.py`が実際に使うのは`NEMD_MP`のみ。

修正箇所は`/opt/RadonPy/radonpy/sim/preset/tc.py`と、実際にimportされる
`site-packages`側の両方(CPU版・GPU版でDockerイメージが別なら、両方のコンテナで
それぞれ)に反映する必要がある。

### 5_sp.py: TALLYパッケージがconda-forge版LAMMPSに無い

```
error: unrecognized compute style 'pe/mol/tally' is part of the TALLY package
which is not enabled in this LAMMPS binary
```

conda-forgeの`lammps-feedstock`の`recipe/build.sh`を確認したところ、`PKG_TALLY`への
言及が一切無い(=常にOFF)。溶解度パラメータの計算(`compute pe/mol/tally`で
分子間相互作用エネルギーだけを取り出す)には、このパッケージが構造的に必須なため、
conda-forge版LAMMPSでは`5_sp.py`は原理的に実行できない。回避するにはLAMMPSを
`-D PKG_TALLY=ON`でソースから自前ビルドする必要がある。

`5_sp.py`は内部で`mpi=16`に固定されている(rerun計算の都合上16・64・128のいずれかが
必要とコメントあり)。MPI数はTALLY不足とは無関係で、変更しても直らない。

### 6_ef_dp.py: 環境変数に計算式をそのまま渡すとエラー

```python
freq = float(os.environ.get('RadonPy_EFDP_Freq', 10.0*1e+9))    # unit: Hz
```

デフォルト値`10.0*1e+9`はPythonが読み込み時に計算するので問題ないが、
`RadonPy_EFDP_Freq`環境変数に`"50.*1e+9"`のような**計算式を文字列としてそのまま**
設定すると、`float("50.*1e+9")`が`*`を含む文字列を解釈できず
`could not convert string to float`になる。環境変数には計算済みの最終的な数値
(例: `"5e10"`)を渡す必要がある。
