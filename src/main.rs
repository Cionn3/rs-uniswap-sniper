mod utils;
mod forked_db;
mod oracles;
mod bot;

use fern::colors::{Color, ColoredLevelConfig};
use utils::helpers::create_local_client;
use bot::bot_config::BotConfig;

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    // setup logger configs
let mut colors = ColoredLevelConfig::new();
colors.trace = Color::Cyan;
colors.debug = Color::Magenta;
colors.info = Color::Green;
colors.warn = Color::Red;
colors.error = Color::BrightRed;

// setup logging both to stdout and file
fern::Dispatch::new()
    .format(move |out, message, record| {
        out.finish(format_args!(
            "{}[{}] {}",
            chrono::Local::now().format("[%H:%M:%S]"),
            colors.color(record.level()),
            message
        ))
    })
    .chain(std::io::stdout())
    .chain(fern::log_file("output.log")?)
    // hide all logs for everything other than bot
    .level(log::LevelFilter::Error)
    .level_for("rs-uniswap-sniper", log::LevelFilter::Info)
    .apply()?;

    let client = create_local_client().await?;

    let mut bot_config = BotConfig::new(client.clone()).await?;

    bot_config.start().await;

    // start the snipe bot

    Ok(())

}
