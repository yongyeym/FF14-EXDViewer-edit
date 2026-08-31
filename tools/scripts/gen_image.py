"""FF14 EXDViewer 数据表内容Diff总结表 - 图片生成工具（PIL 绘制表格，嵌入图标缩略图）。
用法: gen_image_tool.exe <json> <out.png>
不依赖本地 python（PyInstaller 打包），使用 Pillow 库绘制。
"""
import json, sys, os
from PIL import Image, ImageDraw, ImageFont

def load_font(size):
    # Windows 常见中文字体回退
    for name in ['msyh.ttc', 'simhei.ttf', 'simsun.ttc', 'arial.ttf']:
        try:
            return ImageFont.truetype(name, size)
        except Exception:
            continue
    return ImageFont.load_default()

def main():
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    cols = data['columns']
    rows = data['rows']

    header_h = 36
    row_h = 30
    pad = 6
    # 列宽：第1列 Row，其余按是否图片列
    widths = [90] + [46 if c.get('is_icon') else 150 for c in cols]
    ncols = len(widths)

    W = sum(widths) + pad * (ncols + 1)
    H = header_h + row_h * len(rows) + pad * (len(rows) + 1) + 30

    img = Image.new('RGB', (W, H), 'white')
    d = ImageDraw.Draw(img)
    font = load_font(13)
    font_b = load_font(13)

    def draw_cell(x, y, w, h, text, fill=None, font=None, anchor='lm'):
        d.rectangle([x, y, x + w, y + h], fill=fill or 'white', outline=(200, 200, 200))
        d.text((x + 6, y + h / 2), str(text), fill=(20, 20, 20), font=font or load_font(11), anchor=anchor or 'lm')

    # 表头
    base_x = pad
    base_y = pad
    hdr = ['Row'] + [c['name'] for c in cols]
    for ci, w in enumerate(widths):
        x = base_x + sum(widths[:ci]) + pad * ci
        d.rectangle([x, base_y, x + w, base_y + header_h], fill=(40, 90, 160))
        d.text((x + 6, base_y + header_h / 2), str(hdr[ci]), fill='white', font=font, anchor='lm')

    # 行
    for ri, row in enumerate(rows):
        y = base_y + header_h + pad + ri * (row_h + pad)
        cells = row['cells']
        # Row 列
        draw_cell(base_x, y, widths[0], row_h, row['row'], fill=(245, 248, 252), font=load_font(11))
        for ci in range(len(cols)):
            x = base_x + sum(widths[:ci + 1]) + pad * (ci + 1)
            cell = cells[ci] if ci < len(cells) else {}
            w = widths[ci + 1]
            icon_path = cell.get('icon_path') if isinstance(cell, dict) else None
            if icon_path and os.path.exists(icon_path):
                # 图标列：显示缩略图 + id
                draw_cell(x, y, w, row_h, '', fill=(255, 255, 238), font=load_font(11))
                try:
                    icon = Image.open(icon_path).convert('RGBA')
                    icon.thumbnail((w - 8, row_h - 8))
                    ox = x + (w - icon.width) // 2
                    oy = y + (row_h - icon.height) // 2
                    img.paste(icon, (ox, oy), icon)
                    d.text((x + w / 2, y + row_h - 7), str(cell.get('id', '')), fill=(120, 120, 120),
                           font=load_font(8), anchor='mb')
                except Exception:
                    d.text((x + 6, y + row_h / 2), str(cell.get('id', '')), fill=(20, 20, 20), font=load_font(11), anchor='lm')
            else:
                txt = cell.get('value', '') if isinstance(cell, dict) else ''
                draw_cell(x, y, w, row_h, txt, font=load_font(11))

    # 标题
    title = data['sheet_name']
    if data['total_pages'] > 1:
        title += f"  (第 {data['page'] + 1}/{data['total_pages']} 页)"
    d.text((pad, base_y + header_h + pad + len(rows) * (row_h + pad) + 6), title, fill=(40, 90, 160), font=load_font(14))

    img.save(out)
    print("saved", out)

main()
