// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod config;
mod i18n;
mod mimelist;
mod xdghelp;

use chrono::Local;
use cosmic::iced::Limits;
use log::info;
use std::io;

fn setup_logger() -> Result<(), Box<dyn std::error::Error>> {
    fern::Dispatch::new()
        .level(log::LevelFilter::Warn)
        .level_for("launchedit", log::LevelFilter::Debug)
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                Local::now().format("%H:%M:%S"),
                record.level(),
                message
            ));
        })
        .chain(io::stdout())
        .apply()?;
    Ok(())
}

fn main() -> cosmic::iced::Result {
    setup_logger().expect("Failed to initialize logger");

    info!("Application started");
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    let settings = cosmic::app::Settings::default().size_limits(
        Limits::NONE
            .min_width(360.0)
            .min_height(300.0)
    );

    // Starts the application's event loop with `()` as the application's flags.
    cosmic::app::run::<app::AppModel>(settings, ())
}
