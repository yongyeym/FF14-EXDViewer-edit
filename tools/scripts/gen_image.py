"""FF14 EXDViewer 数据表内容Diff总结表 - 图片生成工具（matplotlib）。
用法: gen_image_tool.exe <json> <out.png>
"""
import json, sys
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib import font_manager

def setup_font():
    for name in ['Microsoft YaHei', 'SimHei', 'Noto Sans CJK SC', 'Noto Sans SC', 'Songti SC', 'sans-serif']:
        try:
            font_manager.findfont(name, fallback_to_default=False)
            plt.rcParams['font.family'] = [name]
            break
        except Exception:
            continue
    plt.rcParams['axes.unicode_minus'] = False

def main():
    setup_font()
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    cols = [c['name'] for c in data['columns']]
    rows = data['rows']
    cells = []
    for row in rows:
        line = [str(row['row'])]
        for cell in row['cells']:
            if isinstance(cell, dict) and 'value' in cell:
                line.append(str(cell['value']))
            else:
                line.append(cell.get('id', ''))
        cells.append(line)
    header = ['Row'] + cols
    fig_w = max(10, len(header) * 2.2)
    fig_h = max(3, (len(cells) + 1) * 0.45 + 1.2)
    fig, ax = plt.subplots(figsize=(fig_w, fig_h))
    ax.axis('off')
    title = data['sheet_name'] + (f"  (第 {data['page']+1}/{data['total_pages']} 页)" if data['total_pages'] > 1 else "")
    ax.set_title(title, fontsize=12, pad=8)
    tbl = ax.table(cellText=cells, colLabels=header, loc='center', cellLoc='left')
    tbl.auto_set_font_size(False)
    tbl.set_fontsize(8)
    tbl.scale(1, 1.2)
    fig.savefig(out, dpi=150, bbox_inches='tight')
    print("saved", out)

main()
