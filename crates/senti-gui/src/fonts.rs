//! Load a system CJK font so Traditional Chinese renders (egui's built-in
//! fonts only cover Latin + emoji).

use std::sync::Arc;

use eframe::egui::{self, FontData, FontFamily, FontInsert, FontPriority, InsertFontFamily};

/// Environment variable to point at a specific `.ttf/.otf/.ttc` file.
pub const FONT_ENV: &str = "SENTI_IME_FONT";

/// `(path, face index)` candidates; index 3 of Noto CJK `.ttc` is TC.
const CANDIDATES: &[(&str, u32)] = &[
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 3),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 3),
    (
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        3,
    ),
    ("/usr/share/fonts/noto-cjk/NotoSansTC-Regular.otf", 0),
    (
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        0,
    ),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
    ("/System/Library/Fonts/PingFang.ttc", 0),
    ("C:\\Windows\\Fonts\\msjh.ttc", 0),
];

/// Install the first CJK font found. Returns the path used, if any.
pub fn install_cjk_font(ctx: &egui::Context) -> Option<String> {
    let custom = std::env::var(FONT_ENV).ok().map(|p| (p, 0));
    let candidates = custom
        .into_iter()
        .chain(CANDIDATES.iter().map(|(p, i)| ((*p).to_owned(), *i)));
    for (path, index) in candidates {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        ctx.add_font(FontInsert {
            name: "senti-cjk".to_owned(),
            data: FontData::from_blob(Arc::new(bytes), index),
            families: [FontFamily::Proportional, FontFamily::Monospace]
                .into_iter()
                .map(|family| InsertFontFamily {
                    family,
                    priority: FontPriority::Lowest,
                })
                .collect(),
        });
        return Some(path);
    }
    None
}
