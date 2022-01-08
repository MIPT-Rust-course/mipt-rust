use anyhow::{bail, Context, Result};
use serde::Deserialize;
use structopt::StructOpt;

use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const CONFIG_NAME: &str = ".compose.yml";

#[derive(Deserialize)]
struct Config {
    entries: Vec<PathBuf>,
    no_copy: Vec<PathBuf>,
    no_remove: Vec<PathBuf>,
    workspace_tools: Vec<PathBuf>,
}

#[derive(StructOpt, Debug)]
#[structopt()]
struct Opts {
    /// Path to the private repo.
    #[structopt(short = "i", long = "in-path")]
    in_path: PathBuf,
    /// Path to the public repo.
    #[structopt(short = "o", long = "out-path")]
    out_path: PathBuf,
    /// Disable file processing.
    #[structopt(long = "no-process")]
    no_process: bool,
    /// Spare given directories from pruning.
    #[structopt(short = "s", long = "spare")]
    spare: Vec<PathBuf>,
    /// Add given tools to Cargo.toml
    #[structopt(short = "t", long = "add-tool")]
    add_tools: Vec<PathBuf>,
}

enum TokenKind {
    Private,
    BeginPrivate,
    EndPrivate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenProperty {
    NoHint,
    Unimplemented,
}

struct Token {
    kind: TokenKind,
    properties: Vec<TokenProperty>,
}

fn parse_token(line: &str) -> Result<Option<Token>> {
    let comment = match line.find("//") {
        Some(pos) => &line[pos..],
        None => return Ok(None),
    };

    let cmd = match comment.find("compose::") {
        Some(pos) => &comment[pos + "compose::".len()..],
        None => return Ok(None),
    };

    let kind = if cmd.starts_with("private") {
        TokenKind::Private
    } else if cmd.starts_with("begin_private") {
        TokenKind::BeginPrivate
    } else if cmd.starts_with("end_private") {
        TokenKind::EndPrivate
    } else {
        bail!("unknown compose command: {}", cmd);
    };

    let properties_str = match cmd.find("(") {
        Some(pos) => {
            if !cmd.trim_end().ends_with(")") {
                bail!("unclosed '('");
            }
            &cmd[pos + 1..cmd.trim_end().len() - 1]
        }
        None => "",
    };

    let mut properties = vec![];
    for prop in properties_str.split_inclusive(",") {
        match prop {
            "no_hint" => properties.push(TokenProperty::NoHint),
            "unimplemented" => properties.push(TokenProperty::Unimplemented),
            s => bail!("unknown property: {}", s),
        }
    }

    Ok(Some(Token { kind, properties }))
}

fn find_token(lines: &[&str], start: usize) -> Result<Option<(usize, Token)>> {
    for (i, line) in lines[start..].iter().enumerate() {
        let mb_token = parse_token(line)
            .with_context(|| format!("failed to parse token on line {}", i + 1))?;
        if let Some(token) = mb_token {
            return Ok(Some((i + start, token)));
        }
    }
    Ok(None)
}

fn process_source(src: String) -> Result<String> {
    let mut dst = String::new();

    let lines = src.lines().collect::<Vec<_>>();
    let mut next_pos = 0;
    while let Some((begin, token)) = find_token(&lines, next_pos)? {
        let end = match token.kind {
            TokenKind::EndPrivate => bail!("unpaired 'end_private' on line {}", begin + 1),
            TokenKind::Private => begin + 1,
            TokenKind::BeginPrivate => {
                let mut pos = begin + 1;
                let mut mb_end: Option<usize> = None;
                while let Some((k, token)) = find_token(&lines, pos)? {
                    match token.kind {
                        TokenKind::BeginPrivate => {
                            bail!("nested 'begin_private' on line {}", k + 1)
                        }
                        TokenKind::Private => pos = k + 1,
                        TokenKind::EndPrivate => {
                            mb_end = Some(k);
                            break;
                        }
                    }
                }
                match mb_end {
                    Some(end) => end + 1,
                    None => bail!("unclosed 'begin_private' on line {}", begin + 1),
                }
            }
        };

        for i in next_pos..begin {
            dst += lines[i];
            dst += "\n";
        }

        let no_hint = token.properties.contains(&TokenProperty::NoHint);
        let unimpl = token.properties.contains(&TokenProperty::Unimplemented);
        if no_hint {
            if begin > 0
                && lines[begin - 1].trim().is_empty()
                && end < lines.len()
                && lines[end].trim().is_empty()
            {
                next_pos = end + 1;
            } else {
                next_pos = end;
            }
        } else {
            let mut insert_line = |line: &str| {
                for c in lines[begin].chars() {
                    if c.is_whitespace() {
                        dst.push(c);
                    } else {
                        break;
                    }
                }
                dst.push_str(line);
            };

            insert_line("// TODO: your code here.\n");
            if unimpl {
                insert_line("unimplemented!()\n");
            }

            next_pos = end;
        }
    }

    for line in &lines[next_pos..] {
        dst += line;
        dst += "\n";
    }

    Ok(dst)
}

fn process_file(in_path: &Path, out_path: &Path) -> Result<()> {
    let out_dir = out_path.parent().unwrap();
    fs::create_dir_all(out_dir)
        .with_context(|| format!("failed to create dir {}", out_dir.display()))?;

    if in_path
        .to_str()
        .map(|s| s.ends_with(".rs"))
        .unwrap_or(false)
    {
        let content = fs::read_to_string(in_path)
            .with_context(|| format!("failed to read file {}", in_path.display()))?;
        let new_content = process_source(content)
            .with_context(|| format!("failed to process file {}", in_path.display()))?;
        fs::write(out_path, new_content)
            .with_context(|| format!("failed to write file {}", out_path.display()))?;
    } else {
        fs::copy(in_path, out_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                in_path.display(),
                out_path.display()
            )
        })?;
    }

    Ok(())
}

fn process_dir(
    in_path: &Path,
    out_path: &Path,
    excluded_entries: &HashSet<OsString>,
) -> Result<()> {
    let dir = fs::read_dir(in_path)
        .with_context(|| format!("failed to read dir {}", in_path.display()))?;

    for mb_entry in dir {
        let name = mb_entry
            .with_context(|| format!("failed to read entry in dir {}", in_path.display()))?
            .file_name();

        if excluded_entries.contains(&name) {
            continue;
        }

        let new_in_path = in_path.join(&name);
        let new_out_path = out_path.join(&name);

        if new_in_path.is_dir() {
            process_dir(&new_in_path, &new_out_path, excluded_entries)?;
        } else {
            process_file(&new_in_path, &new_out_path)?;
        }
    }
    Ok(())
}

fn read_config(path: &Path) -> Result<Config> {
    let mut file = fs::File::open(path).context(format!("failed to open {}", path.display()))?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .context(format!("failed to read {}", path.display()))?;

    serde_yaml::from_slice(&buffer).context("failed to parse config")
}

fn process_entries(args: &Opts, config: &Config) -> Result<()> {
    let excluded_entries = config
        .no_copy
        .iter()
        .map(|p| p.clone().into_os_string())
        .collect::<HashSet<_>>();

    for entry in config.entries.iter() {
        let in_path = args.in_path.join(entry);
        let out_path = args.out_path.join(entry);
        if in_path.is_dir() {
            process_dir(&in_path, &out_path, &excluded_entries)?;
        } else {
            process_file(&in_path, &out_path)?;
        }
    }

    Ok(())
}

fn prune_entries(args: &Opts, config: &Config) -> Result<()> {
    let spare = config
        .entries
        .iter()
        .chain(config.no_remove.iter())
        .chain(args.spare.iter())
        .cloned()
        .collect::<HashSet<_>>();

    let dir = fs::read_dir(&args.out_path)
        .with_context(|| format!("failed to read dir {}", args.out_path.display()))?;
    for mb_entry in dir {
        let path = mb_entry
            .with_context(|| format!("failed to read entry in dir {}", args.out_path.display()))?
            .path();

        if !spare.contains(Path::new(path.file_name().unwrap())) {
            let res = if path.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            res.with_context(|| format!("failed to remove {:?}", path.display()))?;
        }
    }

    Ok(())
}

fn write_root_cargo(args: &Opts, config: &Config) -> Result<()> {
    let mut tasks = vec![];
    for entry in config.entries.iter() {
        if args.out_path.join(entry).join("Cargo.toml").exists() {
            tasks.push(
                entry
                    .to_str()
                    .with_context(|| format!("entry is not utf-8: {}", entry.display()))?,
            );
        }
    }

    let tools = config
        .workspace_tools
        .iter()
        .chain(args.add_tools.iter())
        .map(|t| t.to_str().unwrap())
        .collect::<Vec<_>>();

    let content = format!(
        r#"[workspace]
members = [
    # Tasks
    "{}",

    # Tools
    "{}",
]
"#,
        tasks.join("\",\n    \""),
        tools.join("\",\n    \"")
    );

    fs::write(args.out_path.join("Cargo.toml"), content).context("failed to write Cargo.toml")?;

    Ok(())
}

fn do_main(args: Opts) -> Result<()> {
    let config = read_config(&args.in_path.join(CONFIG_NAME)).context("failed to read config")?;

    if !args.no_process {
        process_entries(&args, &config).context("failed to process entries")?;
    }

    prune_entries(&args, &config).context("failed to prune entries")?;

    write_root_cargo(&args, &config).context("failed to write root Cargo.toml")
}

fn main() {
    let args = Opts::from_args();

    if let Err(err) = do_main(args) {
        eprintln!("Error: {:#}", err);
        std::process::exit(1);
    }
}
