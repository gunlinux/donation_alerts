//! CLI entry point for the DonationAlerts consumer.

mod auth;
mod config;
mod stream;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "donationalerts",
    about = "DonationAlerts consumer: OAuth helper + live donation printer"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the OAuth2 authorization URL to open in a browser.
    #[command(alias = "auth", alias = "url")]
    Login,
    /// Exchange the authorization code from the redirect URL for an access
    /// token and store it in .env.
    #[command(alias = "exchange")]
    Token {
        /// The `code` query parameter from the redirect URL.
        code: String,
    },
    /// Connect to DonationAlerts and print incoming donations as JSON lines.
    #[command(alias = "run")]
    Listen,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Login => {
            let config = config::Config::load()?;
            println!("{}", auth::authorize_url(&config)?);
        }
        Command::Token { code } => {
            let config = config::Config::load()?;
            auth::exchange_and_store(&config, &code).await?;
        }
        Command::Listen => {
            let config = config::Config::load()?;
            stream::listen(&config).await?;
        }
    }

    Ok(())
}
