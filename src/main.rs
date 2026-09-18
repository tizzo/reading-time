use std::io::{self, Read, Write};
use std::process::ExitCode;

use reading_time::{estimate, Estimate, Format, Options, DEFAULT_CJK_CPM, DEFAULT_WPM};

const USAGE: &str = "\
readtime — Medium-style reading time estimates

USAGE:
    cat article.md | readtime [OPTIONS]
    readtime [OPTIONS] [FILE]...

OPTIONS:
    -w, --wpm <N>        Words per minute (default: 265, Medium's figure)
        --cjk-cpm <N>    Characters per minute for CJK text (default: 500)
    -f, --format <FMT>   auto (default), text, markdown, html
    -i, --images <N>     Use N images instead of the detected count
        --no-images      Ignore image viewing time
        --skip-code      Exclude code blocks and <pre>/<code> bodies
    -s, --seconds        Print raw seconds
    -m, --minutes        Print just the minute count
        --json           Print a JSON object
    -v, --verbose        Print a breakdown of the estimate
    -h, --help           Show this help
    -V, --version        Show version

With no FILE, or with FILE of -, reads standard input. Several files are
reported one per line with a total, like wc.

ALGORITHM:
    seconds = words / wpm * 60 + cjk_chars / cjk_cpm * 60 + image time,
    where the first image is worth 12 seconds and each later one a second
    less, down to a floor of 3. Minutes round up, with a minimum of 1.
";

#[derive(Clone, Copy, PartialEq)]
enum Output {
    Label,
    Seconds,
    Minutes,
    Json,
}

struct Config {
    opts: Options,
    output: Output,
    verbose: bool,
    files: Vec<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse_args(&args) {
        Ok(Some(config)) => config,
        Ok(None) => return ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("readtime: {msg}");
            eprintln!("Try 'readtime --help' for more information.");
            return ExitCode::from(2);
        }
    };

    match run(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("readtime: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(config: &Config) -> Result<(), String> {
    let inputs: Vec<(String, String)> = if config.files.is_empty() {
        vec![("-".to_string(), read_stdin()?)]
    } else {
        config
            .files
            .iter()
            .map(|name| read_input(name).map(|text| (name.clone(), text)))
            .collect::<Result<_, _>>()?
    };

    let results: Vec<(String, Estimate)> = inputs
        .iter()
        .map(|(name, text)| (name.clone(), estimate(text, &config.opts)))
        .collect();

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if results.len() == 1 {
        let est = &results[0].1;
        write_one(&mut out, est, config, None)?;
    } else {
        let combined = inputs
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let total = estimate(&combined, &config.opts);

        if config.output == Output::Json {
            let files: Vec<String> = results
                .iter()
                .map(|(name, est)| json_object(est, config, Some(name)))
                .collect();
            writeln!(
                out,
                "{{\"files\":[{}],\"total\":{}}}",
                files.join(","),
                json_object(&total, config, None)
            )
            .map_err(fmt_err)?;
        } else {
            for (name, est) in &results {
                write_one(&mut out, est, config, Some(name))?;
            }
            write_one(&mut out, &total, config, Some("total"))?;
        }
    }

    out.flush().map_err(fmt_err)
}

fn write_one<W: Write>(
    out: &mut W,
    est: &Estimate,
    config: &Config,
    name: Option<&str>,
) -> Result<(), String> {
    let suffix = name.map(|n| format!("  {n}")).unwrap_or_default();

    match config.output {
        Output::Json => writeln!(out, "{}", json_object(est, config, name)).map_err(fmt_err)?,
        Output::Seconds => writeln!(out, "{:.1}{suffix}", est.seconds).map_err(fmt_err)?,
        Output::Minutes => writeln!(out, "{}{suffix}", est.minutes).map_err(fmt_err)?,
        Output::Label => writeln!(out, "{}{suffix}", est.label()).map_err(fmt_err)?,
    }

    if config.verbose && config.output != Output::Json {
        writeln!(
            out,
            "  {} words at {} wpm{}{}  ({:.1}s total)",
            est.words,
            config.opts.wpm,
            if est.cjk_chars > 0 {
                format!(
                    ", {} CJK chars at {} cpm",
                    est.cjk_chars, config.opts.cjk_cpm
                )
            } else {
                String::new()
            },
            if config.opts.count_images && est.images > 0 {
                format!(
                    ", {} image{}",
                    est.images,
                    if est.images == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            },
            est.seconds,
        )
        .map_err(fmt_err)?;
    }

    Ok(())
}

fn json_object(est: &Estimate, config: &Config, name: Option<&str>) -> String {
    let named = name
        .map(|n| format!("\"name\":{},", json_string(n)))
        .unwrap_or_default();
    format!(
        "{{{named}\"minutes\":{},\"seconds\":{:.2},\"words\":{},\"cjk_chars\":{},\"images\":{},\"wpm\":{},\"text\":{}}}",
        est.minutes,
        est.seconds,
        est.words,
        est.cjk_chars,
        est.images,
        config.opts.wpm,
        json_string(&est.label()),
    )
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn read_stdin() -> Result<String, String> {
    let mut buf = Vec::new();
    io::stdin()
        .read_to_end(&mut buf)
        .map_err(|e| format!("stdin: {e}"))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_input(name: &str) -> Result<String, String> {
    if name == "-" {
        return read_stdin();
    }
    std::fs::read(name)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| format!("{name}: {e}"))
}

fn fmt_err(e: io::Error) -> String {
    e.to_string()
}

/// Returns `Ok(None)` when the flag was handled and the program should exit.
fn parse_args(args: &[String]) -> Result<Option<Config>, String> {
    let mut opts = Options {
        wpm: DEFAULT_WPM,
        cjk_cpm: DEFAULT_CJK_CPM,
        ..Options::default()
    };
    let mut output = Output::Label;
    let mut verbose = false;
    let mut files = Vec::new();
    let mut only_files = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();

        if only_files || arg == "-" || !arg.starts_with('-') {
            files.push(arg.to_string());
            i += 1;
            continue;
        }

        // Accept both `--flag value` and `--flag=value`.
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) => (f, Some(v.to_string())),
            None => (arg, None),
        };
        let mut next_value = |what: &str| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("{what} requires a value"))
        };

        match flag {
            "--" => only_files = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("readtime {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "-w" | "--wpm" => {
                opts.wpm = parse_rate(&next_value("--wpm")?, "--wpm")?;
            }
            "--cjk-cpm" => {
                opts.cjk_cpm = parse_rate(&next_value("--cjk-cpm")?, "--cjk-cpm")?;
            }
            "-f" | "--format" => {
                let value = next_value("--format")?;
                opts.format = match value.to_lowercase().as_str() {
                    "auto" => Format::Auto,
                    "text" | "txt" | "plain" => Format::Text,
                    "markdown" | "md" => Format::Markdown,
                    "html" | "htm" => Format::Html,
                    other => {
                        return Err(format!(
                            "unknown format '{other}' (want auto, text, markdown or html)"
                        ))
                    }
                };
            }
            "-i" | "--images" => {
                let value = next_value("--images")?;
                opts.images = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("--images needs a whole number, got '{value}'"))?,
                );
            }
            "--no-images" => opts.count_images = false,
            "--skip-code" => opts.skip_code = true,
            "-s" | "--seconds" => output = Output::Seconds,
            "-m" | "--minutes" => output = Output::Minutes,
            "--json" => output = Output::Json,
            "-v" | "--verbose" => verbose = true,
            other => return Err(format!("unknown option '{other}'")),
        }

        i += 1;
    }

    Ok(Some(Config {
        opts,
        output,
        verbose,
        files,
    }))
}

fn parse_rate(value: &str, flag: &str) -> Result<f64, String> {
    let rate: f64 = value
        .parse()
        .map_err(|_| format!("{flag} needs a number, got '{value}'"))?;
    if !rate.is_finite() || rate <= 0.0 {
        return Err(format!("{flag} must be greater than zero"));
    }
    Ok(rate)
}
