//! 中文字体：egui 自带字体没有 CJK，缺字会渲染成方块。
//!
//! 用 fontdb 从系统里挑一款覆盖中文的字体追加到字体族末尾作为回退：
//! 拉丁字符仍走 egui 默认字体，只有缺字时才落到中文字体。
//! macOS 上通常是 PingFang SC（TTC 集合，必须带 face index）。

use eframe::egui;

/// 注入成功返回字体族名，失败返回 None（界面仍可用，只是中文可能显示为方块）。
pub fn install(ctx: &egui::Context) -> Option<&'static str> {
    let (family, data, index) = load_system_cjk()?;
    let mut fonts = egui::FontDefinitions::default();
    let mut font = egui::FontData::from_owned(data);
    font.index = index;
    fonts
        .font_data
        .insert("cjk".to_owned(), std::sync::Arc::new(font));
    for family_id in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family_id)
            .or_default()
            .push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
    tracing::info!(family, index, "已注入中文字体");
    Some(family)
}

fn load_system_cjk() -> Option<(&'static str, Vec<u8>, u32)> {
    // 顺序即优先级：先无衬线正文，再黑体/衬线/Unicode 兜底。
    // Windows 优先雅黑/等线，避免落到字形较旧的 Arial Unicode MS；
    // macOS 仍以苹方为首选；Linux 兜底 Noto/思源。
    const CANDIDATES: [&str; 12] = [
        "PingFang SC",
        "Hiragino Sans GB",
        "Heiti SC",
        "STHeiti",
        "Songti SC",
        "Microsoft YaHei UI",
        "Microsoft YaHei",
        "DengXian",
        "Noto Sans CJK SC",
        "Source Han Sans SC",
        "Arial Unicode MS",
        "SimSun",
    ];

    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    for family in CANDIDATES {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let Some(id) = db.query(&query) else { continue };
        let mut face = None;
        db.with_face_data(id, |data, index| face = Some((data.to_vec(), index)));
        if let Some((data, index)) = face {
            return Some((family, data, index));
        }
    }
    tracing::warn!("系统里没找到中文字体，中文可能显示为方块");
    None
}
