"""コンパイル済みのプログラム（C/C++ の exe）を呼び出して、決まったフォルダ構成で結果を保存する

Python は「どの画像を、どの設定で、どこに保存するか」だけを担当し、計算は exe に任せます。

使い方：
    python run_exe.py config_exe.yaml --dry-run          # 対象フォルダと、実行するコマンドの例を表示するだけ
    python run_exe.py config_exe.yaml
    python run_exe.py config_exe.yaml --folders img_001 "img_00[2-5]" --image-root D:/images --work-dir D:/work

exe に渡す引数は、設定ファイルの program.command に {名前} の形で書きます。使える名前：
    {image}      画像のフルパス（unit: image のとき）
    {ref} {tgt}  基準画像・比較画像のフルパス（unit: pair のとき）
    {folder}     画像フォルダのフルパス（unit: folder のとき。フォルダごとに1回呼ぶ）
    {out}        exe に書き出してほしい結果ファイルのパス
    {out_dir}    結果フォルダのパス
    {run_dir}    呼び出しごとの出力フォルダ（exe が CSV 以外のファイルを書く場所）
    {stem}       画像ファイル名（拡張子なし）
    {label}      設定の名前
    そのほか     runs の params / sweep に書いた値（例：{spec_sigma}）
                 と、画像フォルダの params.yaml に書いた画像ごとの値（例：{ref_x}）
    配列の値     "{名前*}"（引数がちょうどこれだけのとき）→ 要素ごとに別々の引数（例：--ex 12000 12500）
                 "{名前}" → カンマでつないだ 1 つの引数（例：--ex 12000,12500）

画像ごとのパラメータ（画像フォルダの params.yaml。ファイル名は images.params_file で変えられる）：
    default:              # フォルダ内の全画像に共通
      ref_x: 1000
    img003.png:           # この画像だけ上書き
      target_x: 5100
    runs と同じ名前があるとエラーで止める（比べたい設定が気づかずに上書きされるのを防ぐ）
    使った params.yaml は results/<フォルダ>/ にコピーし、値は measurements.csv の param_<名前> 列に残る

出力（work_dir/日付_実験名/）：
    README.md         目的・条件・結果・結論のひな形（初回だけ作る。手で書き足す）
    run_info.md       実行日時、コマンド、exe の版、設定（毎回上書き）
    config.yaml       実行時の設定のコピー
    images.txt        使った画像のフルパス
    results/runs.csv  全呼び出しの一覧（失敗も含む）
    results/summary.csv
    results/<画像フォルダ名>/
        raw/<画像名>__<設定>.csv   exe が出したそのままの結果
        files/<画像名>__<設定>/    exe が {run_dir} に書いたファイル（差分画像など）
        logs/<画像名>__<設定>.log  exe の標準出力・エラー出力・終了コード・かかった時間
        measurements.csv          raw をまとめ、列名をそろえたもの
        measurements_<名前>.csv   stats.series に書いた「まとめた値」を 1 グループ 1 行にしたもの
        stats.csv, conditions.csv, figs/
"""
from __future__ import annotations

import argparse
import fnmatch
import itertools
import platform
import concurrent.futures as cf
import datetime as dt
import hashlib
import io
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import pandas as pd
import yaml


# ================================================================ 画像フォルダ・結果フォルダ・集計
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


def expand_settings(items: list[dict]) -> list[dict]:
    """runs を、sweep の組み合わせに展開する。戻り値の各要素：{"label": 設定の名前, "params": 値の辞書}"""
    out = []
    for m in items:
        base = dict(m.get("params", {}) or {})
        sweep = m.get("sweep", {}) or {}
        for k, v in sweep.items():
            # 文字列のまま product に渡すと、1文字ずつ別の設定になってしまう（"0.05, 0.10" → 10 通り）
            if not isinstance(v, list):
                sys.exit(f"runs の sweep の値はリストで書いてください：{k}: {v!r}\n"
                         f"  正しい例：sweep: {{{k}: [0.05, 0.10]}}  または\n"
                         f"    sweep:\n      {k}: [0.05, 0.10]")
        keys = list(sweep)
        for values in itertools.product(*[sweep[k] for k in keys]) if keys else [()]:
            params = dict(base, **dict(zip(keys, values)))
            label = m.get("label", "run").format(**params)
            out.append({"label": label, "params": params})
    # label が重なると、結果ファイルが上書きされる
    dup = sorted({o["label"] for o in out if [x["label"] for x in out].count(o["label"]) > 1})
    if dup:
        sys.exit(f"runs の label が重なっています：{dup}。sweep を使うときは label に値を入れてください（例：\"s{{spec_sigma}}\"）")
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


def derive_series(df: pd.DataFrame, cx: str, cy: str, group_col: str | None) -> pd.DataFrame:
    """exe が同じ CSV に書いた「まとめた値」（例：100 ブロックごとの平均 ave_dx, ave_dy）を、1グループ 1 行の表にする
    ・group_col があれば、その列が同じ行を 1 グループとみなす
    ・なければ、(cx, cy) の値が前の行と同じあいだを 1 グループとみなす
      （全行に同じ値をくり返す書き方でも、グループの先頭行にだけ書く書き方でもよい。空の行は捨てる）
    block_y はグループに含まれるブロックの y の平均（グループの中心）"""
    out = []
    for (img, label), g in df.groupby(["image", "label"], sort=False):
        g = g.sort_values(["block_x", "block_y"])
        if group_col:
            key = g[group_col]
        else:
            v = g[[cx, cy]].ffill()   # 先頭行にだけ書く形 → 下の空の行を同じグループで埋める
            key = (v != v.shift()).any(axis=1).cumsum()
        g = g.assign(_k=key.values)
        g[[cx, cy]] = g.groupby("_k")[[cx, cy]].transform("first")
        a = g.groupby("_k", sort=False).agg(block_x=("block_x", "first"), block_y=("block_y", "mean"),
                                            dx=(cx, "first"), dy=(cy, "first"), score=("score", "median"),
                                            n_in_group=("block_y", "size"))
        a = a.dropna(subset=["dx", "dy"], how="all")
        a["block_y"] = a["block_y"].round(3)   # 画像間でグループ位置を突き合わせるため（repeat_std）
        out.append(a.assign(image=img, label=label, method=g["method"].iloc[0]).reset_index(drop=True))
    return pd.concat(out, ignore_index=True) if out else pd.DataFrame()


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


# ================================================================ 設定
def load_config(path: Path, args) -> dict:
    with open(path, encoding="utf-8") as fp:
        cfg = yaml.safe_load(fp)
    cfg["_cli"] = {}
    if args.image_root:
        cfg["paths"]["image_root"] = str(Path(args.image_root).resolve())
        cfg["_cli"]["image_root"] = True
    if args.work_dir:
        cfg["paths"]["work_dir"] = str(Path(args.work_dir).resolve())
        cfg["_cli"]["work_dir"] = True
    if args.folders:
        cfg["images"]["folders"] = args.folders
    resolve_paths(cfg, path.resolve().parent)
    cfg.pop("_cli")
    return cfg


def resolve_paths(cfg: dict, base: Path):
    """設定ファイルの中の相対パスを、設定ファイルの場所を基準にした絶対パスに直す。
    （どこから python を実行しても、同じ exe・同じフォルダを指すようにするため）
    コマンドラインで指定したパスは、実行した場所（カレントディレクトリ）基準のまま。"""
    def absolute(p: str) -> str:
        q = Path(p)
        return str(q if q.is_absolute() else (base / q).resolve())

    for key in ("image_root", "work_dir"):
        v = cfg["paths"].get(key)
        if v and not Path(v).is_absolute() and not cfg.get("_cli", {}).get(key):
            cfg["paths"][key] = absolute(v)

    prog = cfg["program"]
    if prog.get("cwd") and "{" not in str(prog["cwd"]):
        prog["cwd"] = absolute(prog["cwd"])
    # {python}：run_exe.py を動かしているのと同じ Python（仮想環境もそのまま）
    prog["command"] = [sys.executable if str(a) == "{python}" else a for a in prog["command"]]
    if prog.get("version_command"):
        prog["version_command"] = [sys.executable if str(a) == "{python}" else a for a in prog["version_command"]]
    # 2要素目以降で、{...} を含まず、設定ファイルの場所から見てファイルとして存在するもの（例：tools/measure.py）も絶対パスにする
    for i in range(1, len(prog["command"])):
        a = str(prog["command"][i])
        if "{" not in a and not a.startswith("-") and not Path(a).is_absolute() and (base / a).is_file():
            prog["command"][i] = str((base / a).resolve())
    exe = str(prog["command"][0])
    if "/" in exe or "\\" in exe:
        # パスとして書かれている（相対なら設定ファイルの場所基準）
        prog["command"][0] = absolute(exe)
        if not Path(prog["command"][0]).is_file():
            sys.exit(f"exe が見つかりません: {prog['command'][0]}（設定ファイル {base} からの相対パスとして探しました）")
    else:
        # 名前だけ（例：measure.exe）なら、設定ファイルと同じフォルダ → PATH の順に探す
        local = base / exe
        if local.is_file():
            prog["command"][0] = str(local)
        elif shutil.which(exe):
            prog["command"][0] = shutil.which(exe)
        else:
            sys.exit(f"exe が見つかりません: {exe}（設定ファイルと同じフォルダにも PATH にもない）")


def exe_fingerprint(command: list) -> str:
    """使ったプログラムの記録：コマンドに含まれる実在ファイル（exe、Python スクリプトなど）の
    更新日時と SHA-256 の先頭。同じプログラムで計算したかを後から確認できる"""
    lines = []
    for a in command:
        p = Path(str(a))
        if "{" in str(a) or not p.is_file():
            continue
        h = hashlib.sha256(p.read_bytes()).hexdigest()[:16]
        t = dt.datetime.fromtimestamp(p.stat().st_mtime).isoformat(timespec="seconds")
        lines.append(f"- {p}（更新 {t}、SHA-256 {h}…）")
    return "\n".join(lines) if lines else f"- {command[0]}（ファイルとしては見つからない）"


# ================================================================ 1回の呼び出し
def build_jobs(cfg: dict, folders, exp: Path) -> list[dict]:
    unit = cfg["program"].get("unit", "image")
    runs = expand_settings(cfg["runs"])
    jobs = []
    for rel, imgs in folders:
        rdir = exp / "results" / rel
        if unit == "image":
            items = [({"image": str(p)}, p.name, p.name) for p in imgs]
        elif unit == "pair":
            refname = cfg["program"].get("ref_image")
            ref = next((p for p in imgs if p.name == refname), imgs[0])
            items = [({"ref": str(ref), "tgt": str(p)}, p.name, ref.name) for p in imgs if p != ref]
        elif unit == "folder":
            items = [({"folder": str(imgs[0].parent)}, "(folder)", "")]
        else:
            raise ValueError(f"program.unit が不正です: {unit}")
        img_params = load_image_params(imgs[0].parent, imgs, cfg["images"].get("params_file", "params.yaml"))
        prog = cfg["program"]
        for r in runs:
            for keys, name, refname in items:
                stem = Path(name).stem if name != "(folder)" else "folder"
                tag = f"{stem}__{r['label']}"
                out = rdir / "raw" / f"{tag}.csv"
                run_dir = rdir / "files" / tag              # exe が CSV 以外のファイルを書く場所（呼び出しごとに別）
                log = rdir / "logs" / f"{tag}.log"
                ip = img_params.get(name, img_params.get("default", {}))
                both = sorted(set(ip) & set(r["params"]))
                if both:
                    sys.exit(f"{rel}/{name}：{both} が画像ごとのパラメータ（{cfg['images'].get('params_file', 'params.yaml')}）と"
                             f" runs の両方にあります。\n  比べたい設定が気づかずに上書きされるのを防ぐため、どちらか一方にしてください。")
                params = dict(ip, **r["params"])
                values = dict(params, **keys, out=str(out), out_dir=str(rdir), run_dir=str(run_dir),
                              stem=stem, label=r["label"])
                where = f"{rel}/{name}（設定 {r['label']}）"
                cmd = expand_command(prog["command"], values, where)
                # 集計に使う CSV：既定は {out}。exe が決まった名前で書く場合は result_csv で場所を指定
                result = Path(fill(str(prog.get("result_csv", "{out}")), values, where))
                cwd = fill(str(prog["cwd"]), values, where) if prog.get("cwd") else None
                jobs.append({"folder": rel, "rdir": rdir, "image": name, "ref_image": refname,
                             "label": r["label"], "params": params, "cmd": cmd, "out": out,
                             "result": result, "run_dir": run_dir, "cwd": cwd, "log": log})
    return jobs


# ================================================================ 画像ごとのパラメータ
def load_image_params(folder: Path, imgs: list[Path], fname: str) -> dict:
    """画像フォルダの params.yaml を読み、{画像名: そのパラメータ, "default": 共通} を返す。
    画像名の項目は default に上書きした結果。ファイルがなければ空。

    params.yaml の書き方：
        default:              # フォルダ内の全画像に共通
          ref_x: 1000
          exclude_rows: [[12000, 12500], [30000, 30200]]
        img003.png:           # この画像だけ上書き（書いたキーだけ。ほかは default のまま）
          target_x: 5100
    """
    p = folder / fname
    if not p.is_file():
        return {}
    with open(p, encoding="utf-8") as fp:
        raw = yaml.safe_load(fp) or {}
    if not isinstance(raw, dict):
        sys.exit(f"{p}：中身は「default: ...」「画像名: ...」の形で書いてください")
    default = raw.get("default", {}) or {}
    names = {q.name for q in imgs}
    unknown = sorted(k for k in raw if k != "default" and k not in names)
    if unknown:   # 画像名の書き間違いは、黙って default が使われるので知らせる
        print(f"  注意：{p} に、フォルダにない画像名があります：{unknown}")
    out = {"default": dict(default)}
    for n in names:
        out[n] = dict(default, **(raw.get(n, {}) or {}))
    return out


def flatten(v) -> list:
    """[[12000, 12500], [30000, 30200]] → [12000, 12500, 30000, 30200]"""
    if isinstance(v, (list, tuple)):
        return [x for e in v for x in flatten(e)]
    return [v]


def fill(s: str, values: dict, where: str) -> str:
    """{名前} を置き換える。配列はカンマでつないだ 1 つの文字列にする（例：12000,12500,30000,30200）"""
    v2 = {k: (",".join(str(x) for x in flatten(v)) if isinstance(v, (list, tuple)) else v) for k, v in values.items()}
    try:
        return s.format(**v2)
    except KeyError as e:
        sys.exit(f"{where}：{{{e.args[0]}}} の値がありません。runs の params / sweep か、画像フォルダの params.yaml に書いてください")


def expand_command(command: list, values: dict, where: str) -> list[str]:
    """コマンドの各引数の {名前} を置き換える。
    引数がちょうど "{名前*}" のときは、配列を要素ごとの別々の引数に展開する（例："{xs*}" → "1000" "3000" "5000"）。
    空の配列なら、その引数は消える。"""
    out = []
    for a in command:
        s = str(a)
        m = re.fullmatch(r"\{(\w+)\*\}", s)
        if m:
            if m[1] not in values:
                sys.exit(f"{where}：{{{m[1]}*}} の値がありません。runs の params / sweep か、画像フォルダの params.yaml に書いてください")
            out.extend(str(x) for x in flatten(values[m[1]]))
        else:
            out.append(fill(s, values, where))
    return out


def run_job(job: dict, prog: dict) -> dict:
    job["out"].parent.mkdir(parents=True, exist_ok=True)
    job["log"].parent.mkdir(parents=True, exist_ok=True)
    job["run_dir"].mkdir(parents=True, exist_ok=True)
    if prog.get("skip_existing", False) and job["result"].exists():
        return dict(job, returncode=0, seconds=0.0, skipped=True, n_files=None)
    t0 = time.perf_counter()
    try:
        cp = subprocess.run(job["cmd"], capture_output=True, text=True, errors="replace",
                            timeout=prog.get("timeout_s"), cwd=job["cwd"])
        rc, so, se = cp.returncode, cp.stdout, cp.stderr
    except subprocess.TimeoutExpired as e:
        rc, so, se = -9, e.stdout or "", f"タイムアウト（{prog.get('timeout_s')} 秒）"
    except FileNotFoundError as e:
        rc, so, se = -1, "", f"プログラムが見つかりません: {e}"
    sec = time.perf_counter() - t0
    if prog.get("output", "file") == "stdout" and rc == 0:
        job["out"].write_text(so, encoding="utf-8")
    files = [p for p in job["run_dir"].rglob("*") if p.is_file()]
    job["log"].write_text(f"command: {shlex.join(job['cmd'])}\ncwd: {job['cwd']}\nreturncode: {rc}\nseconds: {sec:.3f}\n"
                          f"files: {len(files)}\n--- stdout ---\n{so}\n--- stderr ---\n{se}\n", encoding="utf-8")
    return dict(job, returncode=rc, seconds=sec, skipped=False, n_files=len(files))


# ================================================================ 結果の読み込み
def read_output(job: dict, prog: dict) -> pd.DataFrame | None:
    if not job["result"].exists():
        return None
    csv = prog.get("csv", {}) or {}
    text = job["result"].read_text(encoding=csv.get("encoding", "utf-8-sig"), errors="replace")
    if not text.strip():
        return None
    df = pd.read_csv(io.StringIO(text), sep=csv.get("sep", ","), header=0 if csv.get("header", True) else None,
                     comment=csv.get("comment"))
    if not csv.get("header", True):
        df.columns = csv["names"]
    cols = prog.get("columns", {}) or {}
    df = df.rename(columns={v: k for k, v in cols.items()})       # exe の列名 → 標準の列名
    if "image" not in df.columns or job["image"] != "(folder)":
        df["image"] = job["image"]
    df["ref_image"] = job["ref_image"]
    df["label"] = job["label"]
    df["method"] = "exe"
    for k, v in job["params"].items():
        # 配列は 1 つのセルに入るよう文字列にする（例：[[12000, 12500], [30000, 30200]] → "[[12000, 12500], [30000, 30200]]"）
        df[f"param_{k}"] = str(v) if isinstance(v, (list, tuple, dict)) else v
    return df


# ================================================================ メイン
def main():
    ap = argparse.ArgumentParser(description="コンパイル済みプログラムを呼び出して、実験結果を決まった構成で保存する")
    ap.add_argument("config")
    ap.add_argument("--image-root")
    ap.add_argument("--work-dir")
    ap.add_argument("--folders", nargs="+")
    ap.add_argument("--out", help="出力フォルダを直接指定（既存なら追記）")
    ap.add_argument("--dry-run", action="store_true", help="実行せず、対象フォルダとコマンドを表示するだけ")
    ap.add_argument("--show-all", action="store_true", help="--dry-run のとき、全部のコマンドを表示する")
    args = ap.parse_args()

    cfg = load_config(Path(args.config), args)
    prog = cfg["program"]
    root = Path(cfg["paths"]["image_root"]).resolve()
    work = Path(cfg["paths"]["work_dir"]).resolve()
    if work == root or root in work.parents:
        sys.exit(f"work_dir（{work}）が image_root（{root}）の中にあります。\n"
                 f"  結果のグラフや出力画像を、次の実行で画像として拾ってしまうので、image_root の外にしてください。")
    folders = find_image_folders(cfg)
    print(f"対象フォルダ {len(folders)} 個：")
    for rel, imgs in folders:
        print(f"  {rel}  （{len(imgs)} 枚）")
    if not folders:
        sys.exit("画像フォルダが見つかりません。image_root と images.folders を確認してください。")

    if args.dry_run:
        jobs = build_jobs(cfg, folders, Path(cfg["paths"]["work_dir"]) / "(dry-run)")
        print(f"\n呼び出し回数：合計 {len(jobs)} 回")
        # 設定（runs を sweep で展開したもの）ごとに、回数とコマンドの例を1つずつ表示する
        by_label = {}
        for j in jobs:
            by_label.setdefault(j["label"], []).append(j)
        print(f"設定 {len(by_label)} 通り：")
        for label, js in by_label.items():
            params = ", ".join(f"{k}={v}" for k, v in js[0]["params"].items())
            print(f"\n  [{label}]  {params}  → {len(js)} 回")
            print("    例：" + shlex.join(js[0]["cmd"]))
        # 画像ごとのパラメータ（params.yaml）がある場合、画像ごとに使う値を表示する
        pfn = cfg["images"].get("params_file", "params.yaml")
        for rel, imgs in folders:
            ip = load_image_params(imgs[0].parent, imgs, pfn)
            if ip:
                print(f"\n  画像ごとのパラメータ（{rel}/{pfn}）：")
                for p in imgs:
                    print(f"    {p.name}: " + ", ".join(f"{k}={v}" for k, v in ip[p.name].items()))
        if args.show_all:
            print("\n全コマンド：")
            for j in jobs:
                print("  " + shlex.join(j["cmd"]))
        else:
            print("\n（全部のコマンドを見るには --show-all を付ける）")
        return

    exp = make_experiment_dir(cfg, args.out)
    print(f"出力先：{exp}")
    with open(exp / "config.yaml", "w", encoding="utf-8") as fp:
        yaml.safe_dump(cfg, fp, allow_unicode=True, sort_keys=False)
    (exp / "images.txt").write_text("\n".join(str(p) for _, imgs in folders for p in imgs) + "\n", encoding="utf-8")
    write_readme_template(exp, cfg)
    write_run_info(exp, cfg, folders, args)
    with open(exp / "run_info.md", "a", encoding="utf-8") as fp:
        fp.write(f"\n## 使ったプログラム\n{exe_fingerprint(prog['command'])}\n")
        if prog.get("version_command"):
            try:
                v = subprocess.run(prog["version_command"], capture_output=True, text=True, timeout=10)
                fp.write(f"- 版の表示：{(v.stdout or v.stderr).strip()}\n")
            except Exception as e:
                fp.write(f"- 版の表示に失敗：{e}\n")

    for rel, imgs in folders:
        rdir = exp / "results" / rel
        rdir.mkdir(parents=True, exist_ok=True)
        pf = imgs[0].parent / cfg["images"].get("params_file", "params.yaml")
        if pf.is_file():   # 画像ごとのパラメータを、使った時点のまま残す（あとで params.yaml を書き換えても分かるように）
            shutil.copy2(pf, rdir / pf.name)
        cond = rdir / "conditions.csv"
        if not cond.exists():
            pd.DataFrame({"image": [p.name for p in imgs], "exposure_us": "", "F": "", "focus": "",
                          "gain": "", "light": "", "memo": ""}).to_csv(cond, index=False, encoding="utf-8-sig")

    jobs = build_jobs(cfg, folders, exp)
    print(f"呼び出し {len(jobs)} 回（並列 {prog.get('parallel', 1)}）")
    done = []
    with cf.ThreadPoolExecutor(max_workers=prog.get("parallel", 1)) as ex:
        for i, res in enumerate(ex.map(lambda j: run_job(j, prog), jobs), 1):
            mark = "OK" if res["returncode"] == 0 else f"失敗(終了コード {res['returncode']})"
            print(f"  [{i}/{len(jobs)}] {res['folder']}/{res['image']} {res['label']}: {mark} {res['seconds']:.1f}s")
            done.append(res)

    # 空のフォルダを消す（CSV しか出さない exe のとき、空の files/ が並ばないように）。並列実行が終わってから行う
    for rel, _ in folders:
        fdir = exp / "results" / rel / "files"
        if fdir.is_dir():
            for d in sorted((p for p in fdir.rglob("*") if p.is_dir()), key=lambda p: len(p.parts), reverse=True):
                if not any(d.iterdir()):
                    d.rmdir()
            if not any(fdir.iterdir()):
                fdir.rmdir()

    # 実行の一覧（失敗も含めて）
    pd.DataFrame([{k: (shlex.join(v) if k == "cmd" else v) for k, v in d.items()
                   if k in ("folder", "image", "label", "returncode", "seconds", "skipped", "n_files", "cmd")}
                  for d in done]).to_csv(exp / "results" / "runs.csv", index=False, encoding="utf-8-sig")

    all_stats = []
    for rel, _ in folders:
        rdir = exp / "results" / rel
        mine = [d for d in done if d["folder"] == rel]
        failed = [d for d in mine if d["returncode"] != 0]
        missing = [d for d in mine if d["returncode"] == 0 and not d["result"].exists()]
        dfs = [read_output(d, prog) for d in mine if d["returncode"] == 0]
        dfs = [d for d in dfs if d is not None and len(d)]
        # measurements.csv ができない理由を表示する
        if failed:
            print(f"  [{rel}] 終了コードが 0 以外：{len(failed)} 回（例：{failed[0]['log']}）")
        if missing:
            print(f"  [{rel}] 結果 CSV が見つからない：{len(missing)} 回（探した場所の例：{missing[0]['result']}）")
        if not dfs:
            print(f"  [{rel}] 読める結果が 0 件なので measurements.csv を作りません")
            continue
        df = pd.concat(dfs, ignore_index=True)
        df.to_csv(rdir / "measurements.csv", index=False, encoding="utf-8-sig")
        need = {"block_y", "dx", "dy"}
        if need <= set(df.columns):
            if "block_x" not in df.columns:
                df["block_x"] = 0
            if "score" not in df.columns:
                df["score"] = float("nan")
            scfg = cfg.get("stats", {}) or {}
            st = stats_for(df, scfg.get("fail_score")).assign(series="block")
            # exe が同じ CSV に書いた「まとめた値」も同じ指標で集計する（stats.series）
            for name, spec in (scfg.get("series") or {}).items():
                cx, cy = spec["dx"], spec["dy"]
                if cx not in df.columns or cy not in df.columns:
                    print(f"  [{rel}] stats.series.{name}：列 {cx}, {cy} が CSV にないので省略")
                    continue
                ds = derive_series(df, cx, cy, spec.get("group"))
                if len(ds):
                    ds.to_csv(rdir / f"measurements_{name}.csv", index=False, encoding="utf-8-sig")
                    st = pd.concat([st, stats_for(ds, None).assign(series=name)], ignore_index=True)
            st = st[["series"] + [c for c in st.columns if c != "series"]]
            st.to_csv(rdir / "stats.csv", index=False, encoding="utf-8-sig")
            if cfg.get("plots", True):
                save_plots(df, rdir / "figs")
            all_stats.append(st.assign(folder=rel))
        else:
            print(f"  {rel}: 列 {sorted(need - set(df.columns))} がないので、集計とグラフは省略（program.columns で対応を指定）")

    if all_stats:
        summary = pd.concat(all_stats, ignore_index=True)
        cols = ["folder"] + [k for k in summary.columns if k != "folder"]
        summary[cols].to_csv(exp / "results" / "summary.csv", index=False, encoding="utf-8-sig")
    n_fail = sum(d["returncode"] != 0 for d in done)
    print(f"完了（失敗 {n_fail} 回。詳しくは results/runs.csv と各 logs/）")


if __name__ == "__main__":
    main()
