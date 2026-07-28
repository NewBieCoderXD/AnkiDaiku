mod actions;

use clap::{Parser, Subcommand};

#[derive(Subcommand, Debug)]
enum Actions {
  Build { dir_path: String },
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
    Actions::Build { dir_path } => {
      actions::build::execute(dir_path);
    }
  }
}
