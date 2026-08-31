"""FF14 EXDViewer 数据表内容Diff总结表 - Excel 生成工具（openpyxl）。
用法: gen_excel_tool.exe <json> <out.xlsx>
不依赖本地 python（PyInstaller 打包）。图标列只显示缩略图(最低40px,不含原始id)，
列宽/行高自适应文本内容，单元格内容居中。
"""
import json, sys, os
from openpyxl import Workbook
from openpyxl.styles import Alignment
from openpyxl.drawing.image import Image as XLImage
from openpyxl.utils import get_column_letter

MIN_ICON = 40

def main():
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    wb = Workbook(); ws = wb.active
    ws.title = data['sheet_name'][:31]
    cols = data['columns']
    ncols = len(cols) + 1
    center = Alignment(horizontal='center', vertical='center', wrap_text=True)

    # 表头（居中对齐+加粗）
    headers = ['Row'] + [c['name'] for c in cols]
    for ci, h in enumerate(headers, start=1):
        c = ws.cell(row=1, column=ci, value=h)
        c.alignment = center
        from openpyxl.styles import Font
        c.font = Font(bold=True)

    # 行数据
    for ri, row in enumerate(data['rows'], start=2):
        ws.cell(row=ri, column=1, value=row['row']).alignment = center
        for ci, cell in enumerate(row['cells'], start=2):
            if isinstance(cell, dict) and 'value' in cell:
                c = ws.cell(row=ri, column=ci, value=cell['value'])
                c.alignment = center
            else:
                # 图片列：只嵌图，不写原始id
                ws.cell(row=ri, column=ci, value='').alignment = center

    # 列宽自适应：文本长度；图片列预留图标宽度
    for ci in range(1, ncols + 1):
        maxlen = len(str(headers[ci - 1]))
        col_letter = get_column_letter(ci)
        is_icon_col = ci > 1 and cols[ci - 2].get('is_icon')
        for ri in range(2, len(data['rows']) + 2):
            v = ws.cell(row=ri, column=ci).value
            if v is not None and v != '':
                maxlen = max(maxlen, len(str(v)))
        if is_icon_col:
            width = max(8, MIN_ICON / 6)
        else:
            width = max(8, min(60, maxlen * 2.4))
        ws.column_dimensions[col_letter].width = width

    # 行高自适应（图片行加高容纳缩略图；文本行 wrap 后略增）
    for ri in range(2, len(data['rows']) + 2):
        row_h = 20
        for ci, cell in enumerate(data['rows'][ri - 2]['cells'], start=2):
            if isinstance(cell, dict) and 'icon_path' in cell and os.path.exists(cell['icon_path']):
                row_h = max(row_h, MIN_ICON)
            elif isinstance(cell, dict) and 'value' in cell:
                w = ws.column_dimensions[get_column_letter(ci)].width or 8
                est_lines = max(1, (len(str(cell['value'])) // max(1, int(w / 2.4))))
                row_h = max(row_h, est_lines * 15)
        ws.row_dimensions[ri].height = row_h

    # 嵌入缩略图（最低40px，居中锚定单元格）
    for ri, row in enumerate(data['rows'], start=2):
        for ci, cell in enumerate(row['cells'], start=2):
            if isinstance(cell, dict) and 'icon_path' in cell and os.path.exists(cell['icon_path']):
                try:
                    img = XLImage(cell['icon_path'])
                    img.width = MIN_ICON
                    img.height = MIN_ICON
                    ws.add_image(img, f"{get_column_letter(ci)}{ri}")
                except Exception:
                    pass

    wb.save(out)
    print("saved", out)

main()
