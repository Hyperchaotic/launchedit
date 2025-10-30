// SPDX-License-Identifier: GPL-3.0-only

use crate::config::Config;
use crate::fl;
use crate::mimelist::{MimeCache, MimeCategory, MimeItem};
use crate::xdghelp::{IconCache, PickKind, open_path, save_desktop_file};

use cosmic::app::context_drawer;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::Alignment::Center;
use cosmic::iced::alignment::Horizontal::Left;
use cosmic::iced::alignment::{Horizontal, Vertical};
use cosmic::iced::keyboard::Modifiers;
use cosmic::iced::{Alignment, Length, Subscription, event, keyboard, window};

use cosmic::iced::keyboard::Key;
use cosmic::iced::{widget::column, widget::row};
use cosmic::prelude::*;
use cosmic::widget::menu::Action;
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};
use cosmic::widget::{self, container, horizontal_space, list, menu, vertical_space};
use cosmic::widget::{icon, nav_bar, table};
use cosmic::{Apply, Element};
use cosmic::{cosmic_theme, theme};
use freedesktop_desktop_entry::{DecodeError, DesktopEntry};
use futures_util::SinkExt;
use std::collections::HashMap;
use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, path::Path};
use thiserror::Error;

use std::borrow::Cow;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!(
    "../resources/icons/hicolor/scalable/apps/com.github.hyperchaotic.launchedit.svg"
);

const GENERAL_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/general.svg");
const MIMETYPES_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/mimetypes.svg");
const ACTIONS_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/actions.svg");
const CUSTOM_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/extensions.svg");
const ADVANCED_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/advanced.svg");

macro_rules! desktop_edit_field {
    ($key:expr, $hint:expr, $value:expr, $am_editing:expr, $self:ident) => {{
        widget::editable_input($hint, $value, $am_editing, |_| Message::ToggleEdit($key))
            .width(Length::Fill)
            .on_input(|t| Message::SetTextEntry($key, t))
    }};
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Failed to open portal: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Missing file path argument")]
    MissingArgument,
    #[error("File not found")]
    FileNotFound(String),
    #[error("Failed to decode .desktop file: {0}")]
    Decode(#[from] DecodeError),
}

#[derive(Debug, Default)]
struct Editing {
    pub name: bool,
    pub generic_name: bool,
    pub comment: bool,
    pub path: bool,
    pub exec: bool,
    pub icon: bool,
    pub try_exec: bool,
    pub only_shown_in: bool,
    pub not_shown_in: bool,
    pub keywords: bool,
    pub categories: bool,
    pub implements: bool,
    pub startupwmclass: bool,
    pub url: bool,
}

impl Editing {
    pub fn toggle(&mut self, key: &DesktopKey) {
        match key {
            DesktopKey::Name => self.name ^= true,
            DesktopKey::GenericName => self.generic_name ^= true,
            DesktopKey::Comment => self.comment ^= true,
            DesktopKey::Path => self.path ^= true,
            DesktopKey::Exec => self.exec ^= true,
            DesktopKey::Icon => self.icon ^= true,
            DesktopKey::TryExec => self.try_exec ^= true,
            DesktopKey::OnlyShowIn => self.only_shown_in ^= true,
            DesktopKey::NotShowIn => self.not_shown_in ^= true,
            DesktopKey::Keywords => self.keywords ^= true,
            DesktopKey::Categories => self.categories ^= true,
            DesktopKey::Implements => self.implements ^= true,
            DesktopKey::StartupWMClass => self.startupwmclass ^= true,
            DesktopKey::Url => self.url ^= true,
            _ => {
                todo!();
            }
        }
    }
}

#[derive(Default, Debug, Clone, Copy, Eq, PartialEq)]
pub enum DesktopEntryType {
    #[default]
    Application,
    Link,
    Directory,
}

impl fmt::Display for DesktopEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesktopEntryType::Application => f.write_str("Application"),
            DesktopEntryType::Link => f.write_str("Link"),
            DesktopEntryType::Directory => f.write_str("Directory"),
        }
    }
}

impl FromStr for DesktopEntryType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Application" => Ok(Self::Application),
            "Link" => Ok(Self::Link),
            "Directory" => Ok(Self::Directory),
            _ => Err(()),
        }
    }
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    /// Key bindings for the application's menu bar.
    key_binds: HashMap<menu::KeyBind, MenuAction>,
    // Configuration data that persists between application runs.
    config: Config,
    nav: nav_bar::Model,
    mime_table: table::SingleSelectModel<MimeItem, MimeCategory>,
    locales: Vec<String>,
    mime_descriptions: MimeCache,
    icon_cache: IconCache,
    current_entry: Option<DesktopEntry>,
    current_entry_path: Option<PathBuf>,
    current_entry_error: Option<AppError>,
    current_entry_changed: bool,
    am_editing: Editing,
    new_mimetype: String,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    Quit,
    Save,
    SaveAs,
    SaveFinished(Option<PathBuf>),
    OpenPath(PickKind),
    Key(Modifiers, keyboard::Key),
    OpenFileFinished((Option<PathBuf>, PickKind)),
    SetTextEntry(DesktopKey, String),
    SetBoolEntry(DesktopKey, bool),

    MimeItemSelect(table::Entity),
    RemoveMimetype(Option<usize>),
    EditNewMimetype(String),
    CreateMimetype,
    CreateEntry(DesktopEntryType),

    OpenRepositoryUrl,
    SubscriptionChannel,
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    CloseWindow(window::Id),
    ToggleEdit(DesktopKey),
    None,
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "com.github.hyperchaotic.desktop-edit";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn on_app_exit(&mut self) -> Option<Message> {
        Some(Message::Quit)
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            key_binds: Self::key_binds(),
            // Optional configuration file for an application.
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => {
                        // for why in errors {
                        //     tracing::error!(%why, "error loading app config");
                        // }

                        config
                    }
                })
                .unwrap_or_default(),
            nav: nav_bar::Model::default(),
            mime_table: table::Model::new(vec![MimeCategory::Name, MimeCategory::Description]),
            locales: freedesktop_desktop_entry::get_languages_from_env(),
            mime_descriptions: MimeCache::default(),
            icon_cache: IconCache::default(),
            current_entry: None,
            current_entry_path: None,
            current_entry_error: None,
            current_entry_changed: false,
            am_editing: Editing::default(),
            new_mimetype: String::new(),
        };

        app.load_entry_from_args();
        app.create_nav_bar();

        (app, Task::none())
    }

    /// Enables the COSMIC application to create a nav bar with this model.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    fn header_start(&'_ self) -> Vec<Element<'_, Self::Message>> {
        let (save, saveas) = if self.current_entry.is_some() {
            (
                menu::Item::Button(fl!("menu-save"), None, MenuAction::Save),
                menu::Item::Button(fl!("menu-saveas"), None, MenuAction::SaveAs),
            )
        } else {
            (
                menu::Item::ButtonDisabled(fl!("menu-save"), None, MenuAction::Save),
                menu::Item::ButtonDisabled(fl!("menu-saveas"), None, MenuAction::SaveAs),
            )
        };

        let menu_bar = menu::bar(vec![
            menu::Tree::with_children(
                menu::root(fl!("menu-file")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![
                        menu::Item::Folder(
                            fl!("menu-new"),
                            vec![
                                menu::Item::Button(
                                    fl!("menu-newapplication"),
                                    None,
                                    MenuAction::NewApplication,
                                ),
                                menu::Item::Button(fl!("menu-newlink"), None, MenuAction::NewLink),
                                menu::Item::Button(
                                    fl!("menu-newdirectory"),
                                    None,
                                    MenuAction::NewDirectory,
                                ),
                            ],
                        ),
                        menu::Item::Divider,
                        menu::Item::Button(fl!("menu-open"), None, MenuAction::Open),
                        save,
                        saveas,
                        menu::Item::Divider,
                        menu::Item::Button(fl!("menu-quit"), None, MenuAction::Quit),
                    ],
                ),
            ),
            menu::Tree::with_children(
                menu::root(fl!("menu-view")).apply(Element::from),
                menu::items(
                    &self.key_binds,
                    vec![menu::Item::Button(
                        fl!("menu-about"),
                        None,
                        MenuAction::About,
                    )],
                ),
            ),
        ])
        .item_width(menu::ItemWidth::Uniform(200))
        .item_height(menu::ItemHeight::Dynamic(200))
        .spacing(4.0);

        vec![menu_bar.into()]
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer(&'_ self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match &self.context_page {
            ContextPage::About => context_drawer::context_drawer(
                self.context_about(),
                Message::ToggleContextPage(ContextPage::About),
            )
            .title(fl!("menu-about")),
            ContextPage::IOError(e) => context_drawer::context_drawer(
                self.context_ioerror(e),
                Message::ToggleContextPage(ContextPage::IOError(e.to_owned())),
            )
            .title(fl!("context-unabletosave")),
        })
    }

    /// Describes the interface based on the current state of the application model.
    ///
    /// Application events will be processed through the view. Any messages emitted by
    /// events received by widgets will be passed to the update method.
    fn view(&self) -> Element<'_, Self::Message> {
        let theme = cosmic::theme::active();
        let padding = if self.core.is_condensed() {
            theme.cosmic().space_s()
        } else {
            theme.cosmic().space_l()
        };

        // MissingArgument not actually an error
        let fatal_error = self
            .current_entry_error
            .as_ref()
            .filter(|e| !matches!(e, AppError::MissingArgument));

        match (fatal_error, self.current_entry.as_ref()) {
            // Landing / browse
            (None, None) => {
                let folder = widget::icon::from_name("folder-symbolic").handle();

                column!(
                    vertical_space(),
                    widget::text::title1(fl!("app-title"))
                        .apply(widget::container)
                        .width(Length::Fill)
                        .align_x(Horizontal::Center)
                        .align_y(Vertical::Center),
                    widget::button::text(fl!("action-browse"))
                        .trailing_icon(folder)
                        .on_press(Message::OpenPath(PickKind::DesktopFile)),
                    vertical_space()
                )
                .align_x(Horizontal::Center)
                .into()
            }

            // Error
            (Some(error), _) => column!(
                widget::text::title1(fl!("error-parsingentry"))
                    .apply(widget::container)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Center)
                    .align_y(Vertical::Center),
                widget::text::body(error.to_string())
                    .apply(widget::container)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Horizontal::Center)
            )
            .into(),

            // Show entry
            (None, Some(entry)) => {
                match entry.type_().unwrap_or_default().to_lowercase().as_str() {
                    "link" => self.link_view(entry, padding),
                    "directory" => self.directory_view(entry, padding),
                    "application" => self.application_view(entry, padding),
                    _ => column!(
                        widget::text::title1(fl!("error-parsingentry"))
                            .apply(widget::container)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_x(Horizontal::Center)
                            .align_y(Vertical::Center),
                        widget::text::body(fl!("error-parsingentry"))
                            .apply(widget::container)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_x(Horizontal::Center)
                    )
                    .into(),
                }
            }
        }
    }

    /// Register subscriptions for this application.
    ///
    /// Subscriptions are long-running async tasks running in the background which
    /// emit messages to the application through a channel. They are started at the
    /// beginning of the application, and persist through its lifetime.
    fn subscription(&self) -> Subscription<Self::Message> {
        struct MySubscription;

        Subscription::batch(vec![
            event::listen_with(|event, status, window_id| match event {
                event::Event::Keyboard(keyboard::Event::KeyPressed { modifiers, key, .. }) => {
                    match status {
                        event::Status::Ignored => Some(Message::Key(modifiers, key)),
                        event::Status::Captured => None,
                    }
                }
                event::Event::Window(cosmic::iced::window::Event::CloseRequested) => {
                    Some(Message::CloseWindow(window_id))
                }
                _ => None,
            }),
            // Create a subscription which emits updates through a channel.
            Subscription::run_with_id(
                std::any::TypeId::of::<MySubscription>(),
                cosmic::iced::stream::channel(4, move |mut channel| async move {
                    _ = channel.send(Message::SubscriptionChannel).await;

                    futures_util::future::pending().await
                }),
            ),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    // for why in update.errors {
                    //     tracing::error!(?why, "app config error");
                    // }

                    Message::UpdateConfig(update.config)
                }),
        ])
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Quit => {
                std::process::exit(0);
            }
            Message::SaveAs => {
                if let Some(entry) = &self.current_entry {
                    let kind = self.entry_type().unwrap_or_default();

                    let base = entry
                        .name(&self.locales)
                        .map(|s| s.to_lowercase().replace(' ', "-"))
                        .unwrap_or_else(|| match kind {
                            DesktopEntryType::Link => fl!("filename-link"),
                            DesktopEntryType::Directory => fl!("filename-directory"),
                            _ => fl!("filename-application"),
                        });

                    let ext = if kind == DesktopEntryType::Directory {
                        ".directory"
                    } else {
                        ".desktop"
                    };

                    let suggested = format!("{base}{ext}");

                    return Task::perform(save_desktop_file(suggested, kind), |f| {
                        cosmic::Action::App(Message::SaveFinished(f))
                    });
                }
            }
            Message::SaveFinished(res) => {
                println!("Message::SaveFinished {:?}", res);
                if let Some(path) = res
                    && let Some(entry) = &mut self.current_entry
                {
                    if let Err(e) = Self::save_desktop_entry(&path, &entry.to_string()) {
                        println!("Error saving {e}");
                        return self.update(Message::ToggleContextPage(ContextPage::IOError(
                            e.to_string(),
                        )));
                    }

                    self.current_entry_changed = false;
                    self.current_entry_error = None;
                    self.current_entry_path = Some(path);
                }
            }
            Message::Save => {
                if self.current_entry_changed
                    && let Some(entry) = &self.current_entry
                {
                    if self.current_entry_path.is_none() {
                        return self.update(Message::SaveAs);
                    } else if entry.path.is_file() {
                        return self.update(Message::SaveFinished(Some(entry.path.clone())));
                    }
                }
            }
            Message::OpenPath(kind) => {
                return Task::perform(open_path(kind), |f| {
                    cosmic::Action::App(Message::OpenFileFinished(f))
                });
            }
            Message::Key(modifiers, key) => {
                for (key_bind, action) in self.key_binds.iter() {
                    if key_bind.matches(modifiers, &key) {
                        return self.update(action.message());
                    }
                }
            }
            Message::OpenFileFinished(path) => {
                if let (Some(desktop_file), kind) = path {
                    match kind {
                        // Load file
                        PickKind::DesktopFile => {
                            self.load_entry_from_path(&desktop_file);
                        }
                        // Save Exec or Path in current desktop entry
                        PickKind::Executable => {
                            self.set_exec_with_args(&desktop_file, kind, None);
                        }
                        // Save Exec or Path in current desktop entry
                        PickKind::TryExecutable => {
                            self.set_exec_with_args(&desktop_file, kind, None);
                        }
                        PickKind::Directory => {
                            self.set_path(&desktop_file);
                        }
                        PickKind::IconFile => {
                            self.set_text(DesktopKey::Icon, desktop_file.to_string_lossy());
                        }
                    }
                }
            }

            Message::SetTextEntry(key, text) => {
                self.set_text(key, text);
            }

            Message::SetBoolEntry(key, boolean) => {
                self.set_bool(key, boolean);
            }

            Message::MimeItemSelect(entity) => self.mime_table.activate(entity),
            Message::OpenRepositoryUrl => {
                _ = open::that_detached(REPOSITORY);
            }
            Message::RemoveMimetype(pos) => {
                if let Some(p) = pos
                    && let Some(entity) = self.mime_table.entity_at(p as u16)
                {
                    // Update table model
                    self.mime_table.remove(entity);
                    let mut mimes = Vec::new();
                    for entity in self.mime_table.iter() {
                        if let Some(mime) = self.mime_table.item(entity) {
                            mimes.push(mime.name.to_owned());
                        }
                    }
                    // Update desktop entry from table
                    self.set_list(DesktopKey::MimeType, &mimes);
                }
            }

            Message::EditNewMimetype(string) => {
                self.new_mimetype = string.trim_start().trim_end().to_string();
            }
            Message::CreateMimetype => {
                let mime = self.new_mimetype.to_owned();
                self.new_mimetype.clear();
                self.create_mimetype(&mime);
            }

            Message::CreateEntry(new_kind) => {
                self.clear_all();
                let name = match new_kind {
                    DesktopEntryType::Application => fl!("my-application"),
                    DesktopEntryType::Link => fl!("my-link"),
                    DesktopEntryType::Directory => fl!("my-directory"),
                };
                self.current_entry = Some(DesktopEntry::from_appid(name));
                self.set_text(DesktopKey::Type, new_kind.to_string());
                self.create_nav_bar();
            }

            Message::SubscriptionChannel => {
                // For example purposes only.
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    // Close the context drawer if the toggled context page is the same.
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    // Open the context drawer to display the requested context page.
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::CloseWindow(id) => {
                if Some(id) == self.core.main_window_id() {
                    return self.update(Message::Quit);
                }
            }

            Message::ToggleEdit(field) => self.am_editing.toggle(&field),
            Message::None => (),
        }
        Task::none()
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        // Activate the page in the model.
        self.nav.activate(id);

        self.update_title()
    }
}

impl AppModel {
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        if let Some(page) = self.nav.text(self.nav.active()) {
            window_title.push_str(" — ");
            window_title.push_str(page);
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    fn link_view<'a>(
        &'a self,
        entry: &'a DesktopEntry,
        padding: u16,
    ) -> Element<'a, crate::app::Message> {
        let placeholder_row = |page: NavPage| {
            row!(
                horizontal_space(),
                widget::text::body(format!("No {}.", page)),
                horizontal_space()
            )
            .into()
        };

        let active_tab_content: Element<'_, crate::app::Message> =
            match self.nav.position(self.nav.active()) {
                Some(0) => self.link_view_general(entry, padding),
                Some(1) => placeholder_row(NavPage::Mimetypes),
                Some(2) => placeholder_row(NavPage::Actions),
                Some(3) => placeholder_row(NavPage::Custom),
                _ => placeholder_row(NavPage::Advanced),
            };

        column!(active_tab_content)
            .padding(padding)
            .spacing(padding)
            .into()
    }

    fn link_view_general<'a>(
        &'a self,
        entry: &'a DesktopEntry,
        padding: u16,
    ) -> Element<'a, crate::app::Message> {
        let icon_button = container(self.get_icon_button())
            .width(60)
            .height(60)
            .align_y(Center)
            .align_x(Center);

        let label_w = 130;
        let locales = &self.locales;
        let folder = widget::icon::from_name("folder-symbolic").handle();

        let location = format!(
            "Location: {}",
            self.current_entry_path
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
        );

        let content = list::ListColumn::new()
            .add(
                row!(
                    widget::text(fl!("field-name")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Name,
                        fl!("hint-name-link"),
                        entry.name(locales).unwrap_or_default().into_owned(),
                        self.am_editing.name,
                        self
                    )
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-genericname"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::GenericName,
                        fl!("hint-genericname"),
                        entry.generic_name(locales).unwrap_or_default().into_owned(),
                        self.am_editing.generic_name,
                        self
                    )
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-icon")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Icon,
                        fl!("hint-icon"),
                        entry.icon().unwrap_or_default(),
                        self.am_editing.icon,
                        self
                    )
                    .width(Length::Fill),
                    widget::button::icon(folder.clone())
                        .on_press(Message::OpenPath(PickKind::IconFile))
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-comment"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Comment,
                        fl!("hint-comment"),
                        entry.comment(locales).unwrap_or_default().into_owned(),
                        self.am_editing.comment,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-url")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Url,
                        fl!("hint-url"),
                        entry.url().unwrap_or_default(),
                        self.am_editing.url,
                        self
                    ),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-hide")).align_x(Left).width(label_w),
                    horizontal_space(),
                    widget::toggler(entry.no_display())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::NoDisplay, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-keywords"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Keywords,
                        fl!("hint-keywords"),
                        entry
                            .keywords(locales)
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.keywords,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            );

        column!(
            Element::from(icon_button),
            Element::from(content),
            Element::from(widget::text(location))
        )
        .padding(padding)
        .spacing(padding)
        .into()
    }

    fn directory_view<'a>(
        &'a self,
        entry: &'a DesktopEntry,
        padding: u16,
    ) -> Element<'a, crate::app::Message> {
        let placeholder_row = |page: NavPage| {
            row!(
                horizontal_space(),
                widget::text::body(format!("No {}.", page)),
                horizontal_space()
            )
            .into()
        };

        let active_tab_content: Element<'_, crate::app::Message> =
            match self.nav.position(self.nav.active()) {
                Some(0) => self.directory_view_general(entry, padding),
                Some(1) => placeholder_row(NavPage::Mimetypes),
                Some(2) => placeholder_row(NavPage::Actions),
                Some(3) => placeholder_row(NavPage::Custom),
                _ => placeholder_row(NavPage::Advanced),
            };

        column!(active_tab_content)
            .padding(padding)
            .spacing(padding)
            .into()
    }

    fn directory_view_general<'a>(
        &'a self,
        entry: &'a DesktopEntry,
        padding: u16,
    ) -> Element<'a, crate::app::Message> {
        let icon_button = container(self.get_icon_button())
            .width(60)
            .height(60)
            .align_y(Center)
            .align_x(Center);

        let label_w = 130;
        let locales = &self.locales;
        let folder = widget::icon::from_name("folder-symbolic").handle();

        let location = format!(
            "Location: {}",
            self.current_entry_path
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
        );

        let content = list::ListColumn::new()
            .add(
                row!(
                    widget::text(fl!("field-name")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Name,
                        fl!("hint-name-directory"),
                        entry.name(locales).unwrap_or_default().into_owned(),
                        self.am_editing.name,
                        self
                    )
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-icon")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Icon,
                        fl!("hint-icon"),
                        entry.icon().unwrap_or_default(),
                        self.am_editing.icon,
                        self
                    )
                    .width(Length::Fill),
                    widget::button::icon(folder.clone())
                        .on_press(Message::OpenPath(PickKind::IconFile))
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-comment"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Comment,
                        fl!("hint-comment"),
                        entry.comment(locales).unwrap_or_default().into_owned(),
                        self.am_editing.comment,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-keywords"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Keywords,
                        fl!("hint-keywords"),
                        entry
                            .keywords(locales)
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.keywords,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-hide")).align_x(Left).width(label_w),
                    horizontal_space(),
                    widget::toggler(entry.no_display())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::NoDisplay, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-onlyshownin"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::OnlyShowIn,
                        fl!("hint-onlyshownin"),
                        entry
                            .only_show_in()
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.only_shown_in,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-notshownin"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::NotShowIn,
                        fl!("hint-notshownin"),
                        entry.not_show_in().map(|v| v.join(";")).unwrap_or_default(),
                        self.am_editing.not_shown_in,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            );

        column!(
            Element::from(icon_button),
            Element::from(content),
            Element::from(widget::text(location))
        )
        .padding(padding)
        .spacing(padding)
        .into()
    }

    fn application_view<'a>(
        &'a self,
        entry: &'a DesktopEntry,
        padding: u16,
    ) -> Element<'a, crate::app::Message> {
        let active_tab_content: Element<'_, crate::app::Message> =
            match self.nav.position(self.nav.active()) {
                Some(0) => self.view_tab_general(entry),
                Some(1) => self.view_tab_mimetypes(entry),
                Some(2) => row!(
                    horizontal_space(),
                    widget::text::body("😵‍💫"),
                    horizontal_space()
                )
                .into(),
                Some(3) => row!(
                    horizontal_space(),
                    widget::text::body("😵‍💫"),
                    horizontal_space()
                )
                .into(),
                _ => self.view_tab_advanced(entry),
            };

        column!(Element::from(active_tab_content))
            .padding(padding)
            .spacing(padding)
            .into()
    }

    fn view_tab_mimetypes<'a>(
        &'a self,
        appdata: &'a DesktopEntry,
    ) -> Element<'a, crate::app::Message> {
        let mimes_owned: Option<Vec<String>> = appdata.mime_type().as_ref().map(|list| {
            list.iter()
                .map(|mime| mime.to_string())
                .collect::<Vec<String>>()
        });

        let remove_button = if let Some(pos) = self.mime_table.position(self.mime_table.active()) {
            widget::button::text("Remove").on_press(Message::RemoveMimetype(Some(pos as usize)))
        } else {
            widget::button::text("Remove")
        };

        let add_button = if self.new_mimetype.is_empty() {
            widget::button::text("Add")
        } else {
            widget::button::text("Add").on_press(Message::CreateMimetype)
        };

        row!(
            horizontal_space(),
            column!(
                widget::table(&self.mime_table)
                    .on_item_left_click(Message::MimeItemSelect)
                    .item_context(move |item| {
                        let pos = mimes_owned
                            .as_ref()
                            .and_then(|list| list.iter().position(|n| n == &item.name));

                        Some(widget::menu::items(
                            &HashMap::new(),
                            vec![widget::menu::Item::Button(
                                format!("Remove {}", item.name),
                                None,
                                MenuAction::RemoveMimetype(pos),
                            )],
                        ))
                    })
                    .category_context(|category| {
                        Some(widget::menu::items(
                            &HashMap::new(),
                            vec![
                                widget::menu::Item::Button(
                                    format!("Action on {} category", category),
                                    None,
                                    MenuAction::None,
                                ),
                                widget::menu::Item::Button(
                                    format!("Other action on {} category", category),
                                    None,
                                    MenuAction::None,
                                ),
                            ],
                        ))
                    })
                    .width(500),
                row!(
                    remove_button,
                    add_button,
                    widget::text_input("New mimetype", &self.new_mimetype)
                        .on_input(Message::EditNewMimetype)
                        .width(200),
                    horizontal_space()
                )
                .width(500)
            ),
            horizontal_space()
        )
        .apply(Element::from)
    }

    fn view_tab_general<'a>(
        &'a self,
        appdata: &'a DesktopEntry,
    ) -> Element<'a, crate::app::Message> {
        let label_w = 130;
        let locales = &self.locales;
        let folder = widget::icon::from_name("folder-symbolic").handle();

        let location = format!(
            "Location: {}",
            self.current_entry_path
                .clone()
                .unwrap_or_default()
                .to_string_lossy()
        );
        let list = list::ListColumn::new()
            .add(
                row!(
                    widget::text(fl!("field-name")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Name,
                        fl!("hint-name-application"),
                        appdata.name(locales).unwrap_or_default().into_owned(),
                        self.am_editing.name,
                        self
                    )
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-icon")).align_x(Left).width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Icon,
                        fl!("hint-icon"),
                        appdata.icon().unwrap_or_default(),
                        self.am_editing.icon,
                        self
                    )
                    .width(Length::Fill),
                    widget::button::icon(folder.clone())
                        .on_press(Message::OpenPath(PickKind::IconFile))
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-comment"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Comment,
                        fl!("hint-comment"),
                        appdata.comment(locales).unwrap_or_default().into_owned(),
                        self.am_editing.comment,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-command"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Exec,
                        fl!("hint-exec"),
                        appdata.exec().unwrap_or_default(),
                        self.am_editing.exec,
                        self
                    ),
                    widget::button::icon(folder.clone())
                        .on_press(Message::OpenPath(PickKind::Executable)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-workpath"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Path,
                        fl!("hint-path"),
                        appdata.path().unwrap_or_default(),
                        self.am_editing.path,
                        self
                    ),
                    widget::button::icon(folder).on_press(Message::OpenPath(PickKind::Directory)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-runinterm"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.terminal())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::Terminal, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-hide")).align_x(Left).width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.no_display())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::NoDisplay, b)),
                )
                .align_y(Center)
                .spacing(5),
            );

        let icon_button = container(self.get_icon_button())
            .width(60)
            .height(60)
            .align_y(Center)
            .align_x(Center);

        let c = column!(icon_button, list, widget::text(location)).spacing(20);
        widget::scrollable(c).into()
    }

    fn view_tab_advanced<'a>(
        &'a self,
        appdata: &'a DesktopEntry,
    ) -> Element<'a, crate::app::Message> {
        let label_w = 130;
        let locales = &self.locales;
        let folder = widget::icon::from_name("folder-symbolic").handle();

        let list = list::ListColumn::new()
            .add(
                row!(
                    widget::text(fl!("field-genericname"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::GenericName,
                        fl!("hint-genericname"),
                        appdata
                            .generic_name(locales)
                            .unwrap_or_default()
                            .into_owned(),
                        self.am_editing.generic_name,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-tryexec"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::TryExec,
                        fl!("hint-tryexec"),
                        appdata.try_exec().unwrap_or_default(),
                        self.am_editing.try_exec,
                        self
                    ),
                    widget::button::icon(folder.clone())
                        .on_press(Message::OpenPath(PickKind::TryExecutable)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-onlyshownin"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::OnlyShowIn,
                        fl!("hint-onlyshownin"),
                        appdata
                            .only_show_in()
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.only_shown_in,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-notshownin"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::NotShowIn,
                        fl!("hint-notshownin"),
                        appdata
                            .not_show_in()
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.not_shown_in,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-keywords"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Keywords,
                        fl!("hint-keywords"),
                        appdata
                            .keywords(locales)
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.keywords,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-categories"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Categories,
                        fl!("hint-categories"),
                        appdata
                            .categories()
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.categories,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-implements"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::Implements,
                        fl!("hint-implements"),
                        appdata
                            .implements()
                            .map(|v| v.join(";"))
                            .unwrap_or_default(),
                        self.am_editing.implements,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-startupwmclass"))
                        .align_x(Left)
                        .width(label_w),
                    desktop_edit_field!(
                        DesktopKey::StartupWMClass,
                        "",
                        appdata.startup_wm_class().unwrap_or_default(),
                        self.am_editing.startupwmclass,
                        self
                    )
                    .width(Length::Fill)
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-startupnotify"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.startup_notify())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::StartupNotify, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-nondefaultgpu"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.prefers_non_default_gpu())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::PrefersNonDefaultGPU, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-hidden"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.hidden())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::Hidden, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-singlemainwindow"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.single_main_window())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::SingleMainWindow, b)),
                )
                .align_y(Center)
                .spacing(5),
            )
            .add(
                row!(
                    widget::text(fl!("field-dbusactivation"))
                        .align_x(Left)
                        .width(label_w),
                    horizontal_space(),
                    widget::toggler(appdata.dbus_activatable())
                        .on_toggle(|b| Message::SetBoolEntry(DesktopKey::DBusActivatable, b)),
                )
                .align_y(Center)
                .spacing(5),
            );

        let ctrl = widget::scrollable::vertical(list);
        ctrl.into()
    }

    fn changed(&mut self) {
        self.current_entry_changed = true;
    }

    pub fn set_text(&mut self, key: DesktopKey, text: impl Into<String>) {
        if let Some(entry) = &mut self.current_entry {
            entry.add_desktop_entry(key.to_string(), text.into());
            self.changed();
        }
    }

    pub fn set_bool(&mut self, key: DesktopKey, value: bool) {
        self.set_text(key, if value { "true" } else { "false" });
    }

    pub fn set_list<S: AsRef<str>>(&mut self, key: DesktopKey, items: &[S]) {
        let s = items
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(";");
        // Many tools tolerate missing trailing ';', add if you prefer:
        // let s = format!("{s};");
        self.set_text(key, s);
    }

    pub fn set_path(&mut self, path: &Path) {
        let p = path.display().to_string();
        let needs_quotes = p.contains(' ');
        let val = if needs_quotes { format!("\"{p}\"") } else { p };
        self.set_text(DesktopKey::Path, val);
    }

    pub fn set_exec_with_args(&mut self, exe: &Path, kind: PickKind, args: Option<&str>) {
        let exe_str = exe.display().to_string();

        // Quote the path if it contains spaces
        let quoted = if exe_str.contains(' ') {
            format!("\"{exe_str}\"")
        } else {
            exe_str
        };

        // Combine executable + args only if args are provided
        let cmd = match args {
            Some(arg) if !arg.is_empty() => format!("{quoted} {arg}"),
            _ => quoted,
        };

        if kind == PickKind::TryExecutable {
            self.set_text(DesktopKey::TryExec, cmd);
        } else {
            self.set_text(DesktopKey::Exec, cmd);
        }
    }

    pub fn context_about(&'_ self) -> Element<'_, Message> {
        let cosmic_theme::Spacing { space_xxs, .. } = theme::active().cosmic().spacing;

        let icon = widget::svg(widget::svg::Handle::from_memory(APP_ICON));

        let title = widget::text::title3(fl!("app-title"));

        let link = widget::button::link(REPOSITORY)
            .on_press(Message::OpenRepositoryUrl)
            .padding(0);

        let version = format!("Version {}.", env!("CARGO_PKG_VERSION"));

        widget::column()
            .push(icon)
            .push(title)
            .push(link)
            .push(widget::text::body(version))
            .align_x(Alignment::Center)
            .spacing(space_xxs)
            .into()
    }

    pub fn context_ioerror(&'_ self, error: &str) -> Element<'_, Message> {
        let cosmic_theme::Spacing { space_xxs, .. } = theme::active().cosmic().spacing;

        if error.contains("denied") {
            let applications = "~/.local/share/applications/".to_string();
            let autostart = "~/.local/share/autostart/".to_string();

            widget::column()
                .push(widget::text::title4(fl!("context-denied")).align_x(Alignment::Center))
                .push(widget::text::body(fl!("context-denied-expl")).align_x(Alignment::Center))
                .push(widget::text::body(applications).align_x(Alignment::Center))
                .push(widget::text::body(autostart).align_x(Alignment::Center))
                .align_x(Alignment::Center)
                .spacing(space_xxs)
                .into()
        } else {
            widget::column()
                .push(row!(
                    horizontal_space(),
                    widget::text::title4(error.to_owned()).align_x(Alignment::Center),
                    horizontal_space()
                ))
                .align_x(Alignment::Center)
                .spacing(space_xxs)
                .into()
        }
    }

    fn create_nav_bar(&mut self) {
        let mut nav = nav_bar::Model::default();

        nav.insert()
            .text(fl!("nav-general"))
            .data::<NavPage>(NavPage::General)
            .icon(icon::from_svg_bytes(GENERAL_ICON).symbolic(true).icon())
            .activate();

        if let Some(t) = self.entry_type()
            && t == DesktopEntryType::Application
        {
            nav.insert()
                .text(fl!("nav-mimetypes"))
                .data::<NavPage>(NavPage::Mimetypes)
                .icon(icon::from_svg_bytes(MIMETYPES_ICON).symbolic(true).icon());

            nav.insert()
                .text(fl!("nav-actions"))
                .data::<NavPage>(NavPage::Actions)
                .icon(icon::from_svg_bytes(ACTIONS_ICON).symbolic(true).icon());

            nav.insert()
                .text(fl!("nav-custom"))
                .data::<NavPage>(NavPage::Custom)
                .icon(icon::from_svg_bytes(CUSTOM_ICON).symbolic(true).icon());

            nav.insert()
                .text(fl!("nav-advanced"))
                .data::<NavPage>(NavPage::Advanced)
                .icon(icon::from_svg_bytes(ADVANCED_ICON).symbolic(true).icon());
        }

        nav.activate_position(0);

        self.nav = nav;
    }

    fn create_mimetype(&mut self, mimetype: &str) {
        if let Some(entry) = &mut self.current_entry {
            // Make new list, including new one
            let mut mimes = vec![mimetype.to_string()];
            if let Some(existing) = entry.mime_type() {
                mimes.extend(
                    existing
                        .iter()
                        .filter(|s| !s.is_empty())
                        .map(ToString::to_string),
                );
            }
            // Update desktop entry
            self.set_list(DesktopKey::MimeType, &mimes);

            // Update table
            let description = self
                .mime_descriptions
                .lookup(mimetype)
                .cloned()
                .unwrap_or_default();
            let _ = self.mime_table.insert(MimeItem {
                name: mimetype.to_owned(),
                description,
            });
        }
    }

    fn clear_all(&mut self) {
        self.current_entry = None;
        self.current_entry_path = None;
        self.current_entry_error = None;
        self.mime_table.clear();
        self.new_mimetype.clear();
    }

    fn entry_type(&self) -> Option<DesktopEntryType> {
        self.current_entry
            .as_ref()
            .and_then(|e| e.type_())
            .and_then(|s| s.parse::<DesktopEntryType>().ok())
    }

    fn save_desktop_entry(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
        std::fs::write(path, contents)?; // write file contents

        // Get existing permissions
        let mut perms = std::fs::metadata(path)?.permissions();

        // OR existing mode with 0o755 (rwxr-xr-x)
        let mode = perms.mode() | 0o755;
        perms.set_mode(mode);
        std::fs::set_permissions(path, perms)?;

        Ok(())
    }
    fn load_entry_from_path(&mut self, path: &Path) {
        self.clear_all();

        if !path.exists() {
            self.current_entry_error = Some(AppError::FileNotFound(path.display().to_string()));
            return;
        }

        match DesktopEntry::from_path::<&str>(path, None) {
            Ok(entry) => {
                if let Some(mimetypes) = entry.mime_type() {
                    for item in mimetypes {
                        if !item.is_empty() {
                            let description = self
                                .mime_descriptions
                                .lookup(item)
                                .cloned()
                                .unwrap_or_default();
                            let _ = self.mime_table.insert(MimeItem {
                                name: item.to_owned(),
                                description,
                            });
                        }
                    }
                }
                self.current_entry = Some(entry);
                self.current_entry_path = Some(path.to_owned());
                self.create_nav_bar();
            }
            Err(err) => {
                self.current_entry_error = Some(AppError::Decode(err));
            }
        }
    }

    fn load_entry_from_args(&mut self) {
        self.current_entry = None;
        self.current_entry_error = None;

        let args: Vec<String> = std::env::args().collect();

        if args.len() != 2 {
            self.current_entry_error = Some(AppError::MissingArgument);
            return;
        }

        let path = std::path::Path::new(&args[1]);
        if !path.exists() {
            let path_str = format!("{:?}", path);
            self.current_entry_error = Some(AppError::FileNotFound(path_str));
            return;
        }

        self.load_entry_from_path(path);
    }

    fn get_icon_button(&self) -> impl Into<Element<'static, Message>> {
        let no_icon: &str = "<svg width=\"800px\" height=\"800px\" viewBox=\"0 0 25 25\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\">
<path d=\"M12.5 16V14.5M12.5 9V13M20.5 12.5C20.5 16.9183 16.9183 20.5 12.5 20.5C8.08172 20.5 4.5 16.9183 4.5 12.5C4.5 8.08172 8.08172 4.5 12.5 4.5C16.9183 4.5 20.5 8.08172 20.5 12.5Z\" stroke=\"red\" stroke-width=\"1.2\"/>
</svg>";

        let handle = cosmic::widget::icon::from_svg_bytes(no_icon.as_bytes().to_owned());

        let mut icon = widget::icon(handle); // default to placeholder

        if let Some(entry) = &self.current_entry
            && let Some(icon_name) = entry.groups.desktop_entry().and_then(|g| g.entry("Icon"))
            && let Some(icon_path) = self.icon_cache.lookup(icon_name)
        {
            println!("Resolved icon: {}", icon_path.display());
            let handle = cosmic::widget::icon::from_path(icon_path.to_owned());
            icon = widget::icon(handle);
        }

        widget::button::custom(icon)
            .width(90)
            .height(90)
            .on_press(Message::OpenPath(PickKind::IconFile))
    }

    pub fn key_binds() -> HashMap<KeyBind, MenuAction> {
        let mut key_binds = HashMap::new();

        macro_rules! bind {
        ([$($modifier:ident),+ $(,)?], $key:expr, $action:ident) => {{
            key_binds.insert(
                KeyBind {
                    modifiers: vec![$(Modifier::$modifier),+],
                    key: $key,
                },
                MenuAction::$action,
            );
        }};}

        bind!([Ctrl], Key::Character("o".into()), Open);
        bind!([Ctrl], Key::Character("s".into()), Save);
        bind!([Ctrl, Shift], Key::Character("s".into()), SaveAs);
        bind!([Ctrl], Key::Character("q".into()), Quit);

        key_binds
    }
}

/// The page to display in the application.
pub enum NavPage {
    General,
    Mimetypes,
    Actions,
    Custom,
    Advanced,
}

impl fmt::Display for NavPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str = match self {
            NavPage::General => fl!("nav-general"),
            NavPage::Mimetypes => fl!("nav-mimetypes"),
            NavPage::Actions => fl!("nav-actions"),
            NavPage::Custom => fl!("nav-custom"),
            NavPage::Advanced => fl!("nav-advanced"),
        };
        f.write_str(&str)
    }
}

/// The context page to display in the context drawer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    IOError(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
    Open,
    Save,
    SaveAs,
    Quit,
    None,
    RemoveMimetype(Option<usize>),
    NewApplication,
    NewLink,
    NewDirectory,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
            MenuAction::Open => Message::OpenPath(PickKind::DesktopFile),
            MenuAction::Save => Message::Save,
            MenuAction::SaveAs => Message::SaveAs,
            MenuAction::Quit => Message::Quit,
            MenuAction::None => Message::None,
            MenuAction::RemoveMimetype(pos) => Message::RemoveMimetype(*pos),
            MenuAction::NewApplication => Message::CreateEntry(DesktopEntryType::Application),
            MenuAction::NewLink => Message::CreateEntry(DesktopEntryType::Link),
            MenuAction::NewDirectory => Message::CreateEntry(DesktopEntryType::Directory),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DesktopKey {
    Type,
    Name,
    GenericName,
    Comment,
    Icon,
    Exec,
    TryExec,
    Terminal,
    Categories,
    Keywords,
    MimeType,
    Actions,
    OnlyShowIn,
    NotShowIn,
    StartupNotify,
    StartupWMClass,
    DBusActivatable,
    NoDisplay,
    Hidden,
    PrefersNonDefaultGPU,
    Implements,
    SingleMainWindow,
    Url,
    Version,
    Path,

    // endor keys
    Unknown(String),
}

impl DesktopKey {
    pub fn key_str(&self) -> Cow<'_, str> {
        match self {
            DesktopKey::Type => "Type".into(),
            DesktopKey::Name => "Name".into(),
            DesktopKey::GenericName => "GenericName".into(),
            DesktopKey::Comment => "Comment".into(),
            DesktopKey::Icon => "Icon".into(),
            DesktopKey::Exec => "Exec".into(),
            DesktopKey::TryExec => "TryExec".into(),
            DesktopKey::Terminal => "Terminal".into(),
            DesktopKey::Categories => "Categories".into(),
            DesktopKey::Keywords => "Keywords".into(),
            DesktopKey::MimeType => "MimeType".into(),
            DesktopKey::Actions => "Actions".into(),
            DesktopKey::OnlyShowIn => "OnlyShowIn".into(),
            DesktopKey::NotShowIn => "NotShowIn".into(),
            DesktopKey::StartupNotify => "StartupNotify".into(),
            DesktopKey::StartupWMClass => "StartupWMClass".into(),
            DesktopKey::DBusActivatable => "DBusActivatable".into(),
            DesktopKey::NoDisplay => "NoDisplay".into(),
            DesktopKey::Hidden => "Hidden".into(),
            DesktopKey::PrefersNonDefaultGPU => "PrefersNonDefaultGPU".into(),
            DesktopKey::Implements => "Implements".into(),
            DesktopKey::SingleMainWindow => "SingleMainWindow".into(),
            DesktopKey::Url => "URL".into(), // spec-cased
            DesktopKey::Version => "Version".into(),
            DesktopKey::Path => "Path".into(),
            DesktopKey::Unknown(k) => k.as_str().into(),
        }
    }
}

impl fmt::Display for DesktopKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key_str())
    }
}
