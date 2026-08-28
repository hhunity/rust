# RadonPy とは何か

## 1. RadonPy の位置づけ

RadonPy は **CNN でも GNN でもない**。深層学習モデルではなく、高分子材料の物性を
オールアトム古典分子動力学(MD)シミュレーションによって自動計算する
オープンソースの Python ライブラリである。

- 入力: 化学構造(SMILES など)
- 出力: 密度・屈折率・ガラス転移温度・比熱・熱伝導率などの物性値
- 処理: 量子化学計算(電荷計算)+ MD シミュレーションのパイプラインを完全自動化

RadonPy 自体は「学習するパラメータ(重み)」を持たず、物理シミュレーションで
物性を**計算**しているだけであり、過去データから**予測**しているわけではない。

## 2. 使われている主な計算エンジン

| ツール | 役割 |
|---|---|
| **RDKit** | SMILES から分子の立体構造を組み立てる(化学構造の入出力) |
| **Psi4** | 量子化学計算(DFT)。原子の部分電荷などを精密に計算 |
| **GAFF2** | 力場(原子同士の相互作用ルール)を割り当てる |
| **LAMMPS** | 分子動力学(MD)シミュレーションの実行エンジン |

## 3. 入力パラメータ

線状ポリマーの非晶質状態を計算する場合、必要な入力は以下の5つ:

1. **SMILES 文字列** — 繰り返し単位(モノマー)の化学構造
2. **重合度** — 繰り返し単位が何個つながるか
3. **ポリマー鎖の本数** — シミュレーションセル内に入れる鎖の数
4. **温度**
5. **圧力**

## 4. 自動計算ワークフロー

1. モノマーの配座探索
2. DFT による電子的性質(原子電荷など)の計算
3. 自己回避ランダムウォークによるポリマー鎖の初期配置探索
4. GAFF2 による力場パラメータの割り当て
5. 等方的な非晶質セルの生成
6. 平衡化のための MD シミュレーション
7. 平衡状態への到達判定
8. 熱伝導率計算のための非平衡 MD(NEMD)シミュレーション
9. 後処理段階での物性計算

すべて自動で流れるため、実験室での合成・測定なしに大量の候補ポリマーの
物性を計算で予測できる。

## 5. RadonPy と機械学習の関係

RadonPy 自体は ML モデルではないが、**RadonPy が生成した大量の計算データ**は
別のニューラルネットワークの**事前学習(pretraining)**に使われることがある
(例: `docs/CECAM_tutorial` 内の `transfer_learning`)。

### Transfer Learning(Sim2Real 転移学習)の流れ

1. RadonPy で数万件規模の高分子について物性を MD 計算する(シミュレーションデータ)
2. そのシミュレーションデータでニューラルネットワークを**事前学習**する
   - 外部の学習済みモデル(ImageNet 等)は使わない
   - あくまで **RadonPy 自身が生成した計算データ** が事前学習の元データ
3. 少数の実験データ(例: PoLyInfo データベース)でファインチューニングする
4. 実験データのみで学習するより高い予測精度を得る

### 使われているモデルは CNN ではない

- 記述子: 化学構造を **Morgan フィンガープリント(ECFP)** で
  固定長(例: 190次元)の数値ベクトルに変換
- モデル: **通常の全結合多層ニューラルネットワーク(MLP)**
- 画像のような空間的近傍関係を持つデータではないため、CNN ではなく
  MLP がそのまま使われている

## 6. まとめ図

```
[SMILES + 重合度 + 鎖数 + 温度 + 圧力]
        │
        ▼
   RadonPy (RDKit + Psi4 + GAFF2 + LAMMPS)
        │  ← MDシミュレーションによる物性計算(学習なし)
        ▼
  数万件規模の物性データセット(シミュレーションデータ)
        │
        ▼
  Morganフィンガープリント化 → 全結合NN (MLP) で事前学習
        │
        ▼
  少数の実験データでファインチューニング(転移学習)
        │
        ▼
     実世界での物性予測モデル
```

## 参考文献

- Hayashi, Y., Shiomi, J., Morikawa, J., Yoshida, R.
  "RadonPy: automated physical property calculation using all-atom classical
  molecular dynamics simulations for polymer informatics."
  *npj Computational Materials* 8, 222 (2022).
- Minami, S., Hayashi, Y. et al. "Scaling Law of Sim2Real Transfer Learning
  in Expanding Computational Materials Databases for Real-World Predictions."
  arXiv:2408.04042
- RadonPy GitHub: https://github.com/RadonPy/RadonPy
