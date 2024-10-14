use clap::{Command, Arg, ArgAction};

use std::process;

fn main() {
    let matches = Command::new("rchef")
        .version("0.1")
        .author("boolean_coercion <booleancoercion@gmail.com>")
        .about("A fully-featured interpreter for the esoteric programming language Chef.")
        .arg(
            Arg::new("spaced")
                .help("Determines whether your program will run in spaced mode.")
                .short('s')
                .long("spaced")
                .action(ArgAction::SetTrue) // Sets to true if flag is present
                .default_value("false"),    // Default is false if flag is absent
        )
        .arg(
            Arg::new("filename")
                .help("The filename of the program you intend to run.")
                .required(true),
        )
        .get_matches();

        let filename = matches.get_one::<String>("filename").unwrap(); // filename is required
        let spaced = matches.get_flag("spaced");

    if let Err(why) = rchef::run(filename, spaced) {
        eprintln!("{}", why);
        process::exit(1);
    }
}
