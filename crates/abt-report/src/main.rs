//! `abt-report build` renders the site; `abt-report fixtures` writes the
//! sample set the tests build from.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "abt-report", about = "The static report site of the pipeline (PL-7.4)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Renders every page into --out from the log and the data layer, or from --fixtures.
    Build {
        /// The broker list (`PIPELINE_BROKERS` when omitted).
        #[arg(long)]
        brokers: Option<String>,
        /// The data service's Unix socket.
        #[arg(long)]
        data: Option<PathBuf>,
        /// The output directory.
        #[arg(long)]
        out: PathBuf,
        /// A fixtures directory in place of the broker and the data socket.
        #[arg(long)]
        fixtures: Option<PathBuf>,
        /// Also read the dead-letter topics (`dlq.triage`, `dlq.analyze`, `dlq.strategize`).
        #[arg(long)]
        dlq: bool,
        /// A JSON file of the machine's facts, shown on the Operations page.
        #[arg(long)]
        facts: Option<PathBuf>,
        /// Seconds to wait on the broker per poll.
        #[arg(long, default_value = "10")]
        timeout: u64,
    },
    /// Writes the sample fixtures (the set the tests build the site from).
    Fixtures {
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    match Cli::parse().command {
        Command::Build {
            brokers,
            data,
            out,
            fixtures,
            dlq,
            facts,
            timeout,
        } => {
            let mut inputs = match &fixtures {
                Some(dir) => abt_report::fixtures::load(dir).with_context(|| dir.display().to_string())?,
                None => {
                    let brokers = brokers
                        .or_else(|| std::env::var("PIPELINE_BROKERS").ok())
                        .context("--brokers or PIPELINE_BROKERS is required without --fixtures")?;
                    let mut inputs = abt_report::Inputs::default();
                    let mut topics: Vec<&str> = abt_report::log::TOPICS.to_vec();
                    let dlq_topics = ["dlq.triage", "dlq.analyze", "dlq.strategize"];
                    if dlq {
                        topics.extend(dlq_topics);
                    }
                    abt_report::log::read(&brokers, &topics, &mut inputs, Duration::from_secs(timeout))?;
                    match &data {
                        Some(socket) => abt_report::data::read(socket, &mut inputs)?,
                        None => tracing::warn!("no --data socket: the report tables and the audit are not read"),
                    }
                    inputs
                }
            };
            if let Some(f) = facts {
                let text = std::fs::read_to_string(&f).with_context(|| f.display().to_string())?;
                inputs.facts = Some(serde_json::from_str(&text).with_context(|| f.display().to_string())?);
            }
            let built = abt_report::build(&inputs, &out)?;
            println!(
                "{} pages for {} strategies into {} in {:.3} s",
                built.pages.len(),
                built.strategies,
                built.out.display(),
                built.elapsed.as_secs_f64()
            );
        }
        Command::Fixtures { out } => {
            let n = abt_report::sample::write(&out)?;
            println!("{} files into {}", n, out.display());
        }
    }
    Ok(())
}
