"""把用户提供的 GIF 动作制作成 Hermes pet spritesheet。

规格:
- 8 行 legacy 格式 (idle, wave, run, failed, review, jump, extra1, extra2)
- 每帧 192x208, 每行 6 帧动画 + 2 列透明 padding
- 总尺寸: 8列 x 8行 = 1536 x 1664
- 均匀抽帧覆盖完整动作循环
"""
import glob
import json
import os
import sys

from PIL import Image

SRC_DIR = r"F:/桌面/Remielle"
OUT_DIR = r"E:/Workspaces2C/FF14_EXDViewer_edit/.hermes-pet-build"
FRAME_W, FRAME_H = 192, 208
FRAMES_PER_STATE = 6
COLS = 8
ROWS = 8

# 行序: legacy 8 行格式, 与用户 GIF 名一致
ROW_ORDER = ["idle", "wave", "run", "failed", "review", "jump", "extra1", "extra2"]


def load_gif_frames(path: str) -> list[Image.Image]:
    """读取 GIF 所有帧, 正确合成 disposal 后的完整 RGBA 帧。"""
    img = Image.open(path)
    frames = []
    try:
        while True:
            img.seek(len(frames))
            frames.append(img.convert("RGBA"))
    except EOFError:
        pass
    return frames


def pick_frames(n_frames: int, want: int = FRAMES_PER_STATE) -> list[int]:
    """均匀采样 want 帧索引, 覆盖整个动画循环 (含首尾)。"""
    if n_frames <= want:
        return list(range(n_frames))
    return [round(i * (n_frames - 1) / (want - 1)) for i in range(want)]


def fit_frame(frame: Image.Image) -> Image.Image:
    """等比缩放(最近邻, 保持像素画锐利)到 192x208 画布内居中。"""
    canvas = Image.new("RGBA", (FRAME_W, FRAME_H), (0, 0, 0, 0))
    # 等比 fit, 留 8px 边距避免贴边
    scale = min((FRAME_W - 16) / frame.width, (FRAME_H - 16) / frame.height)
    new_w = max(1, round(frame.width * scale))
    new_h = max(1, round(frame.height * scale))
    resized = frame.resize((new_w, new_h), Image.NEAREST)
    x = (FRAME_W - new_w) // 2
    y = (FRAME_H - new_h) // 2
    canvas.paste(resized, (x, y), resized)
    return canvas


def main() -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    gif_files = {os.path.splitext(os.path.basename(f))[0].lower(): f
                 for f in glob.glob(os.path.join(SRC_DIR, "*.gif"))}

    sheet = Image.new("RGBA", (COLS * FRAME_W, ROWS * FRAME_H), (0, 0, 0, 0))

    print("=== 抽帧计划 ===")
    for row_idx, state in enumerate(ROW_ORDER):
        if state.startswith("extra"):
            continue  # 扩展行留空(透明 padding)
        gif = gif_files.get(state)
        if not gif:
            print(f"  [警告] 缺少 {state}.gif, 该行留空")
            continue
        frames = load_gif_frames(gif)
        picks = pick_frames(len(frames))
        print(f"  {state}: {len(frames)}帧 -> {len(picks)}帧 {picks}")
        for col_idx, frame_idx in enumerate(picks):
            cell = fit_frame(frames[frame_idx])
            x = col_idx * FRAME_W
            y = row_idx * FRAME_H
            sheet.paste(cell, (x, y), cell)

    # 保存 spritesheet
    sheet_path = os.path.join(OUT_DIR, "spritesheet.webp")
    sheet.save(sheet_path, "WEBP", lossless=True)
    print(f"\nspritesheet: {sheet.size} -> {sheet_path}")

    # pet.json
    pet_json = {
        "id": "Remielle",
        "displayName": "蕾米埃尔Remielle",
        "description": "绝区零ZZZ Q版画画蕾米",
        "spritesheetPath": "spritesheet.webp",
    }
    json_path = os.path.join(OUT_DIR, "pet.json")
    with open(json_path, "w", encoding="utf-8") as f:
        json.dump(pet_json, f, ensure_ascii=False, indent=2)
    print(f"pet.json -> {json_path}")


if __name__ == "__main__":
    main()
