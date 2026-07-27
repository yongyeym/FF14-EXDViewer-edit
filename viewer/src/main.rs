#![allow(dead_code)]
#![warn(
    clippy::all,
    rust_2018_idioms,
    rust_2021_compatibility,
    rust_2024_compatibility
)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod combined_log;

#[cfg(target_arch = "wasm32")]
mod shortcuts;

use combined_log::CombinedLogger;
use ff14_exdviewer_edit::App;

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    CombinedLogger(
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("debug")).build(),
        egui_logger::builder()
            .max_level(log::LevelFilter::Debug)
            .build(),
    )
    .init();
    log::set_max_level(log::LevelFilter::Debug);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1920.0, 1080.0])
            .with_min_inner_size([1024.0, 768.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(&include_bytes!("../assets/icon.png")[..])
                    .expect("Failed to load icon"),
            ),
        persistence_path: {
            let exe_path = std::env::current_exe().ok();
            let exe_parent = exe_path.as_ref().and_then(|p| p.parent());
            let ron_path = exe_parent
                .map(|p| p.join("config").join("app.ron"))
                .unwrap_or_else(|| std::path::PathBuf::from("config/app.ron"));

            log::info!("Settings file: {}", ron_path.display());

            // Clean up: if a stale file named "config" exists at exe dir, remove it
            if let Some(parent) = ron_path.parent() {
                let stale = parent.join("config");
                if stale.is_file() {
                    log::warn!("Removing stale config file: {}", stale.display());
                    let _ = std::fs::remove_file(&stale);
                }
            }

            // Ensure parent directory exists
            if let Some(parent) = ron_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::error!("Failed to create config dir '{}': {e}", parent.display());
                }
            }

            // Also pre-create export subdirectories alongside exe
            if let Some(parent) = exe_parent {
                for sub in &["export/data", "export/music", "export/img"] {
                    let sub_path = parent.join(sub);
                    let _ = std::fs::create_dir_all(&sub_path);
                }
            }

            Some(ron_path)
        },
        ..Default::default()
    };
    eframe::run_native(
        "FF14 EXDViewer edit",
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    CombinedLogger(
        eframe::WebLogger::new(log::LevelFilter::Debug),
        egui_logger::builder().build(),
    )
    .init();
    log::set_max_level(log::LevelFilter::Info);

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let runner = eframe::WebRunner::new();

        let start_result = async {
            runner
                .start(
                    canvas.clone(),
                    web_options,
                    Box::new(|cc| Ok(Box::new(App::new(cc)))),
                )
                .await?;

            // Override certain key handling to prevent browser defaults.
            runner.add_event_listener(
                &canvas,
                "keydown",
                move |event: web_sys::KeyboardEvent, _| {
                    #[allow(clippy::wildcard_imports)]
                    use crate::shortcuts::*;

                    // https://github.com/emilk/egui/blob/802d307e4a2835cf4cf184d1cc99bea525b0c959/crates/eframe/src/web/input.rs#L152
                    let modifiers = egui::Modifiers {
                        alt: event.alt_key(),
                        ctrl: event.ctrl_key(),
                        shift: event.shift_key(),
                        mac_cmd: event.meta_key(),
                        command: event.ctrl_key() || event.meta_key(),
                    };
                    let key = egui::Key::from_name(&event.key());
                    if let Some(key) = key {
                        for shortcut in &[GOTO_ROW, GOTO_SHEET] {
                            if modifiers.matches_logically(shortcut.modifiers)
                                && key == shortcut.logical_key
                            {
                                event.prevent_default(); // Prevent browser default
                            }
                        }
                    }
                },
            )
        }
        .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(()) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
