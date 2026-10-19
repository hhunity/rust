"""実験の実行スクリプト（run_exe.py、run_experiment.py）で共通に使う処理

- 画像フォルダの探索、設定の展開（sweep）
- 結果フォルダの作成、README・実行情報の書き出し
- 集計（2階差分法、波の振幅、画像間の繰り返し）とグラフ
"""
from __future__ import annotations

import datetime as dt
import fnmatch
import itertools
import platform
import subprocess
import sys
from pathlib import Path

import numpy as np
import pandas as pd
import yaml


def find_image_folders(cfg: dict) -> list[tuple[str, list[Path]]]:
    """(image_root からの相対パス, そのフォルダの画像リスト) の一覧を返す。"""
    root = Path(cfg["paths"]["image_root"])
    icfg = cfg["images"]
    patterns = icfg.get("patterns", ["*.png", "*.tif", "*.tiff", "*.bmp"])
    exclude = icfg.get("exclude", []) or []

    def images_in(d: Path) -> list[Path]:
        files = [p for p in d.iterdir() if p.is_file() and any(fnmatch.fnmatch(p.name.lower(), pt.lower()) for pt in patterns)]
        return sorted(files)

    candidates: set[Path] = set()
    for pat in icfg.get("folders", ["*"]):
        for d in root.glob(pat):
            if not d.is_dir():
                continue
            if icfg.get("recursive", False):
                candidates.update(p for p in [d, *d.rglob("*")] if p.is_dir())
            else:
                candidates.add(d)
    if not candidates and images_in(root):
        candidates.add(root)   # image_root 自体に画像が入っている場合

    out = []
    for d in sorted(candidates):
        rel = d.relative_to(root).as_posix() or "."
        if any(fnmatch.fnmatch(rel, ex) or fnmatch.fnmatch(d.name, ex) for ex in exclude):
            continue
        imgs = images_in(d)
        if imgs:
            out.append((rel, imgs))
    return out


def read_image(path: Path) -> np.ndarray:
    import cv2
    # 日本語パスでも読めるように np.fromfile 経由で読む。8/16bit、1チャンネルとして読む
    data = np.fromfile(str(path), dtype=np.uint8)
    img = cv2.imdecode(data, cv2.IMREAD_ANYDEPTH | cv2.IMREAD_GRAYSCALE)
    if img is None:
        raise IOError(f"画像を読めません: {path}")
    return img


def expand_methods(cfg: dict) -> list[dict]:
    out = []
    for m in cfg["methods"]:
        base = dict(m.get("params", {}) or {})
        sweep = m.get("sweep", {}) or {}
        keys = list(sweep)
        for values in itertools.product(*[sweep[k] for k in keys]) if keys else [()]:
            params = dict(base, **dict(zip(keys, values)))
            label = m.get("label", m["name"]).format(**params)
            out.append({"name": m["name"], "label": label, "params": params})
    return out


def robust_sigma_2nd(v: np.ndarray) -> float:
    """2階差分法（外れ値に強い版）：1.4826 × 中央値(|Δ²v|) ÷ √6"""
    v = v[np.isfinite(v)]
    if len(v) < 5:
        return np.nan
    return 1.4826 * np.median(np.abs(np.diff(v, 2))) / np.sqrt(6)


def wave_amplitude(y: np.ndarray, d: np.ndarray) -> float:
    """直線を引いた残りを、ずれの小数部に対する正弦波で当てはめた振幅（ずれが 1 画素以上変わる範囲があるときだけ）"""
    ok = np.isfinite(d)
    if ok.sum() < 8:
        return np.nan
    y, d = y[ok], d[ok]
    a, b = np.polyfit(y, d, 1)
    fit = a * y + b
    if fit.max() - fit.min() < 1.0:
        return np.nan
    ph = 2 * np.pi * (fit - np.floor(fit))
    A = np.column_stack([np.sin(ph), np.cos(ph), np.ones_like(ph)])
    coef, *_ = np.linalg.lstsq(A, d - fit, rcond=None)
    return float(np.hypot(coef[0], coef[1]))


def stats_for(df: pd.DataFrame, fail_score) -> pd.DataFrame:
    rows = []
    for (img, label), g in df.groupby(["image", "label"], sort=False):
        g = g.sort_values(["block_x", "block_y"])
        ok = np.isfinite(g["dx"]) & np.isfinite(g["dy"])
        if fail_score is not None:
            ok &= g["score"] >= fail_score
        gg = g[ok]
        row = {"image": img, "label": label, "method": g["method"].iloc[0],
               "n_blocks": len(g), "n_fail": int((~ok).sum())}
        if len(gg) >= 2:
            y = gg["block_y"].to_numpy(float)
            for ax in ("dx", "dy"):
                v = gg[ax].to_numpy(float)
                a, b = np.polyfit(y, v, 1)
                row[f"mean_{ax}"] = v.mean()
                row[f"slope_{ax}_per_row"] = a
                row[f"resid_std_{ax}"] = np.std(v - (a * y + b))
                row[f"sigma2nd_{ax}"] = robust_sigma_2nd(v)
                row[f"wave_amp_{ax}"] = wave_amplitude(y, v)
        row["median_score"] = float(np.nanmedian(gg["score"])) if len(gg) else np.nan
        rows.append(row)
    st = pd.DataFrame(rows)

    # フォルダ内の画像間のばらつき（同じ条件の繰り返しなら、そのまま繰り返し性）
    if df["image"].nunique() >= 2:
        rep = []
        for label, g in df.groupby("label", sort=False):
            p = g.pivot_table(index=["block_x", "block_y"], columns="image", values=["dx", "dy"])
            rep.append({"image": "(画像間の繰り返し)", "label": label, "method": g["method"].iloc[0],
                        "repeat_std_dx": float(np.nanmedian(p["dx"].std(axis=1))),
                        "repeat_std_dy": float(np.nanmedian(p["dy"].std(axis=1)))})
        st = pd.concat([st, pd.DataFrame(rep)], ignore_index=True)
    return st


def save_plots(df: pd.DataFrame, figdir: Path):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib import font_manager
    have = {f.name for f in font_manager.fontManager.ttflist}
    jp = [n for n in ("Yu Gothic", "Meiryo", "MS Gothic", "IPAexGothic", "Noto Sans CJK JP", "Hiragino Sans") if n in have]
    plt.rcParams["font.family"] = jp[:1] + ["DejaVu Sans"]   # 日本語フォントがなければ英字フォント（日本語は□になる）
    figdir.mkdir(parents=True, exist_ok=True)
    first = df["image"].iloc[0]
    d0 = df[df["image"] == first]
    for ax_name in ("dx", "dy"):
        fig, axs = plt.subplots(2, 1, figsize=(9, 7), sharex=True)
        for label, g in d0.groupby("label", sort=False):
            g = g.sort_values("block_y")
            y, v = g["block_y"].to_numpy(float), g[ax_name].to_numpy(float)
            ok = np.isfinite(v)
            axs[0].plot(y[ok], v[ok], ".-", label=label)
            if ok.sum() >= 2:
                a, b = np.polyfit(y[ok], v[ok], 1)
                axs[1].plot(y[ok], v[ok] - (a * y[ok] + b), ".-", label=label)
        axs[0].set_ylabel(f"{ax_name} [画素]")
        axs[1].set_ylabel(f"{ax_name} − 直線 [画素]")
        axs[1].set_xlabel("ブロックの y [行]")
        axs[0].set_title(f"{first}")
        axs[0].legend(fontsize=8)
        for a in axs:
            a.grid(alpha=0.3)
        fig.tight_layout()
        fig.savefig(figdir / f"{ax_name}_vs_row.png", dpi=120)
        plt.close(fig)


def make_experiment_dir(cfg: dict, out: str | None) -> Path:
    if out:
        p = Path(out)
    else:
        today = dt.date.today().isoformat()
        base = Path(cfg["paths"]["work_dir"]) / f"{today}_{cfg['experiment']['name']}"
        p, i = base, 2
        while p.exists():
            p = base.with_name(f"{base.name}_run{i}")
            i += 1
    (p / "results").mkdir(parents=True, exist_ok=True)
    return p


def code_version() -> str:
    here = Path(__file__).resolve().parent
    try:
        rev = subprocess.run(["git", "-C", str(here), "rev-parse", "--short", "HEAD"],
                             capture_output=True, text=True, timeout=5).stdout.strip()
        dirty = subprocess.run(["git", "-C", str(here), "status", "--porcelain"],
                               capture_output=True, text=True, timeout=5).stdout.strip()
        return (rev + (" (未コミットの変更あり)" if dirty else "")) if rev else "git 管理外"
    except Exception:
        return "git 管理外"


def write_readme_template(exp: Path, cfg: dict):
    p = exp / "README.md"
    if p.exists():
        return
    p.write_text(f"""# {cfg['experiment']['name']}

## 目的
{cfg['experiment'].get('purpose', '').strip()}

## 条件
- 撮影条件：（F値、ピント、露光、ゲイン、照明、ラインレート、搬送速度、温度など。画像ごとに違う場合は results/<フォルダ>/conditions.csv）
- 実行設定：config.yaml、実行情報：run_info.md

## 結果
- 集計：results/summary.csv
- （気づいたこと、グラフへのリンク）

## 結論
-
""", encoding="utf-8")


def write_run_info(exp: Path, cfg: dict, folders, args):
    lines = [
        "# 実行情報（自動生成、毎回上書き）",
        f"- 実行日時：{dt.datetime.now().isoformat(timespec='seconds')}",
        f"- コマンド：{' '.join(sys.argv)}",
        f"- コードの版：{code_version()}",
        f"- Python {platform.python_version()} / numpy {np.__version__} / pandas {pd.__version__}",
        f"- 画像の場所：{cfg['paths']['image_root']}",
        f"- 対象フォルダ数：{len(folders)}、画像数：{sum(len(v) for _, v in folders)}",
        "", "## 設定", "```yaml", yaml.safe_dump(cfg, allow_unicode=True, sort_keys=False).strip(), "```",
    ]
    (exp / "run_info.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
