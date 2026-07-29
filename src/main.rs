mod actions;
mod config;

use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
enum Actions {
  Build {
    dir_path: String,
    #[arg(short, long, default_value = "output.apkg")]
    output: String,
    #[arg(short = 'C', long)]
    cards_dir: Option<String>,
    #[arg(long)]
    config: Option<String>,
  },
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
  #[command(subcommand)]
  action: Actions,
}

fn main() {
  let args = Args::parse();

  match &args.action {
    Actions::Build { dir_path, output, cards_dir, config } => {
      if let Err(e) = actions::export::export_apkg(dir_path, output, cards_dir.as_deref(), config.as_deref()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
      }
    }
  }
}
