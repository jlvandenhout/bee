// Copyright 2020-2021 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

//! A logger backend for the `log` crate.

mod config;

pub use config::{LoggerConfig, LoggerConfigBuilder, LoggerOutputConfig, LoggerOutputConfigBuilder};

use thiserror::Error;

/// Name of the standard output.
pub const LOGGER_STDOUT_NAME: &str = "stdout";

/// Error occuring when initializing a logger backend.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Creating output file failed.
    #[error("Creating output file failed.")]
    CreatingFileFailed,
    /// Initializing the logger backend failed.
    #[error("Initializing the logger backend failed.")]
    InitializationFailed,
}

#[cfg(not(feature = "tokio-console"))]
macro_rules! log_format {
    ($target:expr, $level:expr, $message:expr, $target_width:expr, $level_width:expr) => {
        format_args!(
            "{} {:target_width$} {:level_width$} {}",
            crate::time::format(&crate::time::now_local()),
            $target,
            $level,
            $message,
            target_width = $target_width,
            level_width = $level_width
        )
    };
}

/// Initializes a logger backend for running with the `console` feature.
#[cfg(feature = "tokio-console")]
pub fn logger_init(config: LoggerConfig) -> Result<(), Error> {
    use tracing_subscriber::{
        filter::{self, Targets},
        prelude::*,
    };

    let targets = match config.outputs.iter().find(|output| output.name == LOGGER_STDOUT_NAME) {
        Some(output) => {
            let level_filter = output
                .level_filter
                .to_string()
                .parse::<filter::LevelFilter>()
                .unwrap_or(filter::LevelFilter::INFO);

            if output.target_filters.is_empty() && output.target_exclusions.is_empty() {
                Targets::default().with_default(level_filter)
            } else {
                let mut targets = Targets::default();

                for filter in &output.target_filters {
                    targets = targets.with_target(filter, level_filter);
                }

                for exclusion in &output.target_exclusions {
                    targets = targets.with_target(exclusion, filter::LevelFilter::OFF);
                }

                targets
            }
        }
        None => Targets::default().with_default(filter::LevelFilter::INFO),
    };

    console_subscriber::build()
        .with(tracing_subscriber::fmt::layer().with_filter(targets))
        .init();

    Ok(())
}

/// Initializes a `fern` logger backend for the `log` crate.
///
/// # Arguments
///
/// * `config`  -   Logger configuration
#[cfg(not(feature = "tokio-console"))]
pub fn logger_init(config: LoggerConfig) -> Result<(), Error> {
    use fern::{
        colors::{Color, ColoredLevelConfig},
        Dispatch,
    };

    let target_width = config.target_width;
    let level_width = config.level_width;

    let mut logger = if config.color_enabled {
        let colors = ColoredLevelConfig::new()
            .trace(Color::BrightMagenta)
            .debug(Color::BrightBlue)
            .info(Color::BrightGreen)
            .warn(Color::BrightYellow)
            .error(Color::BrightRed);

        // Creates a logger dispatch with color support.
        Dispatch::new().format(move |out, message, record| {
            out.finish(log_format!(
                record.target(),
                colors.color(record.level()),
                message,
                target_width,
                level_width
            ))
        })
    } else {
        // Creates a logger dispatch without color support.
        Dispatch::new().format(move |out, message, record| {
            out.finish(log_format!(
                record.target(),
                record.level(),
                message,
                target_width,
                level_width
            ))
        })
    };

    for output in config.outputs {
        // Creates a logger dispatch for each output of the configuration.
        let mut dispatch = Dispatch::new().level(output.level_filter);

        if !output.target_filters.is_empty() {
            let target_filters = output.target_filters;
            dispatch = dispatch.filter(move |metadata| {
                let target = metadata.target().to_lowercase();
                target_filters.iter().any(|f| target.contains(f))
            });
        }

        if !output.target_exclusions.is_empty() {
            let target_exclusions = output.target_exclusions;
            dispatch = dispatch.filter(move |metadata| {
                let target = metadata.target().to_lowercase();
                !target_exclusions.iter().any(|f| target.contains(f))
            });
        }

        // Special case for the standard output.
        dispatch = if output.name == LOGGER_STDOUT_NAME {
            dispatch.chain(std::io::stdout())
        } else {
            dispatch.chain(fern::log_file(output.name).map_err(|_| Error::CreatingFileFailed)?)
        };

        logger = logger.chain(dispatch);
    }

    logger.apply().map_err(|_| Error::InitializationFailed)?;

    Ok(())
}