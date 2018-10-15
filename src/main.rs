#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate lazy_static;

extern crate reqwest;
extern crate serde;
extern crate toml;

#[macro_use]
extern crate futures;
extern crate tokio;

extern crate clap;
extern crate indicatif;

mod extract;
mod providers;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::io::prelude::*;
use std::error::Error;

use futures::{Future, Stream};

use clap::{App, AppSettings, SubCommand};
use indicatif::{HumanDuration, MultiProgress, ProgressStyle, ProgressBar};
use std::time::{Duration, Instant};

const TEMP_DIR: &'static str = ".wam-temp";
const ADDON_DIR_PATH: &'static str = "Interface/Addons";

const CONFIG_FILE_PATH: &'static str = "wam.toml";
const LOCK_FILE_PATH: &'static str = "wam-lock.toml";

#[derive(Serialize, Deserialize, Debug)]
struct ConfigFile {
    pub config: Option<GlobalConfig>,
    pub addons: Vec<Addon>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GlobalConfig {
    pub parallel: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Addon {
    pub name: String,
    pub provider: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct LockFile {
    pub addons: Vec<AddonLock>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct AddonLock {
    pub name: String,
    pub resolved: String,
    // do i even need this? timestamp is always better for comparing
    // keeping it for now for displaying information about installed addons
    pub version: String,
    pub timestamp: u64,
}

lazy_static! {
    static ref LOCK: LockFile = {
        let lock_path = Path::new(LOCK_FILE_PATH);

        if !lock_path.exists() || !lock_path.is_file() {
            LockFile { addons: Vec::new() }
        } else {
            let mut contents = String::new();
            let mut f = File::open(lock_path).unwrap();
            f.read_to_string(&mut contents).unwrap();
            toml::from_str::<LockFile>(&contents)
                .expect("failed to parse lock file")
        }
    };
}

lazy_static! {
    static ref ADDON_DIR: PathBuf = {
        let addon_dir = Path::new(ADDON_DIR_PATH);
        if !addon_dir.is_dir() {
            fs::create_dir_all(addon_dir).unwrap();
        }

        addon_dir.to_path_buf()
    };
}

lazy_static! {
    static ref SPINNER_STYLE: ProgressStyle = {
        ProgressStyle::default_spinner()
            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
            .template("{prefix:.bold.dim} {spinner} {wide_msg}")
    };
}

fn main() {
    let started = Instant::now();

    let app = App::new("wam")
        .version("0.1")
        .author("Hilmar Wiegand <me@hwgnd.de>")
        .about("WoW Addon Manager")
        .setting(AppSettings::ArgRequiredElseHelp)
        .subcommands(vec![
            SubCommand::with_name("install")
                .about("install new addons and update existing ones"),

            SubCommand::with_name("add")
                .about("add and install a new addon")
                .args_from_usage("[NAME] 'addon name in format <provider>/<name>'"),

            SubCommand::with_name("remove")
                .about("not implemented"),

            SubCommand::with_name("search")
                .about("not implemented"),
        ]);

    let matches = app.get_matches();

    if let Some(_) = matches.subcommand_matches("install") {
        match install() {
            Err(err) => println!("an error occurred: {:?}", err),
            _ => println!("all done!"),
        };
    }

    if let Some(matches) = matches.subcommand_matches("add") {
        let name = String::from(matches.value_of("NAME").unwrap());

        match add(name) {
            Err(err) => println!("add error occurred: {:?}", err),
            _ => println!("added!"),
        };
    }

    delete_temp_dir().unwrap();

    println!("Done in {}", HumanDuration(started.elapsed()));
}

fn add(name: String) -> Result<(), Box<Error>> {
    let mut f = File::open(CONFIG_FILE_PATH)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let mut parsed: ConfigFile = toml::from_str(&contents)?;

    let name = name.to_lowercase();
    let name_parts = name.split("/").collect::<Vec<&str>>();
    if name_parts.len() != 2 {
        println!("please use the format <provider>/<addon>");
        return Ok(());
    }

    if let Some(_) = LOCK.addons.iter().find(|&it| it.name == name) {
        println!("already installed!");
        return Ok(());
    }

    let provider = String::from(name_parts[0]);
    let name = String::from(name_parts[1]);

    let addon = Addon { name, provider };
    let addon_for_lock = addon.clone();

    let _temp_dir = create_temp_dir()?;

    let m = MultiProgress::new();
    let m2 = MultiProgress::new();

    let add_future = |f: providers::AddonLockFuture| { f
        .and_then(move |it| providers::download_addon(it, m.add(ProgressBar::new(0))))
        .map_err(|err| println!("error downloading: {}", err))
        .map(move |result| {
            match result {
                Some((downloaded, lock)) => {
                    println!("downloaded {}, extracting...", lock.name);
                    extract::extract_zip(downloaded, &ADDON_DIR);
                    println!("done with {}", lock.name);
                    Ok(lock)
                },
                _ => Err(String::from("download failed")),
            }
        })
        .map(move |lock| {
            let lock_path = Path::new(&LOCK_FILE_PATH);
            let _ = save_lock_file(&lock_path, &LOCK, &vec![lock.unwrap()]);

            parsed.addons.push(addon);
            let config_str = toml::to_string(&parsed).unwrap();
            let mut f = File::create(CONFIG_FILE_PATH).unwrap();
            f.write_all(config_str.as_bytes()).unwrap();
        })
    };

    let pb = m2.add(ProgressBar::new(0));
    match providers::get_lock((addon_for_lock, None), pb).map(add_future) {
        Some(add_future) => tokio::run(add_future),
        _ => println!("addon not found"),
    };

    Ok(())
}

fn install() -> Result<(), Box<Error>> {
    let mut f = File::open(CONFIG_FILE_PATH)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let parsed: ConfigFile = toml::from_str(&contents)?;

    let config = match parsed.config {
        Some(config) => config,
        _ => GlobalConfig {
            parallel: Some(5),
        }
    };

    let _temp_dir = create_temp_dir()?;

    let parsed_with_locks = parsed.addons.into_iter().map(|it| {
        let maybe_lock = find_existing_lock(&it);
        (it, maybe_lock)
    }).collect::<Vec<(Addon, Option<AddonLock>)>>();

    let m = MultiProgress::new();
    let m2 = MultiProgress::new();

    let install_future = futures::stream::iter_ok(parsed_with_locks)
        .filter_map(move |it| providers::get_lock(it, m.add(ProgressBar::new(0))))
        .buffer_unordered(config.parallel.unwrap_or(5))
        .filter(|(addon, lock)| find_existing_lock(&addon)
            .map(|found| lock.timestamp > found.timestamp)
            .unwrap_or(true)
        )
        .collect()
        .map(futures::stream::iter_ok)
        .flatten_stream()
        .filter_map(move |it| providers::download_addon(it, m2.add(ProgressBar::new(0))))
        .buffer_unordered(config.parallel.unwrap_or(5))
        .map(move |(downloaded, lock)| {
            extract::extract_zip(downloaded, &ADDON_DIR);
            lock
        })
        .collect()
        .map(|new_locks| {
            let lock_path = Path::new(&LOCK_FILE_PATH);
            let _ = save_lock_file(&lock_path, &LOCK, &new_locks);
        })
        // TODO: do error handling here
        .map_err(|_| ());

    tokio::run(install_future);
    m.join();
    m2.join_and_clear();

    Ok(())
}

fn find_existing_lock(addon: &Addon) -> Option<AddonLock> {
    LOCK.addons.iter().find(|it| {
        it.name == format!("{}/{}", addon.provider, addon.name)
    }).map(Clone::clone)
}

fn create_temp_dir() -> Result<PathBuf, Box<Error>> {
    let temp_dir = Path::new(TEMP_DIR);
    if temp_dir.exists() && temp_dir.is_dir() {
        fs::remove_dir_all(temp_dir)?;
    }

    fs::create_dir(temp_dir)?;

    Ok(temp_dir.to_path_buf())
}

fn delete_temp_dir() -> Result<(), Box<Error>> {
    let temp_dir = Path::new(TEMP_DIR);
    if temp_dir.exists() && temp_dir.is_dir() {
        fs::remove_dir_all(temp_dir)?;
    }

    Ok(())
}

fn save_lock_file(
    path: &Path, old_lock: &LockFile,
    new_locks: &Vec<AddonLock>
) -> Result<(), Box<Error>> {
    let mut locks = old_lock.clone();
    for lock in new_locks {
        let existing = old_lock.addons
            .iter().enumerate().find(|(_, it)| {
                it.name == lock.name
            });

        if let Some((i, _)) = existing {
            locks.addons[i] = lock.clone();
        } else {
            locks.addons.push(lock.clone());
        }
    }

    let lock_str = toml::to_string(&locks)?;
    
    // recreate the file because we want to overwrite anyways
    let mut f = File::create(path)?;
    f.write_all(lock_str.as_bytes())?;

    Ok(())
}
