mod actions;
mod config;

use std::path::Path;

use clap::{Parser, Subcommand};

use actions::build;
use actions::export;

#[derive(Subcommand, Debug)]
enum Actions {
  Build {
    dir_path: String,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(short = 'C', long)]
    cards_dir: Option<String>,
    #[arg(long)]
    config: Option<String>,
  },
  Delete {
    target: String,
    #[arg(default_value = ".")]
    dir_path: String,
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
      let output = output.clone().unwrap_or_else(|| format!("{}/dist/output.apkg", dir_path));
      if let Err(e) = export::export_apkg(dir_path, &output, cards_dir.as_deref(), config.as_deref()) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
      }
    }
    Actions::Delete { target, dir_path, cards_dir, config } => {
      let root = Path::new(dir_path);
      let cfg = export::resolve_config(root, config.as_deref());
      let resolved = export::resolve_cards_dir(root, cards_dir.as_deref(),
        cfg.as_ref().and_then(|c| c.cards_dir.as_deref()));
      if let Err(e) = build::soft_delete(&resolved, target) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
      }
    }
  }
}
