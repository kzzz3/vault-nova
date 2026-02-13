mod app_core;
mod desktop;
mod desktop_prefs;
mod password;
mod vault;

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use rpassword::prompt_password;
use vault::Entry;
use zeroize::Zeroize;

#[derive(Parser)]
#[command(
    name = "vault-nova",
    version,
    about = "A local encrypted password manager written in Rust"
)]
struct Cli {
    #[arg(long, global = true, default_value = "vault.json")]
    vault: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the desktop app (default behavior)
    Desktop,
    /// Initialize a new encrypted vault
    Init,
    /// Add or update an entry
    Add(AddArgs),
    /// Show one entry
    Get(GetArgs),
    /// List all saved entries
    List,
    /// Delete one or more entries
    Delete(DeleteArgs),
    /// Generate a strong random password
    Generate(GenerateArgs),
}

#[derive(Args)]
struct AddArgs {
    service: String,
    username: String,
    #[arg(short, long)]
    password: Option<String>,
    #[arg(short, long)]
    notes: Option<String>,
}

#[derive(Args)]
struct GetArgs {
    service: String,
    #[arg(short, long)]
    username: Option<String>,
    #[arg(long)]
    show_password: bool,
}

#[derive(Args)]
struct DeleteArgs {
    service: String,
    #[arg(short, long)]
    username: Option<String>,
}

#[derive(Args)]
struct GenerateArgs {
    #[arg(short, long, default_value_t = 20)]
    length: usize,
    #[arg(long)]
    no_numbers: bool,
    #[arg(long)]
    no_symbols: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Desktop) => desktop::run(cli.vault),
        Some(Command::Init) => init_vault(&cli.vault),
        Some(Command::Add(args)) => add_entry(&cli.vault, args),
        Some(Command::Get(args)) => get_entry(&cli.vault, args),
        Some(Command::List) => list_entries(&cli.vault),
        Some(Command::Delete(args)) => delete_entry(&cli.vault, args),
        Some(Command::Generate(args)) => {
            let generated =
                password::generate_password(args.length, !args.no_numbers, !args.no_symbols)?;
            println!("{generated}");
            Ok(())
        }
    }
}

fn init_vault(vault_path: &Path) -> Result<()> {
    let mut master_password = prompt_secret("Set master password: ")?;
    if master_password.trim().is_empty() {
        bail!("master password cannot be empty")
    }

    let mut confirm_password = prompt_secret("Confirm master password: ")?;
    if master_password != confirm_password {
        master_password.zeroize();
        confirm_password.zeroize();
        bail!("passwords do not match")
    }
    confirm_password.zeroize();

    vault::create_new(vault_path, &master_password)?;
    master_password.zeroize();
    println!("Initialized encrypted vault at {}", vault_path.display());
    Ok(())
}

fn add_entry(vault_path: &Path, args: AddArgs) -> Result<()> {
    let mut master_password = prompt_secret("Master password: ")?;
    let mut unlocked = vault::open(vault_path, &master_password)?;

    let mut secret = if let Some(password) = args.password {
        password
    } else {
        prompt_secret("Entry password: ")?
    };

    if secret.is_empty() {
        master_password.zeroize();
        secret.zeroize();
        bail!("entry password cannot be empty")
    }

    let updated_at = current_timestamp()?;
    let mut created_new = true;

    if let Some(existing) = unlocked
        .data
        .entries
        .iter_mut()
        .find(|entry| entry.service == args.service && entry.username == args.username)
    {
        existing.password = secret;
        existing.notes = args.notes;
        existing.updated_at = updated_at;
        created_new = false;
    } else {
        unlocked.data.entries.push(Entry {
            service: args.service.clone(),
            username: args.username.clone(),
            password: secret,
            notes: args.notes,
            updated_at,
        });
    }

    vault::save(vault_path, &master_password, &unlocked.salt, &unlocked.data)?;
    master_password.zeroize();

    if created_new {
        println!("Added entry for {} / {}", args.service, args.username);
    } else {
        println!("Updated entry for {} / {}", args.service, args.username);
    }

    Ok(())
}

fn get_entry(vault_path: &Path, args: GetArgs) -> Result<()> {
    let mut master_password = prompt_secret("Master password: ")?;
    let unlocked = vault::open(vault_path, &master_password)?;
    master_password.zeroize();

    let mut matches: Vec<&Entry> = unlocked
        .data
        .entries
        .iter()
        .filter(|entry| entry.service == args.service)
        .collect();

    if let Some(username) = &args.username {
        matches.retain(|entry| entry.username == *username);
    }

    if matches.is_empty() {
        bail!("no matching entry found")
    }

    if matches.len() > 1 && args.username.is_none() {
        println!("Found multiple entries for service '{}':", args.service);
        for entry in matches {
            println!("- {}", entry.username);
        }
        println!("Use --username to select one entry.");
        return Ok(());
    }

    let entry = matches[0];
    println!("Service : {}", entry.service);
    println!("Username: {}", entry.username);
    if args.show_password {
        println!("Password: {}", entry.password);
    } else {
        println!("Password: ********");
    }
    if let Some(notes) = &entry.notes {
        println!("Notes   : {notes}");
    }
    println!("Updated : {}", entry.updated_at);
    Ok(())
}

fn list_entries(vault_path: &Path) -> Result<()> {
    let mut master_password = prompt_secret("Master password: ")?;
    let unlocked = vault::open(vault_path, &master_password)?;
    master_password.zeroize();

    if unlocked.data.entries.is_empty() {
        println!("Vault is empty.");
        return Ok(());
    }

    let mut entries = unlocked.data.entries;
    entries.sort_by(|left, right| {
        left.service
            .cmp(&right.service)
            .then(left.username.cmp(&right.username))
    });

    for entry in &entries {
        println!("{} / {}", entry.service, entry.username);
    }
    println!("Total entries: {}", entries.len());
    Ok(())
}

fn delete_entry(vault_path: &Path, args: DeleteArgs) -> Result<()> {
    let mut master_password = prompt_secret("Master password: ")?;
    let mut unlocked = vault::open(vault_path, &master_password)?;

    let before = unlocked.data.entries.len();
    unlocked.data.entries.retain(|entry| {
        if let Some(username) = &args.username {
            !(entry.service == args.service && entry.username == *username)
        } else {
            entry.service != args.service
        }
    });

    let removed = before.saturating_sub(unlocked.data.entries.len());
    if removed == 0 {
        master_password.zeroize();
        bail!("no matching entry to delete")
    }

    vault::save(vault_path, &master_password, &unlocked.salt, &unlocked.data)?;
    master_password.zeroize();
    println!(
        "Deleted {removed} entr{}",
        if removed == 1 { "y" } else { "ies" }
    );
    Ok(())
}

fn prompt_secret(prompt: &str) -> Result<String> {
    prompt_password(prompt).with_context(|| format!("failed to read input for prompt: {prompt}"))
}

fn current_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is set before Unix epoch")?
        .as_secs())
}
