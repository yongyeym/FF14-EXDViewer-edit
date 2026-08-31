"""FF14 EXDViewer 数据表内容Diff总结表 - 图片生成工具（PIL 绘制，自适应表格，嵌入图标）。
用法: gen_image_tool.exe <json> <out.png>
不依赖本地 python（PyInstaller 打包）。图标列只显示缩略图（不含原始id），标题置顶，
列宽/行高按内容自适应，单元格内容居中。
"""
import json, sys, os
from PIL import Image, ImageDraw, ImageFont

def load_font(size):
    for name in ['msyh.ttc', 'simhei.ttf', 'simsun.ttc', 'arial.ttf']:
        try:
            return ImageFont.truetype(name, size)
        except Exception:
            continue
    return ImageFont.load_default()

def te(d, s, f):
    try:
        return d.textlength(str(s), font=f)
    except Exception:
        return len(str(s)) * 8

def wrap_centered(d, box, text, font, color=(20, 20, 20)):
    x0, y0, x1, y1 = box
    w = x1 - x0
    text = str(text)
    if not text:
        return
    words = text
    # 按字符换行
    lines = []
    cur = ''
    for ch in words:
        if te(d, cur + ch, font) <= w - 12:
            cur += ch
        else:
            if cur:
                lines.append(cur)
            cur = ch
    if cur:
        lines.append(cur)
    lh = te(d, 'Ag', font)
    total_h = len(lines) * lh
    y = (y0 + y1) / 2 - total_h / 2
    for ln in lines:
        lw = te(d, ln, font)
        d.text(((x0 + x1) / 2 - lw / 2, y), ln, fill=color, font=font)
        y += lh

def main():
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    cols = data['columns']
    rows = data['rows']
    pad = 6
    header_h = 36
    row_h_base = 30
    MIN_ICON = 40
    font = load_font(13)
    font_s = load_font(11)

    header = ['Row'] + [c['name'] for c in cols]
    # 测量用 draw：文本宽测量不依赖最终画布
    measure = ImageDraw.Draw(Image.new('RGB', (1, 1)))

    # 预解析行内容 / 图标
    rows_dec = []
    for row in rows:
        texts = []
        icons = []
        for ci, cell in enumerate(row['cells']):
            if isinstance(cell, dict) and 'value' in cell:
                texts.append(cell['value']); icons.append(None)
            else:
                texts.append(''); icons.append((cell or {}).get('icon_path'))
        rows_dec.append((row['row'], texts, icons))

    # 列宽自适应
    widths = [max(60, te(measure, header[0], font) + 16)]
    for ci in range(len(cols)):
        mx = te(measure, header[ci + 1], font) + 16
        for (_, texts, icons) in rows_dec:
            if ci < len(texts) and texts[ci]:
                mx = max(mx, te(measure, texts[ci], font) + 16)
            if ci < len(icons) and icons[ci] and os.path.exists(icons[ci]):
                pass
        mx = max(MIN_ICON if cols[ci].get('is_icon') else 0, mx)
        widths.append(mx)

    # 行高自适应（文本换行 + 图标）
    row_heights = []
    for (rid, texts, icons) in rows_dec:
        h = row_h_base
        # 图标行高：至少 40px 或图片实际高度
        for ci in range(len(cols)):
            if ci < len(icons) and icons[ci] and os.path.exists(icons[ci]):
                try:
                    ih = Image.open(icons[ci]).height
                    h = max(h, max(MIN_ICON, ih) + 8)
                except Exception:
                    pass
            if ci < len(texts) and texts[ci]:
                # 该列可用宽内可放字符数
                avail = widths[ci + 1] - 12
                cw = te(measure, 'x', font_s) or 1
                chars_per_line = max(1, int(avail / cw))
                lines = max(1, -(-len(str(texts[ci])) // chars_per_line))
                h = max(h, lines * (te(measure, 'Ag', font_s) + 4) + 8)
        row_heights.append(h)

    widths = [int(round(w)) for w in widths]
    row_heights = [int(round(h)) for h in row_heights]
    W = int(sum(widths) + pad * (len(widths) + 1))
    H = int(pad + 26 + header_h + pad + sum(row_heights) + pad * len(rows) + pad)
    img = Image.new('RGB', (W, H), 'white')
    d = ImageDraw.Draw(img)

    title = data['sheet_name']
    if data['total_pages'] > 1:
        title += f"  (第 {data['page'] + 1}/{data['total_pages']} 页)"
    # 标题置顶
    d.text((pad + 2, pad), title, fill=(40, 90, 160), font=load_font(15))
    y = pad + 26

    # 表头
    x = pad
    for ci, w in enumerate(widths):
        d.rectangle([x, y, x + w, y + header_h], fill=(40, 90, 160))
        d.text((x + w / 2, y + header_h / 2), str(header[ci]), fill='white', font=font, anchor='mm')
        x += w + pad
    y += header_h + pad

    # 行
    for ri, (rid, texts, icons) in enumerate(rows_dec):
        h = row_heights[ri]
        x = pad
        # Row 列（居中）
        d.rectangle([x, y, x + widths[0], y + h], fill=(245, 248, 252), outline=(200, 200, 200))
        d.text((x + widths[0] / 2, y + h / 2), str(rid), fill=(20, 20, 20), font=font_s, anchor='mm')
        x += widths[0] + pad
        for ci in range(len(cols)):
            w = widths[ci + 1]
            icon = icons[ci] if ci < len(icons) else None
            is_icon_col = cols[ci].get('is_icon')
            if is_icon_col and icon and os.path.exists(icon):
                d.rectangle([x, y, x + w, y + h], fill=(255, 255, 238), outline=(200, 200, 200))
                try:
                    im = Image.open(icon).convert('RGBA')
                    im.thumbnail((max(MIN_ICON, w - 8), max(MIN_ICON, h - 8)))
                    ox = x + (w - im.width) // 2
                    oy = y + (h - im.height) // 2
                    img.paste(im, (ox, oy), im)
                except Exception:
                    pass
            else:
                txt = texts[ci] if ci < len(texts) else ''
                d.rectangle([x, y, x + w, y + h], fill='white', outline=(200, 200, 200))
                wrap_centered(d, (x, y, x + w, y + h), txt, font_s)
            x += w + pad
        y += h + pad

    img.save(out)
    print("saved", out)

main()
