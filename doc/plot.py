"""dump_corr が出した相関面の CSV を、ヒートマップ・3D グラフ・断面のグラフにする

使い方：
  python plot_corr.py corr.csv                 # 画面に表示して、corr.png も保存
  python plot_corr.py corr.csv --no-show       # 保存だけ
  python plot_corr.py a.csv b.csv              # 2つを並べて比べる（例：通常 と 位相限定）
"""
import argparse
from pathlib import Path

import matplotlib.pyplot as plt
import numpy as np


def load(path):
    """グリッド形式の CSV を読む → (dx の軸, dy の軸, 値の行列, UpDFT のずれ (sx, sy), 説明)"""
    lines = Path(path).read_text(encoding="utf-8").splitlines()
    info = {}
    for ln in lines:
        if ln.startswith("#"):
            for kv in ln[1:].strip().split(","):
                k, _, v = kv.partition("=")
                info[k.strip()] = v.strip()
    rows = [ln.split(",") for ln in lines if ln and not ln.startswith("#")]
    xs = np.array(rows[0][1:], float)
    ys = np.array([r[0] for r in rows[1:]], float)
    z = np.array([r[1:] for r in rows[1:]], float)
    shift = (float(info.get("shift_x", "nan")), float(info.get("shift_y", "nan")))
    label = f"{Path(path).stem}（{info.get('method', '')}, σ={info.get('sigma', '')}）"
    return xs, ys, z, shift, label


def plot_one(fig, col, ncol, xs, ys, z, shift, label):
    X, Y = np.meshgrid(xs, ys)
    j, l = np.unravel_index(np.argmax(z), z.shape)

    # 1. ヒートマップ（上から見た図）
    ax = fig.add_subplot(3, ncol, col)
    im = ax.imshow(z, origin="lower", extent=[xs[0], xs[-1], ys[0], ys[-1]], aspect="equal", cmap="viridis")
    ax.plot(shift[0], shift[1], "r+", ms=12, mew=2, label=f"推定 ({shift[0]:.3f}, {shift[1]:.3f})")
    ax.set_xlabel("dx [画素]"); ax.set_ylabel("dy [画素]"); ax.set_title(label, fontsize=9)
    ax.legend(fontsize=8, loc="upper right"); fig.colorbar(im, ax=ax, shrink=0.8)

    # 2. 3D グラフ（ドラッグで回せる）
    ax3 = fig.add_subplot(3, ncol, ncol + col, projection="3d")
    ax3.plot_surface(X, Y, z, cmap="viridis", linewidth=0, antialiased=True, rstride=1, cstride=1)
    ax3.set_xlabel("dx"); ax3.set_ylabel("dy"); ax3.set_zlabel("相関")
    ax3.set_title(f"最大 {z.max():.3f}", fontsize=9)

    # 3. 頂上を通る断面（x 方向・y 方向）
    ax2 = fig.add_subplot(3, ncol, 2 * ncol + col)
    ax2.plot(xs - xs[l], z[j, :], label="dx 方向の断面")
    ax2.plot(ys - ys[j], z[:, l], "--", label="dy 方向の断面")
    ax2.set_xlabel("頂上からの距離 [画素]"); ax2.set_ylabel("相関"); ax2.grid(alpha=0.3); ax2.legend(fontsize=8)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv", nargs="+")
    ap.add_argument("--out", default=None, help="保存する画像（既定：1つ目の CSV と同じ名前の .png）")
    ap.add_argument("--no-show", action="store_true")
    a = ap.parse_args()

    plt.rcParams["font.family"] = ["Yu Gothic", "MS Gothic", "Hiragino Sans", "Noto Sans CJK JP", "IPAexGothic", "sans-serif"]
    n = len(a.csv)
    fig = plt.figure(figsize=(5.5 * n, 14))
    for i, p in enumerate(a.csv, start=1):
        plot_one(fig, i, n, *load(p))
    fig.tight_layout()
    out = a.out or str(Path(a.csv[0]).with_suffix(".png"))
    fig.savefig(out, dpi=120)
    print(f"保存：{out}")
    if not a.no_show:
        plt.show()


if __name__ == "__main__":
    main()
