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
    {label}      設定の名前
    そのほか     runs の params / sweep に書いた値（例：{spec_sigma}）

出力（work_dir/日付_実験名/）：
    README.md, run_info.md, config.yaml, images.txt      （run_experiment.py と同じ）
    results/summary.csv
    results/<画像フォルダ名>/
        raw/<画像名>__<設定>.csv   exe が出したそのままの結果
        logs/<画像名>__<設定>.log  exe の標準出力・エラー出力・終了コード・かかった時間
        measurements.csv          raw をまとめ、列名をそろえたもの
        stats.csv, conditions.csv, figs/
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import datetime as dt
import hashlib
import io
import shlex
import subprocess
import sys
import time
from pathlib import Path

import pandas as pd
import yaml

from run_experiment import (expand_methods, find_image_folders, make_experiment_dir, save_plots,
                            stats_for, write_readme_template, write_run_info)


# ================================================================ 設定
def load_config(path: Path, args) -> dict:
    with open(path, encoding="utf-8") as fp:
        cfg = yaml.safe_load(fp)
    if args.image_root:
        cfg["paths"]["image_root"] = args.image_root
    if args.work_dir:
        cfg["paths"]["work_dir"] = args.work_dir
    if args.folders:
        cfg["images"]["folders"] = args.folders
    # run_experiment の expand_methods をそのまま使えるよう、runs を methods の形にする
    cfg["methods"] = [dict(r, name=r.get("name", "exe")) for r in cfg["runs"]]
    return cfg


def exe_fingerprint(cmd0: str) -> str:
    """exe の版の記録：更新日時と SHA-256 の先頭（同じ exe で計算したかを後から確認できる）"""
    p = Path(cmd0)
    if not p.is_file():
        return f"{cmd0}（ファイルとしては見つからない。PATH 上のコマンドの可能性）"
    h = hashlib.sha256(p.read_bytes()).hexdigest()[:16]
    t = dt.datetime.fromtimestamp(p.stat().st_mtime).isoformat(timespec="seconds")
    return f"{p}（更新 {t}、SHA-256 {h}…）"


# ================================================================ 1回の呼び出し
def build_jobs(cfg: dict, folders, exp: Path) -> list[dict]:
    unit = cfg["program"].get("unit", "image")
    runs = expand_methods(cfg)
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
        for r in runs:
            for keys, name, refname in items:
                stem = Path(name).stem if name != "(folder)" else "folder"
                out = rdir / "raw" / f"{stem}__{r['label']}.csv"
                log = rdir / "logs" / f"{stem}__{r['label']}.log"
                values = dict(r["params"], **keys, out=str(out), out_dir=str(rdir), label=r["label"])
                cmd = [str(a).format(**values) for a in cfg["program"]["command"]]
                jobs.append({"folder": rel, "rdir": rdir, "image": name, "ref_image": refname,
                             "label": r["label"], "params": r["params"], "cmd": cmd, "out": out, "log": log})
    return jobs


def run_job(job: dict, prog: dict) -> dict:
    job["out"].parent.mkdir(parents=True, exist_ok=True)
    job["log"].parent.mkdir(parents=True, exist_ok=True)
    if prog.get("skip_existing", False) and job["out"].exists():
        return dict(job, returncode=0, seconds=0.0, skipped=True)
    t0 = time.perf_counter()
    try:
        cp = subprocess.run(job["cmd"], capture_output=True, text=True, errors="replace",
                            timeout=prog.get("timeout_s"), cwd=prog.get("cwd"))
        rc, so, se = cp.returncode, cp.stdout, cp.stderr
    except subprocess.TimeoutExpired as e:
        rc, so, se = -9, e.stdout or "", f"タイムアウト（{prog.get('timeout_s')} 秒）"
    except FileNotFoundError as e:
        rc, so, se = -1, "", f"プログラムが見つかりません: {e}"
    sec = time.perf_counter() - t0
    if prog.get("output", "file") == "stdout" and rc == 0:
        job["out"].write_text(so, encoding="utf-8")
    job["log"].write_text(f"command: {shlex.join(job['cmd'])}\nreturncode: {rc}\nseconds: {sec:.3f}\n"
                          f"--- stdout ---\n{so}\n--- stderr ---\n{se}\n", encoding="utf-8")
    return dict(job, returncode=rc, seconds=sec, skipped=False)


# ================================================================ 結果の読み込み
def read_output(job: dict, prog: dict) -> pd.DataFrame | None:
    if not job["out"].exists():
        return None
    csv = prog.get("csv", {}) or {}
    text = job["out"].read_text(encoding=csv.get("encoding", "utf-8-sig"), errors="replace")
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
        df[f"param_{k}"] = v
    return df


# ================================================================ メイン
def main():
    ap = argparse.ArgumentParser(description="コンパイル済みプログラムを呼び出して、実験結果を決まった構成で保存する")
    ap.add_argument("config")
    ap.add_argument("--image-root")
    ap.add_argument("--work-dir")
    ap.add_argument("--folders", nargs="+")
    ap.add_argument("--out", help="出力フォルダを直接指定（既存なら追記）")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    cfg = load_config(Path(args.config), args)
    prog = cfg["program"]
    folders = find_image_folders(cfg)
    print(f"対象フォルダ {len(folders)} 個：")
    for rel, imgs in folders:
        print(f"  {rel}  （{len(imgs)} 枚）")
    if not folders:
        sys.exit("画像フォルダが見つかりません。image_root と images.folders を確認してください。")

    if args.dry_run:
        jobs = build_jobs(cfg, folders, Path(cfg["paths"]["work_dir"]) / "(dry-run)")
        print(f"\n呼び出し回数：{len(jobs)} 回。最初のコマンドの例：")
        for j in jobs[:3]:
            print("  " + shlex.join(j["cmd"]))
        return

    exp = make_experiment_dir(cfg, args.out)
    print(f"出力先：{exp}")
    with open(exp / "config.yaml", "w", encoding="utf-8") as fp:
        yaml.safe_dump({k: v for k, v in cfg.items() if k != "methods"}, fp, allow_unicode=True, sort_keys=False)
    (exp / "images.txt").write_text("\n".join(str(p) for _, imgs in folders for p in imgs) + "\n", encoding="utf-8")
    write_readme_template(exp, cfg)
    write_run_info(exp, {k: v for k, v in cfg.items() if k != "methods"}, folders, args)
    with open(exp / "run_info.md", "a", encoding="utf-8") as fp:
        fp.write(f"\n## 使ったプログラム\n- {exe_fingerprint(str(prog['command'][0]))}\n")
        if prog.get("version_command"):
            try:
                v = subprocess.run(prog["version_command"], capture_output=True, text=True, timeout=10)
                fp.write(f"- 版の表示：{(v.stdout or v.stderr).strip()}\n")
            except Exception as e:
                fp.write(f"- 版の表示に失敗：{e}\n")

    for rel, imgs in folders:
        rdir = exp / "results" / rel
        rdir.mkdir(parents=True, exist_ok=True)
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

    # 実行の一覧（失敗も含めて）
    pd.DataFrame([{k: (shlex.join(v) if k == "cmd" else v) for k, v in d.items()
                   if k in ("folder", "image", "label", "returncode", "seconds", "skipped", "cmd")}
                  for d in done]).to_csv(exp / "results" / "runs.csv", index=False, encoding="utf-8-sig")

    all_stats = []
    for rel, _ in folders:
        rdir = exp / "results" / rel
        dfs = [read_output(d, prog) for d in done if d["folder"] == rel and d["returncode"] == 0]
        dfs = [d for d in dfs if d is not None and len(d)]
        if not dfs:
            continue
        df = pd.concat(dfs, ignore_index=True)
        df.to_csv(rdir / "measurements.csv", index=False, encoding="utf-8-sig")
        need = {"block_y", "dx", "dy"}
        if need <= set(df.columns):
            if "block_x" not in df.columns:
                df["block_x"] = 0
            if "score" not in df.columns:
                df["score"] = float("nan")
            st = stats_for(df, cfg.get("stats", {}).get("fail_score"))
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
