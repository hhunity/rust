#!/usr/bin/env python3
"""CSVのSMILES列を構造式PNGに変換し、Excel(.xlsx)の同じ列に埋め込む。

使い方:
    python smiles_to_excel.py input.csv output.xlsx --column E
    python smiles_to_excel.py input.csv output.xlsx --column SMILES
"""
import argparse
import csv
import io
import sys
from pathlib import Path

from openpyxl import Workbook
from openpyxl.drawing.image import Image as XLImage
from openpyxl.utils import column_index_from_string, get_column_letter
from PIL import Image as PILImage
from rdkit import Chem
from rdkit.Chem import Draw

IMG_SIZE = (200, 200)
CELL_ROW_HEIGHT = 150  # points
CELL_COL_WIDTH = 28  # excel column width units


def resolve_column_index(header, column_arg):
    """--column に列名(例: SMILES)か列記号(例: E)のどちらが来ても列インデックス(0始まり)を返す"""
    if column_arg in header:
        return header.index(column_arg)
    try:
        return column_index_from_string(column_arg.upper()) - 1
    except ValueError:
        pass
    raise SystemExit(
        f"列 '{column_arg}' が見つかりません。ヘッダー: {header}"
    )


def smiles_to_png_bytes(smiles):
    mol = Chem.MolFromSmiles(smiles)
    if mol is None:
        return None
    pil_img = Draw.MolToImage(mol, size=IMG_SIZE)
    buf = io.BytesIO()
    pil_img.save(buf, format="PNG")
    buf.seek(0)
    return buf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_csv", type=Path)
    parser.add_argument("output_xlsx", type=Path)
    parser.add_argument(
        "--column",
        default="E",
        help="SMILESが入っている列。列記号(E)またはヘッダー名(SMILES)。既定: E",
    )
    args = parser.parse_args()

    with open(args.input_csv, newline="", encoding="utf-8-sig") as f:
        rows = list(csv.reader(f))
    if not rows:
        raise SystemExit("CSVが空です")

    header, data_rows = rows[0], rows[1:]
    smiles_idx = resolve_column_index(header, args.column)

    wb = Workbook()
    ws = wb.active
    ws.append(header)

    smiles_col_letter = get_column_letter(smiles_idx + 1)
    ws.column_dimensions[smiles_col_letter].width = CELL_COL_WIDTH

    failed = []
    for row_i, row in enumerate(data_rows, start=2):
        ws.append(row)
        ws.row_dimensions[row_i].height = CELL_ROW_HEIGHT

        smiles = row[smiles_idx].strip() if smiles_idx < len(row) else ""
        if not smiles:
            continue

        png_buf = smiles_to_png_bytes(smiles)
        if png_buf is None:
            failed.append((row_i, smiles))
            continue

        cell_ref = f"{smiles_col_letter}{row_i}"
        ws[cell_ref] = None  # SMILESテキストを消して画像に差し替え

        img = XLImage(png_buf)
        img.width, img.height = IMG_SIZE
        img.anchor = cell_ref
        ws.add_image(img)

    wb.save(args.output_xlsx)
    print(f"書き出し完了: {args.output_xlsx} ({len(data_rows)}行, 失敗{len(failed)}件)")
    for row_i, smiles in failed:
        print(f"  [warn] row {row_i}: SMILES解析失敗 -> {smiles!r}", file=sys.stderr)


if __name__ == "__main__":
    main()
