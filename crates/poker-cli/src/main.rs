//! Command-line entry point for simulation and benchmark tooling.
#![forbid(unsafe_code)]

use clap::{Args, Parser, Subcommand};
use poker_cli::{PolicyKind, SimConfig, run};
use poker_core::Variant;

#[derive(Debug, Parser)]
#[command(
    name = "poker-cli",
    version,
    about = "Texas hold'em simulation and benchmark tooling"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Self-play a table with a deterministic policy and print JSON metrics.
    Sim(SimArgs),
}

#[derive(Debug, Args)]
struct SimArgs {
    /// Variant: standard or shortdeck.
    #[arg(long, default_value = "standard")]
    variant: String,
    /// Number of seats (including seat 0 as the nominal human).
    #[arg(long, default_value_t = 6)]
    players: usize,
    /// Number of hands to play (or until fewer than two players remain).
    #[arg(long, default_value_t = 1000)]
    hands: u64,
    /// Master seed for deck shuffles and agent choices.
    #[arg(long, default_value_t = 20251023)]
    seed: u64,
    /// Starting stack per seat.
    #[arg(long, default_value_t = 20000)]
    stack: u64,
    /// Small blind (standard variant).
    #[arg(long, default_value_t = 50)]
    sb: u64,
    /// Big blind (standard variant).
    #[arg(long, default_value_t = 100)]
    bb: u64,
    /// Ante (short-deck variant).
    #[arg(long, default_value_t = 40)]
    ante: u64,
    /// Minimum bet (short-deck variant).
    #[arg(long, default_value_t = 80)]
    min_bet: u64,
    /// Policy: random, checkcall or aggressive.
    #[arg(long, default_value = "random")]
    policy: String,
    /// Refill busted stacks between hands so long simulations keep running.
    #[arg(long, default_value_t = false)]
    refill: bool,
}

fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(2);
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Sim(args) => {
            let variant = match args.variant.as_str() {
                "standard" => Variant::Standard,
                "shortdeck" | "short-deck" => Variant::ShortDeck,
                other => fail(&format!("unknown variant {other}")),
            };
            let Some(policy) = PolicyKind::from_name(&args.policy) else {
                fail(&format!("unknown policy {}", args.policy));
            };
            if args.players < 2 {
                fail("need at least two seats");
            }
            if args.hands == 0 {
                fail("need at least one hand");
            }
            let config = SimConfig {
                variant,
                players: args.players,
                hands: args.hands,
                seed: args.seed,
                stack: args.stack,
                small_blind: args.sb,
                big_blind: args.bb,
                ante: args.ante,
                min_bet: args.min_bet,
                policy,
                refill: args.refill,
            };
            let outcome = run(&config);
            match serde_json::to_string_pretty(&outcome) {
                Ok(json) => println!("{json}"),
                Err(error) => fail(&format!("cannot serialize outcome: {error}")),
            }
        }
    }
}
