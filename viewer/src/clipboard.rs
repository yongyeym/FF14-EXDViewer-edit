//! 系统剪贴板图片写入（`arboard` 第三方库，支持 RGBA 图片）。

use image::RgbaImage;

/// 把 RGBA 图片写入系统剪贴板，供用户粘贴到其他程序。
/// 返回是否成功。
pub fn copy_rgba(rgba: RgbaImage) -> bool {
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let data = arboard::ImageData {
        width: w,
        height: h,
        bytes: std::borrow::Cow::Owned(rgba.into_raw()),
    };
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_image(data)) {
        Ok(_) => true,
        Err(e) => {
            log::error!("写入剪贴板失败: {e}");
            false
        }
    }
}
