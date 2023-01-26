use std::error::Error;
use std::sync::Arc;
use envconfig::Envconfig;
use crate::bot::Bot;
use crate::models::Env;

mod models;
mod tg;
mod db;
mod parser;
mod bot;
mod errors;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    env_logger::builder().filter(None, log::LevelFilter::Info).init();
    let env = Env::init_from_env()?;
    let bot = bot::BotDeps::new(&env).await?;
    let arced = Arc::new(bot);
    let bot = Bot { dependency: arced.clone() };
    let task_bot = Bot { dependency: arced };
    log::info!("Starting background task");
    let handle = task_bot.run_background_task();

    log::info!("Starting bot");
    bot.run().await?;
    handle.await?;
    Ok(())
}
