//! The settings page: the parts of `config.toml` that make sense to change
//! with the mouse, plus the shortcut list and the file itself.
//!
//! The page never applies a setting on its own. Every choice is written to
//! `config.toml` (keeping the user's comments, see `config::set_value`) and
//! the hot-reload watcher applies it as if the file had been edited by hand,
//! so there is one path from a setting to the window and the file stays the
//! single source of truth. The root hands the page each config it applies.

use gpui::{
    AnyElement, Context, Div, ElementId, Render, SharedString, Stateful, Window, div, prelude::*,
    px, rgb,
};
use gpui_component::{Icon, IconName};
use nohrs_core::config::{self, Config, SortOrder, SplitDirection, ThemeMode};
use nohrs_core::telemetry::LogErr;
use nohrs_ui::theme::theme;

/// The sections of the settings page, in the order the side list shows them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsSection {
    /// Theme mode and accent colour.
    Appearance,
    /// Listing and pane behaviour.
    Explorer,
    /// The keyboard shortcuts, for reference.
    Shortcuts,
    /// The config file itself and the version.
    About,
}

impl SettingsSection {
    const ALL: [Self; 4] = [Self::Appearance, Self::Explorer, Self::Shortcuts, Self::About];

    fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Aparência",
            Self::Explorer => "Explorador",
            Self::Shortcuts => "Atalhos",
            Self::About => "Sobre",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Palette,
            Self::Explorer => IconName::FolderOpen,
            Self::Shortcuts => IconName::CaseSensitive,
            Self::About => IconName::Info,
        }
    }
}

// Portuguese names for the accents `theme::NAMED_ACCENTS` knows, same order.
const ACCENT_LABELS: &[&str] = &["Laranja", "Azul", "Verde-água", "Verde", "Roxo", "Rosa", "Vermelho"];

// What the explorer actually answers to on Windows. Kept by hand next to the
// page rather than read from the keymap: half of these are `on_key_down`
// arms in `explorer/view.rs`, which no binding table knows about. A shortcut
// changed there has to change here too.
const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Arquivos",
        &[
            ("Ctrl+C", "Copiar"),
            ("Ctrl+X", "Recortar"),
            ("Ctrl+V", "Colar"),
            ("F2", "Renomear"),
            ("Delete", "Mandar para a Lixeira"),
            ("Shift+Delete", "Excluir para sempre"),
            ("Ctrl+Shift+N", "Nova pasta"),
            ("Enter", "Abrir no programa padrão"),
        ],
    ),
    (
        "Seleção e busca",
        &[
            ("Ctrl+A", "Selecionar tudo"),
            ("Esc", "Limpar a seleção ou o filtro"),
            ("Qualquer letra", "Filtrar a pasta atual"),
            ("Ctrl+F", "Buscar nas subpastas"),
            ("Ctrl+Shift+F", "Buscar no índice"),
            ("Ctrl+Shift+L", "Barra de filtros"),
        ],
    ),
    (
        "Abas e painéis",
        &[
            ("Ctrl+T", "Nova aba"),
            ("Ctrl+W", "Fechar aba"),
            ("Ctrl+\\", "Dividir lado a lado"),
            ("Ctrl+Shift+\\", "Dividir em cima e embaixo"),
            ("Ctrl+1 / Ctrl+2", "Ir para o painel 1 ou 2"),
            ("Ctrl+] / Ctrl+[", "Próximo / anterior painel"),
            ("Ctrl+B", "Mostrar ou esconder a barra lateral"),
        ],
    ),
    (
        "Exibição",
        &[
            ("Ctrl+= / Ctrl+-", "Aumentar / diminuir o zoom"),
            ("Ctrl+0", "Zoom padrão"),
            ("Ctrl+roda do mouse", "Zoom contínuo"),
            ("Ctrl+K", "Abrir o lançador"),
        ],
    ),
];

// What picking an option of a [`SettingsPage::choice`] does.
#[derive(Clone, Copy)]
enum Pick {
    // Writes the value (already in TOML form) to `[section] key`.
    Config(&'static str, &'static str, &'static str),
    // Takes folders over from Explorer, or gives them back.
    DefaultExplorer(bool),
}

/// The settings page.
pub struct SettingsPage {
    section: SettingsSection,
    // The config last applied by the root; what every control reads.
    config: Config,
    // The outcome of the last file operation, and whether it was an error.
    status: Option<(String, bool)>,
    // "Restaurar padrão" asks for a second click before replacing the file.
    confirm_reset: bool,
    // Whether folders open in Nohrs; `None` until the registry has answered.
    default_explorer: Option<bool>,
}

impl SettingsPage {
    /// Creates a new settings page on its first section.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut page = Self {
            section: SettingsSection::Appearance,
            config: Config::default(),
            status: None,
            confirm_reset: false,
            default_explorer: None,
        };
        page.refresh_default_explorer(cx);
        page
    }

    // Asks the registry, off the UI thread: each question is a `reg.exe` run.
    fn refresh_default_explorer(&mut self, cx: &mut Context<Self>) {
        let query = cx.background_spawn(async { crate::default_explorer::is_default() });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let is_default = query.await;
            this.update(cx, |page, cx| {
                page.default_explorer = Some(is_default);
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn set_default_explorer(&mut self, enable: bool, cx: &mut Context<Self>) {
        // The running binary, so the entries point wherever it was built.
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                self.status = Some((format!("Não foi possível achar o nohrs.exe: {error}"), true));
                cx.notify();
                return;
            }
        };
        let change = cx.background_spawn(async move {
            if enable {
                crate::default_explorer::enable(&exe)
            } else {
                crate::default_explorer::disable()
            }
        });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let outcome = change.await;
            this.update(cx, |page, cx| {
                page.status = Some(match outcome {
                    Ok(()) if enable => ("Pastas, drives e Win+E agora abrem no Nohrs.".to_string(), false),
                    Ok(()) => ("Pastas voltaram a abrir no Explorador do Windows.".to_string(), false),
                    Err(error) => (format!("Não foi possível mudar o registro: {error}"), true),
                });
                page.refresh_default_explorer(cx);
            })
            .log_err();
        })
        .detach();
    }

    fn pick(&mut self, pick: Pick, cx: &mut Context<Self>) {
        match pick {
            Pick::Config(section, key, value) => self.write(section, key, value.to_string(), cx),
            Pick::DefaultExplorer(enable) => self.set_default_explorer(enable, cx),
        }
    }

    /// Shows `section`, for the menu entries that lead to one directly.
    pub fn show_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.section = section;
        self.confirm_reset = false;
        cx.notify();
    }

    /// Takes the config the window now runs on, so the controls show it.
    pub fn set_config(&mut self, config: Config, cx: &mut Context<Self>) {
        self.config = config;
        cx.notify();
    }

    // Writes one value; the watcher brings it back through `set_config`.
    fn write(&mut self, section: &'static str, key: &'static str, value: String, cx: &mut Context<Self>) {
        let path = config::paths::config_file();
        let write = cx.background_spawn(async move { config::set_value(&path, section, key, &value) });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            if let Err(error) = write.await {
                this.update(cx, |page, cx| {
                    page.status = Some((format!("Não foi possível salvar {section}.{key}: {error}"), true));
                    cx.notify();
                })
                .log_err();
            }
        })
        .detach();
    }

    fn open_config_file(&mut self, cx: &mut Context<Self>) {
        let path = config::paths::config_file();
        // A first run may not have written it yet, and there is nothing to
        // open until it exists.
        if let Err(error) = config::ensure_exists(&path) {
            self.status = Some((format!("Não foi possível criar {}: {error}", path.display()), true));
            cx.notify();
            return;
        }
        // `explorer.exe <file>` hands it to whatever program `.toml` is
        // associated with, the same way a double click in the listing does.
        if let Err(error) = std::process::Command::new("explorer.exe").arg(&path).spawn() {
            self.status = Some((format!("Não foi possível abrir {}: {error}", path.display()), true));
            cx.notify();
        }
    }

    fn show_config_folder(&mut self, cx: &mut Context<Self>) {
        let path = config::paths::config_file();
        let mut command = std::process::Command::new("explorer.exe");
        if path.exists() {
            command.arg(format!("/select,{}", path.display()));
        } else {
            command.arg(config::paths::config_dir());
        }
        if let Err(error) = command.spawn() {
            self.status = Some((format!("Não foi possível abrir a pasta: {error}"), true));
            cx.notify();
        }
    }

    fn reset_config(&mut self, cx: &mut Context<Self>) {
        if !self.confirm_reset {
            self.confirm_reset = true;
            cx.notify();
            return;
        }
        self.confirm_reset = false;
        let path = config::paths::config_file();
        let reset = cx.background_spawn(async move { config::reset(&path) });
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            let outcome = reset.await;
            this.update(cx, |page, cx| {
                page.status = Some(match outcome {
                    Ok(Some(backup)) => (
                        format!("Configurações restauradas. A versão anterior ficou em {}", backup.display()),
                        false,
                    ),
                    Ok(None) => ("Configurações restauradas.".to_string(), false),
                    Err(error) => (format!("Não foi possível restaurar: {error}"), true),
                });
                cx.notify();
            })
            .log_err();
        })
        .detach();
    }

    fn render_nav(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(200.0))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(12.0))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .children(SettingsSection::ALL.into_iter().map(|section| {
                let active = section == self.section;
                div()
                    .id(ElementId::Name(SharedString::from(format!("settings-nav-{section:?}"))))
                    .h(px(32.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_sm()
                    .map(|this| {
                        if active {
                            this.bg(rgb(theme::accent_subtle())).text_color(rgb(theme::accent()))
                        } else {
                            this.text_color(rgb(theme::fg_secondary()))
                                .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
                        }
                    })
                    .on_click(cx.listener(move |page, _event, _window, cx| page.show_section(section, cx)))
                    .child(Icon::new(section.icon()).size_4())
                    .child(section.label())
            }))
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> Div {
        let mode = self.config.theme.mode;
        let accent = self.config.theme.accent.trim().to_string();
        let custom = accent.starts_with('#') && theme::parse_accent(&accent).is_some();

        // Laid out in a row of its own: the control column has no width to
        // wrap against, and `flex_wrap` there put every swatch on its own line.
        let swatches = div().flex().flex_row().gap_1().children(
            theme::NAMED_ACCENTS.iter().zip(ACCENT_LABELS).map(|(&(name, color), &label)| {
                let selected = accent.eq_ignore_ascii_case(name)
                    || (color.is_none() && theme::parse_accent(&accent).is_none());
                let swatch = color.unwrap_or_else(theme::palette_accent);
                div()
                    .id(ElementId::Name(SharedString::from(format!("accent-{name}"))))
                    .w(px(60.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .on_click(cx.listener(move |page, _event, _window, cx| {
                        page.write("theme", "accent", format!("\"{name}\""), cx);
                    }))
                    .child(
                        div()
                            .size(px(28.0))
                            .rounded_full()
                            .bg(rgb(swatch))
                            .border_2()
                            .border_color(rgb(if selected { theme::fg() } else { theme::bg() })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(if selected { theme::fg() } else { theme::fg_secondary() }))
                            .child(label),
                    )
            }),
        );

        section_body("Aparência")
            .child(setting_row(
                "Tema",
                None,
                self.config_choice(
                    "theme-mode",
                    &[
                        ("Escuro", mode == ThemeMode::Dark, "\"dark\""),
                        ("Claro", mode == ThemeMode::Light, "\"light\""),
                        ("Igual ao Windows", mode == ThemeMode::System, "\"system\""),
                    ],
                    ("theme", "mode"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Cor de destaque",
                Some(if custom {
                    "O config.toml tem uma cor própria; escolher uma aqui a substitui."
                } else {
                    "Seleção, abas ativas e botões."
                }),
                swatches,
            ))
    }

    fn render_explorer(&self, cx: &mut Context<Self>) -> Div {
        let ui = &self.config.ui;
        let explorer = &self.config.explorer;
        section_body("Explorador")
            .child(setting_row(
                "Itens ocultos",
                Some("Arquivos e pastas marcados como ocultos."),
                self.config_choice(
                    "show-hidden",
                    &[("Esconder", !ui.show_hidden, "false"), ("Mostrar", ui.show_hidden, "true")],
                    ("ui", "show_hidden"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Ordenação padrão",
                Some("Como cada pasta é ordenada ao abrir."),
                self.config_choice(
                    "default-sort",
                    &[
                        ("Nome", ui.default_sort == SortOrder::Name, "\"name\""),
                        ("Modificado", ui.default_sort == SortOrder::Modified, "\"modified\""),
                        ("Tamanho", ui.default_sort == SortOrder::Size, "\"size\""),
                        ("Tipo", ui.default_sort == SortOrder::Kind, "\"kind\""),
                    ],
                    ("ui", "default_sort"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Direção da ordenação",
                Some("Crescente: A→Z, menor e mais antigo primeiro. Pastas em que você mudou a ordem guardam a delas."),
                self.config_choice(
                    "default-sort-direction",
                    &[
                        ("Crescente", !ui.default_sort_descending, "false"),
                        ("Decrescente", ui.default_sort_descending, "true"),
                    ],
                    ("ui", "default_sort_descending"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Linhas do nome nos ícones",
                Some("Quantas linhas um nome ocupa no modo Ícones, em qualquer zoom; o resto vira \"…\"."),
                self.config_choice(
                    "name-lines",
                    &[
                        ("Sem limite", ui.name_lines == 0, "0"),
                        ("1", ui.name_lines == 1, "1"),
                        ("2", ui.name_lines == 2, "2"),
                        ("3", ui.name_lines == 3, "3"),
                        ("4", ui.name_lines == 4, "4"),
                    ],
                    ("ui", "name_lines"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Rolagem suave",
                Some("Quanto a roda do mouse desliza a lista em vez de pular. Desligada rola em degraus."),
                self.config_choice(
                    "scroll-smoothness",
                    &[
                        ("Desligada", ui.scroll_smoothness == 0, "0"),
                        ("Dura", ui.scroll_smoothness == 25, "25"),
                        ("Média", ui.scroll_smoothness == 50, "50"),
                        ("Suave", ui.scroll_smoothness == 75, "75"),
                        ("Muito suave", ui.scroll_smoothness == 100, "100"),
                    ],
                    ("ui", "scroll_smoothness"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Restaurar abas",
                Some("Reabrir as abas da última sessão. Vale na próxima vez que o Nohrs abrir."),
                self.config_choice(
                    "restore-tabs",
                    &[
                        ("Sim", explorer.restore_tabs, "true"),
                        ("Não", !explorer.restore_tabs, "false"),
                    ],
                    ("explorer", "restore_tabs"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Divisão de painéis",
                Some("Para onde Ctrl+\\ divide a janela."),
                self.config_choice(
                    "split-direction",
                    &[
                        ("Lado a lado", explorer.split_direction == SplitDirection::Vertical, "\"vertical\""),
                        (
                            "Em cima e embaixo",
                            explorer.split_direction == SplitDirection::Horizontal,
                            "\"horizontal\"",
                        ),
                    ],
                    ("explorer", "split_direction"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Painéis sincronizados",
                Some("Navegar num painel leva o outro para a mesma pasta."),
                self.config_choice(
                    "synced-panes",
                    &[
                        ("Não", !explorer.synced_panes, "false"),
                        ("Sim", explorer.synced_panes, "true"),
                    ],
                    ("explorer", "synced_panes"),
                    cx,
                ),
            ))
            .child(setting_row(
                "Explorador padrão do Windows",
                Some(
                    "Pastas, drives, Win+E e o ícone do Explorador na barra de tarefas abrem no Nohrs.                      Só neste usuário do Windows; \"Não\" devolve tudo ao Explorador.",
                ),
                match self.default_explorer {
                    Some(is_default) => self
                        .choice(
                            "default-explorer",
                            &[
                                ("Não", !is_default, Pick::DefaultExplorer(false)),
                                ("Sim", is_default, Pick::DefaultExplorer(true)),
                            ],
                            cx,
                        )
                        .into_any_element(),
                    None => div()
                        .text_xs()
                        .text_color(rgb(theme::fg_secondary()))
                        .child("Verificando…")
                        .into_any_element(),
                },
            ))
    }

    fn render_shortcuts(&self) -> Div {
        section_body("Atalhos").children(SHORTCUTS.iter().map(|(group, keys)| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .pb(px(4.0))
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::fg_secondary()))
                        .child(*group),
                )
                .children(keys.iter().map(|(keys, what)| {
                    div()
                        .h(px(30.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(rgb(theme::border()))
                        .child(div().text_sm().text_color(rgb(theme::fg())).child(*what))
                        .child(
                            div()
                                .px(px(8.0))
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .bg(rgb(theme::bg_secondary()))
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .text_xs()
                                .text_color(rgb(theme::fg()))
                                .child(*keys),
                        )
                }))
        }))
    }

    fn render_about(&self, cx: &mut Context<Self>) -> Div {
        let path = config::paths::config_file();
        section_body("Sobre")
            .child(setting_row(
                "Arquivo de configuração",
                Some("Tudo desta página fica nele, e editá-lo à mão vale na hora."),
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme::fg_secondary()))
                            .child(path.display().to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button("open-config", "Abrir config.toml", false).on_click(
                                cx.listener(|page, _event, _window, cx| page.open_config_file(cx)),
                            ))
                            .child(button("show-config", "Mostrar na pasta", false).on_click(
                                cx.listener(|page, _event, _window, cx| page.show_config_folder(cx)),
                            )),
                    ),
            ))
            .child(setting_row(
                "Restaurar padrão",
                Some("Troca o arquivo pelo padrão; o atual fica guardado ao lado."),
                button(
                    "reset-config",
                    if self.confirm_reset { "Clique de novo para confirmar" } else { "Restaurar padrão" },
                    self.confirm_reset,
                )
                .on_click(cx.listener(|page, _event, _window, cx| page.reset_config(cx))),
            ))
            .child(setting_row(
                "Versão",
                None,
                div().text_sm().text_color(rgb(theme::fg())).child(env!("CARGO_PKG_VERSION")),
            ))
    }

    // A `choice` whose every option writes its TOML value to `key` (a
    // `(section, key)` pair).
    fn config_choice(
        &self,
        id: &'static str,
        options: &[(&'static str, bool, &'static str)],
        (section, key): (&'static str, &'static str),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let options: Vec<_> = options
            .iter()
            .map(|&(label, selected, value)| (label, selected, Pick::Config(section, key, value)))
            .collect();
        self.choice(id, &options, cx)
    }

    // A row of mutually exclusive options, each doing its `Pick`.
    fn choice(
        &self,
        id: &'static str,
        options: &[(&'static str, bool, Pick)],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(6.0))
            .bg(rgb(theme::bg_secondary()))
            .border_1()
            .border_color(rgb(theme::border()))
            .children(options.iter().enumerate().map(|(ix, &(label, selected, pick))| {
                div()
                    .id(ElementId::NamedInteger(id.into(), ix as u64))
                    .h(px(26.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .text_sm()
                    .map(|this| {
                        if selected {
                            this.bg(rgb(theme::accent())).text_color(rgb(theme::accent_fg()))
                        } else {
                            this.text_color(rgb(theme::fg_secondary()))
                                .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
                        }
                    })
                    .when(!selected, |this| {
                        this.on_click(cx.listener(move |page, _event, _window, cx| page.pick(pick, cx)))
                    })
                    .child(label)
            }))
    }
}

fn section_body(title: &'static str) -> Div {
    div().flex().flex_col().gap(px(20.0)).child(
        div()
            .text_xl()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(theme::fg()))
            .child(title),
    )
}

// A setting: its name and explanation on the left, the control on the right.
fn setting_row(label: &'static str, hint: Option<&'static str>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(px(24.0))
        .pb(px(16.0))
        .border_b_1()
        .border_color(rgb(theme::border()))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .min_w(px(160.0))
                .flex_1()
                .child(div().text_sm().text_color(rgb(theme::fg())).child(label))
                .when_some(hint, |this, hint| {
                    this.child(div().text_xs().text_color(rgb(theme::fg_secondary())).child(hint))
                }),
        )
        .child(div().flex_shrink_0().child(control))
}

fn button(id: &'static str, label: &'static str, danger: bool) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(28.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .text_sm()
        .border_1()
        .map(|this| {
            if danger {
                this.border_color(rgb(theme::danger())).text_color(rgb(theme::danger()))
            } else {
                this.border_color(rgb(theme::border())).text_color(rgb(theme::fg()))
            }
        })
        .hover(|style| style.bg(rgb(theme::bg_hover())))
        .child(label)
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.section {
            SettingsSection::Appearance => self.render_appearance(cx),
            SettingsSection::Explorer => self.render_explorer(cx),
            SettingsSection::Shortcuts => self.render_shortcuts(),
            SettingsSection::About => self.render_about(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_row()
            .bg(rgb(theme::bg()))
            .child(self.render_nav(cx))
            .child(
                div()
                    .id("settings-content")
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .overflow_y_scroll()
                    .px(px(32.0))
                    .py(px(24.0))
                    .child(
                        div()
                            .max_w(px(760.0))
                            .flex()
                            .flex_col()
                            .gap_4()
                            .when_some(self.status.clone(), |this, (message, error)| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(if error { theme::danger() } else { theme::fg_secondary() }))
                                        .child(message),
                                )
                            })
                            .child(body),
                    ),
            )
    }
}

impl crate::Page for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        <Self as Render>::render(self, window, cx).into_any_element()
    }
}
