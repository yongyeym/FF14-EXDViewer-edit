use egui::{Context, Id, Image, ImageSource, Layout, Modal, Sense, SizeHint, Spinner, TextStyle, UiBuilder};
use egui::load::ImagePoll;

use super::ManagedIcon;


/// Show `icon` over the whole app. Returns true once it has been dismissed.
pub fn icon_modal(ctx: &Context, icon_id: u32, icon: ManagedIcon) -> bool {
    Modal::new(Id::new("icon-modal"))
        .area(Modal::default_area(Id::new(format!(
            "icon-modal-{icon_id}"
        ))))
        .show(ctx, |ui| match icon {
            ManagedIcon::Loaded(source) => {
                let display = preview_size(ctx, &source);
                ui.add(Image::new(source).fit_to_exact_size(display))
            }
            ManagedIcon::Failed(e) => {
                ui.label("图片加载失败").on_hover_text(e.to_string())
            }
            ManagedIcon::Loading => {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(rect)
                        .layout(Layout::centered_and_justified(ui.layout().main_dir())),
                    |ui| {
                        ui.add(Spinner::new().size(ui.text_style_height(&TextStyle::Heading) * 3.0))
                    },
                )
                .inner
            }
            ManagedIcon::NotLoaded => ui.label("图片未加载"),
        })
        .should_close()
}

/// 预览显示尺寸：小图按原始分辨率；大图等比缩放到最大尺寸内。
fn preview_size(ctx: &Context, source: &ImageSource<'static>) -> egui::Vec2 {
    // 最大尺寸：程序主窗口分辨率的 80%
    let max_preview = ctx.viewport_rect().size() * 0.8;
    let natural = match source {
        ImageSource::Texture(texture) => {
            Some(egui::vec2(texture.size.x as f32, texture.size.y as f32))
        }
        ImageSource::Uri(uri) => match ctx.try_load_image(uri, SizeHint::Scale(1.0.into())) {
            Ok(ImagePoll::Ready { image }) => {
                Some(egui::vec2(image.size[0] as f32, image.size[1] as f32))
            }
            _ => None,
        },
        _ => None,
    };
    let Some(natural) = natural else {
        return max_preview;
    };
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return max_preview;
    }
    if natural.x <= max_preview.x && natural.y <= max_preview.y {
        // 原始图片较小：按原始分辨率显示
        natural
    } else {
        // 原始图片较大：等比缩放到最大尺寸内
        let scale = (max_preview.x / natural.x).min(max_preview.y / natural.y);
        natural * scale
    }
}
