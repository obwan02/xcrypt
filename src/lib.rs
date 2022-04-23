use clap::{ArgGroup, Parser};
use flexi_logger::LoggerHandle;
use log::{error, info, warn};
use std::{
    fs::{File, OpenOptions},
    io::{stdin, Read, Write},
    process::exit,
};

mod kl_anal;
mod key_guess;

#[derive(Parser, Debug)]
#[clap(author = "Oliver W. (obwan02)", version, about = "An Xor ANALyser for cryptanalysis", long_about = None)]
#[clap(group(
            ArgGroup::new("method")
                .required(true)
                .args(&["most-common-byte", "crib"])
           ))]
pub struct Config {
    /// The file to analyse
    ///
    /// Specifies the input file for xanal to analyse.
    /// A '-' can be provided to read from stdin. If reading
    /// from stdin, the program will output after an EOF.
    #[clap(required = true)]
    file: String,

    /// The output file to write the decrypted output to
    ///
    /// This flag is optional. If provided, the decrypted output
    /// will be written to the specified file. It will only write to
    /// the file if there are no errors during other stages of the program.
    #[clap(short, long)]
    output_file: Option<String>,

    /// The maximum key length to check for key length analysis
    ///
    /// During the key length analysis stage, a range of
    /// key lengths are checked, and the key length that
    /// gives the closest ic value to the target ic value is
    /// selected. The max-key-length param is the upper bound
    /// (inclusive) of the key lengths to check.
    #[clap(long, default_value_t = 32)]
    max_key_length: usize,

    /// A specific key length to use to guess the key (skips key length analysis)
    ///
    /// Using this options skips the key length analysis stage,
    /// and instead of guessing the key length, uses the one  
    /// provided.
    #[clap(short = 'k', long = "key-length")]
    specific_key_length: Option<usize>,

    /// The target index of coincidence to use for key length analysis.
    ///
    /// The key length analysis is done through comparing index of
    /// coincidences for different key lengths. The key length that
    /// gives the closest index of coincidence to the target ic is the
    /// key length that is chosen. This argument specifies the target ic
    /// to compare against (not normalised). By default it is the index of coincidence of
    /// the english langauge.
    #[clap(short, long = "target-ic", default_value_t = 0.067)]
    target_ic: f32,

    /// The most common byte for key analysis.
    ///
    /// This is the default analysis option that is used when no arguments are
    /// passed to xanal. The default most common byte is 0x20, which is the space
    /// character. This flag cannot be used in conjunction with the 'crib' flag, which
    /// uses crib analysis for guessing the key.
    #[clap(short = 'c', long, default_value_t = 0x20)]
    most_common_byte: u8,

    /// The crib to be used for key analysis.
    ///
    /// The crib flag specifies that a crib will be used to
    /// find the key. This is an alternative to analysis by
    /// most common byte. Cannot be used in conjunction with the
    /// 'most_common_byte' flag. This flag requires one of
    /// the 'crib-offset' or 'crib-search' flags to be set.
    #[clap(long = "crib", requires = "crib-method")]
    crib: Option<String>,

    /// The crib offset to be used for key analysis
    ///
    /// The crib flag specifies that a key analysis should be done using
    /// a crib. Specifying this flag (crib-offset) uses the mode of crib offset
    /// to recover the key. Cannot be used in conjunction with 'crib-search' flag.
    #[clap(long = "crib-offset", requires = "crib", group = "crib-method")]
    crib_offset: Option<usize>,

    /// The crib search to be used for key analysis
    ///
    /// The crib flag specifies that a key analysis should be done using
    /// a crib. Specifying this flag (crib-search) uses the mode of crib search
    /// to recover the key. This method works by testing the crib in all possible
    /// positions and seeing if the search term appears in the ouptut.
    #[clap(long = "crib-search", requires = "crib", group = "crib-method")]
    crib_search: Option<String>,

    /// Specifies if the output should be verbose or not
    #[clap(short, long)]
    pub verbose: bool,

    /// Specifies if the output shouldn't be colored
    ///
    /// This option is mainly for use in scripts and other
    /// programs where you just want raw standard ouptut
    #[clap(long)]
    pub no_color_output: bool,

    /// Specifies that only key length analysis should be run
    ///
    /// This option will make only the key length analysis portion
    /// of the program run.
    #[clap(short = 'l', long)]
    pub key_length_only: bool,
}

pub fn decrypt<'a>(data: &'a [u8], key: &'a [u8]) -> impl Iterator<Item = u8> + 'a {
    data.iter()
        .enumerate()
        .map(|(i, &x)| x ^ key[i % key.len()])
}

fn read_input(config: &Config) -> Vec<u8> {
    let mut buf = vec![];

    if config.file == "-" {
        if let Err(e) = stdin().lock().read_to_end(&mut buf) {
            error!("Failed to read from stdin because '{}'", e);
            exit(-1);
        }

        return buf;
    }

    match File::open(&config.file) {
        Ok(mut file) => {
            if let Err(e) = file.read_to_end(&mut buf) {
                error!("Failed to read file '{}' because '{}'", &config.file, e);
                exit(-1);
            }
        }
        Err(e) => {
            error!("Failed to open file '{}' because '{}'", &config.file, e);
            exit(-1);
        }
    }

    buf
}

pub fn run(config: Config, mut logger: LoggerHandle) {
    use key_guess::*;

    if config.verbose {
        logger.parse_new_spec("debug").unwrap();
    }

    let data = read_input(&config);

    let key_length = if let Some(x) = config.specific_key_length {
        info!("Using Key Length: {}", x);
        x
    } else {
        let x = kl_anal::analyse_key_length(&data, config.max_key_length, 0.067);
        info!("Key Length Guess: {}", x);
        x
    };

    if data.len() / key_length < 26 {
        warn!("The selected key length probably does not give enough data to analyse");
    }

    if config.key_length_only {
        return;
    }

    let method = match (&config.crib, &config.crib_offset) {
        (Some(crib), Some(offset)) => {
            info!(
                "Using crib: '{}', which has length {}",
                crib,
                crib.as_bytes().len()
            );
            GuessMethod::Crib(crib.as_bytes(), *offset)
        }
        (Some(_), None) => panic!("Only one crib option given"),
        (None, Some(_)) => panic!("Only one crib option given"),
        _ => GuessMethod::MostCommon(config.most_common_byte),
    };

    let key_guess = guess_key(&data, key_length, method);

    if let Ok(guess) = &key_guess {
        info!("Key Guess: {}", String::from_utf8_lossy(guess));
        info!("Key Guess (base64): {}", base64::encode(guess));
        info!("Key Guess (hex): 0x{}", hex::encode(guess));
    } else {
        error!("Error while guessing key: {}", key_guess.unwrap_err());
        return;
    }

    let key_guess = key_guess.unwrap();

    if let Some(output_file) = config.output_file {
        let mut options = OpenOptions::new();
        options.write(true).create(true);

        let data: Vec<u8> = decrypt(&data, &key_guess).collect();

        match options.open(output_file) {
            Ok(mut file) => { 
                if let Err(e) = file.write(&data) {
                    error!("Failed to write to output file: {}", e);
                }
            },
            Err(e) => error!("Failed to open output file: {}", e),
        };
    }
}