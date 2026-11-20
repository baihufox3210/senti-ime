//! The demo window: a mock "host application" document on the right,
//! controls for context, plugins and learning on the left.

use eframe::egui::{self, Color32, FontId, RichText, Stroke, text::LayoutJob};
use senti_app::{AppOptions, Session, build_engine};
use senti_core::{AppKind, Formality, InputMode, Sentiment, TimeOfDay};
use senti_engine::{EngineView, KeyEvent, KeyOutcome};

use crate::fonts;
use crate::keys::{self, ShiftTap};

const ACCENT: Color32 = Color32::from_rgb(64, 156, 255);

/// Sample documents that put the engine into different contexts.
const SAMPLES: &[(&str, AppKind, &str)] = &[
    ("開心聊天", AppKind::Chat, "今天考試考得很好，好開心哈哈！"),
    ("低落聊天", AppKind::Chat, "唉，今天好累，心情有點難過…"),
    ("正式郵件", AppKind::Email, "敬啟者：\n您好，"),
    ("寫程式", AppKind::Code, "fn main() {\n    // "),
];

pub struct SentiApp {
    session: Session,
    shift: ShiftTap,
    show_scores: bool,
    settings_open: bool,
    theme: egui::ThemePreference,
    status: String,
    font_warning: Option<String>,
    data_path: Option<String>,
    memory_scope_edit: String,
}

impl SentiApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let font_warning = match fonts::install_cjk_font(&cc.egui_ctx) {
            Some(_) => None,
            None => Some(format!(
                "找不到中文字型，請以 {} 環境變數指定字型檔路徑。",
                fonts::FONT_ENV
            )),
        };
        cc.egui_ctx.set_theme(egui::ThemePreference::System);
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            cc.egui_ctx.style_mut_of(theme, |style| {
                style.spacing.item_spacing = egui::vec2(12.0, 12.0);
                style.spacing.button_padding = egui::vec2(14.0, 9.0);
                style.spacing.interact_size.y = 36.0;
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, FontId::proportional(16.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, FontId::proportional(16.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Heading, FontId::proportional(24.0));
            });
        }
        let options = AppOptions::default();
        let data_path = options
            .persistence
            .model_path()
            .map(|p| p.display().to_string());
        let mut engine = build_engine(options);
        if engine.memory_scope() == "default" {
            let _ = engine.set_memory_scope("personal");
        }
        let session = Session::new(engine);
        let status = session
            .engine()
            .last_error()
            .map(str::to_owned)
            .unwrap_or_else(|| "就緒：中英混打 · su3cl3 → 你好 · hello → hello。".to_owned());
        let memory_scope_edit = session.engine().memory_scope().to_owned();
        Self {
            session,
            memory_scope_edit,
            shift: ShiftTap::default(),
            show_scores: false,
            settings_open: false,
            theme: egui::ThemePreference::System,
            status,
            font_warning,
            data_path,
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let actions = keys::collect(ctx, &mut self.shift);
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        for action in actions {
            match action {
                keys::InputAction::Paste(text) => {
                    self.session.paste_text(&text);
                    self.status = "已貼上文字".into();
                }
                keys::InputAction::Copy => self.copy_document(ctx),
                action => {
                    let view = self.session.engine().view();
                    if let Some(key) = action.key(view.mode, view.selecting) {
                        let outcome = self.session.press(key);
                        self.after(key_label(key), outcome);
                    }
                }
            }
        }
    }

    fn copy_document(&mut self, ctx: &egui::Context) {
        if !self.session.document().is_empty() {
            ctx.copy_text(self.session.document().to_owned());
            self.status = "已複製全文".into();
        }
    }

    fn after(&mut self, action: String, outcome: KeyOutcome) {
        if let KeyOutcome::Commit(text) = outcome {
            self.status = format!("{action} → 上屏「{}」", text.escape_debug());
        }
        if let Some(err) = self.session.engine().last_error() {
            self.status = err.to_owned();
        }
    }
}

fn key_label(key: KeyEvent) -> String {
    match key {
        KeyEvent::Char(c) => format!("按鍵 {c}"),
        other => format!("{other:?}"),
    }
}

impl eframe::App for SentiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render(ui);
    }
}

impl SentiApp {
    fn render(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.handle_keyboard(&ctx);
        let view = self.session.engine().view();

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui, &view));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&self.status).color(ui.visuals().weak_text_color()));
                if let Some(w) = &self.font_warning {
                    ui.colored_label(Color32::YELLOW, w);
                }
            });
        });
        let view = self.session.engine().view();
        if self.settings_open && ctx.content_rect().width() < 960.0 {
            let mut open = self.settings_open;
            egui::Window::new("設定")
                .open(&mut open)
                .default_width(320.0)
                .max_height((ctx.content_rect().height() - 80.0).max(200.0))
                .show(&ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui, &view));
                });
            self.settings_open = open;
        } else if self.settings_open {
            egui::Panel::left("controls")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui, &view));
                });
        }
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(20.0);
                let inset = ((ui.available_width() - 920.0) / 2.0).clamp(12.0, 120.0);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(inset as i8, 8))
                    .show(ui, |ui| self.editor(ui));
            });
        });
    }
}

// ----------------------------------------------------------------------
// Panels
// ----------------------------------------------------------------------
impl SentiApp {
    fn top_bar(&mut self, ui: &mut egui::Ui, view: &EngineView) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("SentiIME").size(25.0).strong().color(ACCENT));
            ui.label(RichText::new("讓想法，自然成字").color(ui.visuals().weak_text_color()));
            ui.separator();
            for (mode, label) in [
                (InputMode::Mixed, "中英混打"),
                (InputMode::Zhuyin, "純注音"),
                (InputMode::Chinese, "拼音"),
                (InputMode::English, "English"),
            ] {
                if ui.selectable_label(view.mode == mode, label).clicked() {
                    let outcome = self.session.set_mode(mode);
                    self.after(format!("切換至 {label}"), outcome);
                }
            }
            if ui.selectable_label(self.settings_open, "設定").clicked() {
                self.settings_open = !self.settings_open;
            }
            egui::ComboBox::from_id_salt("theme")
                .selected_text(match self.theme {
                    egui::ThemePreference::System => "跟隨系統",
                    egui::ThemePreference::Light => "淺色",
                    egui::ThemePreference::Dark => "深色",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.theme, egui::ThemePreference::System, "跟隨系統");
                    ui.selectable_value(&mut self.theme, egui::ThemePreference::Light, "淺色");
                    ui.selectable_value(&mut self.theme, egui::ThemePreference::Dark, "深色");
                });
            ui.ctx().set_theme(self.theme);
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui, view: &EngineView) {
        ui.add_space(12.0);
        ui.heading("偏好設定");
        ui.label(RichText::new("調整你的輸入體驗").color(ui.visuals().weak_text_color()));
        ui.add_space(8.0);
        egui::CollapsingHeader::new("個人記憶").show(ui, |ui| {
            let mut enabled = self.session.engine().memory_config_enabled();
            if ui.checkbox(&mut enabled, "使用本機記憶").changed() {
                self.session.engine_mut().set_memory_enabled(enabled);
            }
            ui.label("不同工作範圍分開保存；只記錄本輸入法提交的內容。");
            ui.text_edit_singleline(&mut self.memory_scope_edit);
            if ui.button("切換工作範圍").clicked() {
                self.status = match self
                    .session
                    .engine_mut()
                    .set_memory_scope(&self.memory_scope_edit)
                {
                    Ok(()) => "已切換記憶範圍".into(),
                    Err(e) => e,
                };
            }
            if !view.memory.persistent {
                ui.label("此工作階段的記憶只存在記憶體。");
            }
            let entries = self.session.engine().memory_page(0, 30);
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("memory-list")
                .max_height(180.0)
                .show(ui, |ui| {
                    for entry in entries.iter().take(30) {
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("刪除").clicked() {
                                remove = Some(entry.id);
                            }
                            ui.label(&entry.text);
                        });
                    }
                });
            if let Some(id) = remove {
                self.session.engine_mut().remove_memory(id);
            }
            if ui.button("清除這個範圍的記憶").clicked() {
                self.session.engine_mut().clear_memory();
            }
        });
        egui::CollapsingHeader::new("情境感知")
            .default_open(true)
            .show(ui, |ui| {
                let mut app = self.session.engine().app();
                egui::ComboBox::from_label("應用類型")
                    .selected_text(app.label())
                    .show_ui(ui, |ui| {
                        for kind in AppKind::ALL {
                            ui.selectable_value(&mut app, kind, kind.label());
                        }
                    });
                if app != self.session.engine().app() {
                    self.session.engine_mut().set_app(app);
                }
                ui.label(formality_label(view.context.formality));
                ui.label(sentiment_label(view.context.sentiment));
                ui.label(format!(
                    "{} · {}",
                    time_label(view.context.time_of_day),
                    view.context.now
                ));
                ui.label(
                    RichText::new("依最近文字、應用類型與時間調整候選排序。")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(4.0);
                for (name, app, text) in SAMPLES {
                    if ui.button(*name).clicked() {
                        self.session.engine_mut().set_app(*app);
                        self.session.set_document(*text);
                        self.status = format!("已載入範例「{name}」");
                    }
                }
            });
        ui.separator();
        ui.collapsing("插件", |ui| {
            for plugin in self.session.engine().plugins() {
                let mut enabled = plugin.enabled;
                if ui
                    .checkbox(&mut enabled, plugin.manifest.name)
                    .on_hover_text(plugin.manifest.description)
                    .changed()
                {
                    if let Err(err) = self
                        .session
                        .engine_mut()
                        .set_plugin_enabled(plugin.manifest.id, enabled)
                    {
                        self.status = err.to_string();
                    }
                }
            }
            ui.label(
                RichText::new("日期與計算機快捷碼可在混打或拼音模式使用。")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
        });
        ui.separator();
        ui.collapsing("個人化學習", |ui| {
            let stats = self.session.engine().user_stats();
            ui.label(format!(
                "舊詞庫詞語 {} · 歷史選字 {}",
                stats.distinct_words, stats.total_word_commits
            ));
            let mut learning = self.session.engine().learning_enabled();
            if ui.checkbox(&mut learning, "啟用學習").changed() {
                self.session.engine_mut().set_learning_enabled(learning);
            }
            if let Some(path) = &self.data_path {
                ui.label(
                    RichText::new(path)
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            if !self.session.engine().is_persistent() {
                ui.label("本次學習僅保留於記憶體");
            }
            if ui.button("立即儲存").clicked() {
                let saved = self
                    .session
                    .engine_mut()
                    .save_user_model()
                    .map_err(|e| e.to_string())
                    .and_then(|_| self.session.engine().flush_memory());
                self.status = match saved {
                    Ok(()) => "已儲存學習資料".into(),
                    Err(err) => format!("儲存失敗：{err}"),
                };
            }
            if ui.button("清除學習資料").clicked() {
                self.status = match self.session.engine_mut().reset_user_model() {
                    Ok(()) => "已清除學習資料".into(),
                    Err(err) => format!("清除失敗：{err}"),
                };
            }
        });
        ui.separator();
        ui.collapsing("進階資訊", |ui| {
            ui.label(self.session.engine().predictor_ids().join(" · "));
            ui.checkbox(&mut self.show_scores, "顯示候選分數與來源");
        });
        ui.separator();
        ui.collapsing("操作說明", |ui| {
            for line in [
                "混打：英文與注音共用一個輸入列",
                "su3cl3 → 你好，hello 保留英文",
                "混打 Enter 確認；Ctrl／Cmd+Enter 保留原文",
                "混打空白：英文加空白；注音一聲／選字",
                "注音：標準大千鍵盤，空白為一聲",
                "6／3／4／7：二聲／三聲／四聲／輕聲",
                "連按聲調鍵可更正上一音節聲調",
                "Ctrl／Cmd+C 複製，Ctrl／Cmd+V 貼上",
                "↓ 開啟選字，再以 1–9 選擇",
                "選字時方向鍵移動，PageUp／Down 翻頁",
                "Enter 確認整句；Esc 返回／取消",
                "Shift 單擊或 Tab：中英切換",
                "拼音／英文：空白選字或上屏",
                "拼音 Enter：上屏原始字母",
                "混打／拼音 rq／sj／xq：日期、時間、星期",
                "混打／拼音 v1+2*3：計算機",
            ] {
                ui.label(line);
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("輸入體驗區")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.horizontal_wrapped(|ui| {
            ui.heading("寫下你的想法");
            if self.session.engine().mode() == InputMode::Mixed
                && self.session.engine().view().composing
            {
                if ui.button("確認候選").clicked() {
                    let outcome = self.session.press(KeyEvent::Enter);
                    self.after("確認".into(), outcome);
                }
                if ui
                    .button("保留原文")
                    .on_hover_text("Ctrl／Cmd+Enter：保留所有原始鍵碼")
                    .clicked()
                {
                    let outcome = self.session.press(KeyEvent::CommitRaw);
                    self.after("保留原文".into(), outcome);
                }
            }
            if ui
                .add_enabled(
                    !self.session.document().is_empty(),
                    egui::Button::new("複製全文"),
                )
                .clicked()
            {
                self.copy_document(ui.ctx());
            }
            if ui.button("清空").clicked() {
                self.session.set_document("");
                self.status = "已清空文件與組字".into();
            }
        });
        let view = self.session.engine().view();
        ui.label(
            RichText::new(match view.mode {
                InputMode::Mixed => "su3cl3 → 你好 · hello 保留英文 · ↓ 選字 · Enter 確認",
                InputMode::Zhuyin => "標準注音 · 空白一聲 · ↓ 選字 · Enter 確認整句",
                _ => "直接輸入 · 空白選字 · Shift 切換中英文",
            })
            .color(ui.visuals().weak_text_color()),
        );
        let font = FontId::proportional(26.0);
        let text_color = ui.visuals().text_color();

        let mut job = LayoutJob::default();
        job.wrap.max_width = ui.available_width() - 24.0;
        let plain = egui::TextFormat::simple(font.clone(), text_color);
        job.append(self.session.document(), 0.0, plain.clone());
        if view.composing {
            job.append(
                &view.converted_preedit,
                0.0,
                egui::TextFormat {
                    color: ACCENT,
                    underline: Stroke::new(1.5, ACCENT),
                    ..plain.clone()
                },
            );
        }
        job.append("▏", 0.0, egui::TextFormat::simple(font, ACCENT));

        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .stroke(Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .corner_radius(16)
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.set_min_height(220.0);
                if self.session.document().is_empty() && !view.composing {
                    ui.label(
                        RichText::new("直接輸入，讓想法成為文字。")
                            .color(ui.visuals().weak_text_color()),
                    );
                }
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .max_height(320.0)
                    .show(ui, |ui| ui.label(job));
            });

        if let Some(phonetic) = &view.phonetic_preview {
            ui.label(RichText::new(format!("注音解讀  {phonetic}")).color(ACCENT));
        }
        ui.add_space(8.0);
        self.candidate_bar(ui, &view);
        if view.mode.uses_candidate_selection() {
            ui.add_space(12.0);
            ui.collapsing("標準注音鍵盤", |ui| {
                for row in ["1234567890-", "qwertyuiop", "asdfghjkl;", "zxcvbnm,./"] {
                    ui.horizontal_wrapped(|ui| {
                        for key in row.chars() {
                            let label = if let Some(symbol) = senti_core::zhuyin::symbol(key) {
                                format!("{key}  {symbol}")
                            } else {
                                format!(
                                    "{key}  {}",
                                    match key {
                                        '6' => "ˊ",
                                        '3' => "ˇ",
                                        '4' => "ˋ",
                                        '7' => "˙",
                                        _ => "",
                                    }
                                )
                            };
                            ui.label(RichText::new(label).monospace());
                        }
                    });
                }
                ui.label("空白：一聲　｜　Shift：中英切換　｜　Tab：中英切換");
            });
        }
    }

    fn candidate_bar(&mut self, ui: &mut egui::Ui, view: &EngineView) {
        if !view.selecting {
            if let Some(suggestion) = &view.suggestion {
                ui.label(
                    RichText::new(format!("預選：{suggestion}（↓ 選取）"))
                        .italics()
                        .color(ui.visuals().weak_text_color()),
                );
            }
            if !view.candidates.is_empty() {
                ui.label("↓ 開啟候選選單");
            }
            return;
        }
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(12)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let title = if view.association {
                    "聯想（下一個詞）"
                } else if view.composing {
                    "候選詞"
                } else {
                    "候選詞（開始輸入以顯示）"
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(title).strong());
                    if view.mode.uses_candidate_selection() && view.composing {
                        ui.label(
                            RichText::new(if view.selecting {
                                "1–9 選字 · Esc 返回組字"
                            } else {
                                "↓ 開啟選字"
                            })
                            .small()
                            .color(ui.visuals().weak_text_color()),
                        );
                    }
                    if view.page_count > 1 {
                        if ui.small_button("上一頁").clicked() {
                            self.session.press(KeyEvent::PageUp);
                        }
                        if ui.small_button("下一頁").clicked() {
                            self.session.press(KeyEvent::PageDown);
                        }
                    }
                    if view.page_count > 1 {
                        ui.label(
                            RichText::new(format!("第 {}/{} 頁", view.page + 1, view.page_count))
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
                if view.composing && view.candidates.is_empty() {
                    ui.label("沒有符合的候選，請修改注音或聲調；Esc 可取消組字。");
                }
                let mut clicked = None;
                ui.horizontal_wrapped(|ui| {
                    for c in &view.candidates {
                        let mut text = RichText::new(format!("{}. {}", c.label, c.text)).size(20.0);
                        if if view.selecting {
                            c.label == view.candidate_cursor + 1
                        } else {
                            c.label == 1
                        } {
                            text = text.color(ACCENT);
                        }
                        let mut hover = format!("來源：{}", c.source);
                        if let Some(note) = &c.annotation {
                            hover.push_str(&format!("　注音／編碼：{note}"));
                        }
                        if c.partial {
                            hover.push_str("　（部分轉換）");
                        }
                        ui.vertical(|ui| {
                            if view.mode == InputMode::Mixed {
                                let language = if c.source == "raw" {
                                    "原文"
                                } else if matches!(
                                    c.source.as_str(),
                                    "emoji" | "datetime" | "calculator"
                                ) {
                                    "插件"
                                } else {
                                    c.input_mode.map_or("候選", InputMode::short_label)
                                };
                                ui.label(
                                    RichText::new(language)
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                );
                            }
                            if ui
                                .add(egui::Button::new(text).selected(
                                    view.selecting && c.label == view.candidate_cursor + 1,
                                ))
                                .on_hover_text(hover)
                                .clicked()
                            {
                                clicked = Some(c.label);
                            }
                            if let Some(note) = &c.annotation {
                                ui.label(
                                    RichText::new(if c.partial {
                                        format!("{note} · 分段")
                                    } else {
                                        note.clone()
                                    })
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                                );
                            }
                            if self.show_scores {
                                ui.label(
                                    RichText::new(format!("{} {:.2}", c.source, c.score))
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                );
                            }
                        });
                    }
                });
                if let Some(label) = clicked {
                    let outcome = self.session.select(label);
                    self.after(format!("點選 {label}"), outcome);
                }
            });
    }
}

fn formality_label(f: Formality) -> &'static str {
    match f {
        Formality::Casual => "輕鬆 Casual",
        Formality::Neutral => "一般 Neutral",
        Formality::Formal => "正式 Formal",
    }
}

fn sentiment_label(s: Sentiment) -> &'static str {
    match s {
        Sentiment::Positive => "正面 😊",
        Sentiment::Neutral => "中性",
        Sentiment::Negative => "負面 😢",
    }
}

fn time_label(t: TimeOfDay) -> &'static str {
    match t {
        TimeOfDay::Morning => "早上",
        TimeOfDay::Afternoon => "下午",
        TimeOfDay::Evening => "晚上",
        TimeOfDay::Night => "深夜",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> SentiApp {
        SentiApp {
            session: Session::new(build_engine(AppOptions {
                mode: InputMode::Zhuyin,
                ..AppOptions::ephemeral().with_mode(senti_core::InputMode::Mixed)
            })),
            shift: ShiftTap::default(),
            show_scores: false,
            settings_open: false,
            theme: egui::ThemePreference::System,
            status: String::new(),
            font_warning: None,
            data_path: None,
            memory_scope_edit: "default".into(),
        }
    }

    #[test]
    fn themes_and_small_viewport_render_zhuyin_candidates() {
        let ctx = egui::Context::default();
        let font = fonts::install_cjk_font(&ctx);
        let mut app = test_app();
        app.session.type_text("su3cl3");
        app.session.press(KeyEvent::OpenCandidates);
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            app.theme = theme.into();
            ctx.set_theme(theme);
            for (width, settings_open) in [
                (760.0, false),
                (760.0, true),
                (1100.0, false),
                (1100.0, true),
            ] {
                app.settings_open = settings_open;
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 700.0),
                    )),
                    ..Default::default()
                };
                let view = app.session.engine().view();
                let mut output = ctx.run_ui(input, |ui| {
                    app.render(ui);
                    if font.is_some() {
                        ctx.fonts_mut(|fonts| {
                            assert!(fonts.has_glyphs(
                                &FontId::proportional(24.0),
                                "ㄅㄆㄇㄈㄋㄧㄏㄠˊˇˋ˙你好"
                            ))
                        });
                    }
                });
                assert!(!output.shapes.is_empty());
                output.textures_delta.clear();
                assert_eq!(ctx.theme(), theme);
                assert_eq!(view.preedit, "ㄋㄧˇ ㄏㄠˇ");
                assert_eq!(view.candidates[0].text, "你好");
            }
        }
    }

    #[test]
    fn clipboard_events_reach_the_document_and_copy_the_exact_text() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        let text = "你好  Taiwan\n下一行";
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Paste(text.into()), egui::Event::Copy],
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        assert_eq!(app.session.document(), text);
        assert!(app.session.engine().user_model().is_empty());
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(copied) if copied == text)
        }));
        output.textures_delta.clear();
    }

    #[test]
    fn mixed_keyboard_input_preview_and_raw_shortcut_flow_through_the_real_ui() {
        let ctx = egui::Context::default();
        let mut app = test_app();
        app.session.set_mode(InputMode::Mixed);
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("su3cl3".into())],
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        output.textures_delta.clear();
        let view = app.session.engine().view();
        assert_eq!(view.preedit, "su3cl3");
        assert!(view.phonetic_preview.is_none());
        assert_eq!(view.candidates[0].text, "你好");
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    key(egui::Key::Enter, Default::default()),
                    egui::Event::Text("hello".into()),
                    key(egui::Key::Space, Default::default()),
                    egui::Event::Text(" ".into()),
                ],
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        output.textures_delta.clear();
        assert_eq!(app.session.document(), "你好hello ");
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    egui::Event::Text("OpenAI".into()),
                    key(
                        egui::Key::Enter,
                        egui::Modifiers {
                            command: true,
                            ..Default::default()
                        },
                    ),
                ],
                ..Default::default()
            },
            |ui| app.render(ui),
        );
        output.textures_delta.clear();
        assert_eq!(app.session.document(), "你好hello OpenAI");
        assert_eq!(
            app.session
                .engine()
                .user_model()
                .word_count(InputMode::English, "openai", "OpenAI"),
            0
        );
    }

    #[test]
    fn mixed_candidate_labels_and_controls_render_in_light_and_dark_small_windows() {
        let ctx = egui::Context::default();
        fonts::install_cjk_font(&ctx);
        let mut app = test_app();
        app.session.set_mode(InputMode::Mixed);
        app.session.type_text("su3cl3");
        app.settings_open = true;
        for theme in [egui::Theme::Light, egui::Theme::Dark] {
            app.theme = theme.into();
            for width in [760.0, 1100.0] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 700.0),
                        )),
                        ..Default::default()
                    },
                    |ui| app.render(ui),
                );
                assert!(!output.shapes.is_empty());
                output.textures_delta.clear();
                assert_eq!(ctx.theme(), theme);
                let view = app.session.engine().view();
                assert_eq!(view.preedit, "su3cl3");
                assert!(
                    view.candidates
                        .iter()
                        .any(|c| c.input_mode == Some(InputMode::English))
                );
                assert!(
                    view.candidates
                        .iter()
                        .any(|c| c.input_mode == Some(InputMode::Zhuyin))
                );
            }
        }
    }
}
