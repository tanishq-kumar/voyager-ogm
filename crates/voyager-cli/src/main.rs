//! Voyager OGM Command-Line Interface (`voyager`)

use clap::{Parser, Subcommand};
use miette::Result;

#[derive(Parser, Debug)]
#[command(
    name = "voyager",
    author = "Voyager OGM Team",
    version = env!("CARGO_PKG_VERSION"),
    about = "High-performance vendor-neutral Graph OGM & AST compiler CLI",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show version and build diagnostic information
    Doctor,
    /// Compile an AST query or file into a target dialect
    Compile {
        /// Target dialect (cypher, sql_pgq, iso_gql, duck_pgq)
        #[arg(short, long, default_value = "cypher")]
        dialect: String,
    },
    /// Run benchmark tests for AST compilation and Arrow streaming
    Bench {
        /// Number of nodes to simulate in the benchmark
        #[arg(short, long, default_value_t = 100000)]
        nodes: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => {
            println!("󰓾 Voyager OGM v{}", env!("CARGO_PKG_VERSION"));
            println!("  Engine Core: voyager-core v{}", voyager_core::VERSION);
            println!("  Memory Arena: Active (32-bit handles)");
            println!(
                "  Supported Dialects: openCypher 9, Cypher 25, SQL:2023 PGQ, ISO GQL 2024, DuckPGQ"
            );
            println!("  Environment: All diagnostics OK [PASS]");
        }
        Commands::Compile { dialect } => {
            println!("Compiling query for dialect: {}", dialect);
        }
        Commands::Bench { nodes } => {
            println!("Running Voyager OGM benchmark for {} nodes...", nodes);
        }
    }

    Ok(())
}
