"""FF14 EXDViewer 数据表内容Diff总结表 - 图片生成工具（PIL 绘制，自适应表格，嵌入图标）。
用法: gen_image_tool.exe <json> <out.png> [font_path]
不依赖本地 python（PyInstaller 打包）。图标列只显示缩略图（不含原始id），标题置顶，
列宽/行高按内容自适应，单元格内容居中。
[font_path] 由主程序传入（程序内嵌的同款 Noto Sans 字体），保证文本与程序界面字体一致；
未提供时回退到系统字体。
"""
import json, sys, os
from PIL import Image, ImageDraw, ImageFont

# 尺寸（整体放大，保证导出图片文本清晰可读）
FONT_MAIN = 20    # 表头 / Row 列
FONT_SMALL = 17   # 单元格正文
FONT_TITLE = 26   # 标题
HEADER_H = 56     # 表头行高
ROW_H_BASE = 44   # 行高基准
MIN_ICON = 64     # 图标缩略图最小边长
PAD = 10          # 单元格间距


def load_font(size, font_path=None):
    candidates = []
    if font_path:
        candidates.append(font_path)
    # 回退：系统中文字体
    candidates += ['msyh.ttc', 'msyh.ttf', 'simhei.ttf', 'simsun.ttc', 'arial.ttf']
    for name in candidates:
        try:
            return ImageFont.truetype(name, size)
        except Exception:
            continue
    try:
        return ImageFont.load_default(size)  # Pillow >= 10.1
    except Exception:
        return ImageFont.load_default()


def te(d, s, f):
    """文本宽度测量（像素）。"""
    try:
        return d.textlength(str(s), font=f)
    except Exception:
        return len(str(s)) * 8


def line_h(f):
    """字体行高（像素）——注意不能用 textlength（那是宽度）。"""
    try:
        asc, desc = f.getmetrics()
        return asc + desc
    except Exception:
        return getattr(f, 'size', 12) * 1.3


def wrap_lines(d, text, font, max_w):
    text = str(text)
    if not text:
        return []
    lines = []
    cur = ''
    for ch in text:
        if te(d, cur + ch, font) <= max_w:
            cur += ch
        else:
            if cur:
                lines.append(cur)
            cur = ch
    if cur:
        lines.append(cur)
    return lines


def wrap_centered(d, box, text, font, color=(20, 20, 20)):
    x0, y0, x1, y1 = box
    lines = wrap_lines(d, text, font, x1 - x0 - 16)
    if not lines:
        return
    lh = line_h(font)
    total_h = len(lines) * lh
    y = (y0 + y1) / 2 - total_h / 2
    for ln in lines:
        lw = te(d, ln, font)
        d.text(((x0 + x1) / 2 - lw / 2, y), ln, fill=color, font=font)
        y += lh


def main():
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    font_path = sys.argv[3] if len(sys.argv) > 3 else None
    cols = data['columns']
    rows = data['rows']

    font = load_font(FONT_MAIN, font_path)
    font_s = load_font(FONT_SMALL, font_path)
    font_t = load_font(FONT_TITLE, font_path)

    header = ['Row'] + [c['name'] for c in cols]
    # 测量用 draw：文本宽测量不依赖最终画布
    measure = ImageDraw.Draw(Image.new('RGB', (1, 1)))

    # 预解析行内容 / 图标
    rows_dec = []
    for row in rows:
        texts = []
        icons = []
        for cell in row['cells']:
            if isinstance(cell, dict) and 'value' in cell:
                texts.append(cell['value']); icons.append(None)
            else:
                texts.append(''); icons.append((cell or {}).get('icon_path'))
        rows_dec.append((row['row'], texts, icons))

    # 列宽自适应
    widths = [max(70, te(measure, header[0], font) + 20)]
    for ci in range(len(cols)):
        mx = te(measure, header[ci + 1], font) + 20
        for (_, texts, _icons) in rows_dec:
            if ci < len(texts) and texts[ci]:
                # 长文本按最长单词/行内换行量估算，限制单列最大宽度
                mx = max(mx, min(te(measure, texts[ci], font_s) + 20, 480))
        mx = max(MIN_ICON + 12 if cols[ci].get('is_icon') else 0, mx)
        widths.append(mx)

    # 行高自适应（文本换行 + 图标）
    row_heights = []
    for (_rid, texts, icons) in rows_dec:
        h = ROW_H_BASE
        for ci in range(len(cols)):
            if ci < len(icons) and icons[ci] and os.path.exists(icons[ci]):
                h = max(h, MIN_ICON + 12)
            if ci < len(texts) and texts[ci]:
                avail = widths[ci + 1] - 16
                lines = max(1, len(wrap_lines(measure, texts[ci], font_s, avail)))
                h = max(h, lines * (line_h(font_s) + 6) + 10)
        row_heights.append(h)

    widths = [int(round(w)) for w in widths]
    row_heights = [int(round(h)) for h in row_heights]
    W = int(sum(widths) + PAD * (len(widths) + 1))
    title_h = line_h(font_t) + 12
    H = int(PAD + title_h + HEADER_H + PAD + sum(row_heights) + PAD * len(rows) + PAD)
    img = Image.new('RGB', (W, H), 'white')
    d = ImageDraw.Draw(img)

    title = data['sheet_name']
    if data['total_pages'] > 1:
        title += f"  (第 {data['page'] + 1}/{data['total_pages']} 页)"
    # 标题置顶
    d.text((PAD + 2, PAD), title, fill=(40, 90, 160), font=font_t)
    y = PAD + title_h

    # 表头
    x = PAD
    for ci, w in enumerate(widths):
        d.rectangle([x, y, x + w, y + HEADER_H], fill=(40, 90, 160))
        d.text((x + w / 2, y + HEADER_H / 2), str(header[ci]), fill='white', font=font, anchor='mm')
        x += w + PAD
    y += HEADER_H + PAD

    # 行
    for ri, (rid, texts, icons) in enumerate(rows_dec):
        h = row_heights[ri]
        x = PAD
        # Row 列（居中）
        d.rectangle([x, y, x + widths[0], y + h], fill=(245, 248, 252), outline=(200, 200, 200))
        d.text((x + widths[0] / 2, y + h / 2), str(rid), fill=(20, 20, 20), font=font, anchor='mm')
        x += widths[0] + PAD
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
            x += w + PAD
        y += h + PAD

    img.save(out)
    print("saved", out)


main()
