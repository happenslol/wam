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

#[macro_use]
extern crate clap;
use clap::{App, AppSettings};

mod extract;
mod providers;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::io::prelude::*;
use std::error::Error;

use futures::{Future, Stream};

const TEMP_DIR: &'static str = ".wam-temp";
const ADDONS_DIR: &'static str = "Interface/Addons";

const CONFIG_FILE_PATH: &'static str = "wam.toml";
const LOCK_FILE_PATH: &'static str = "wam-lock.toml";

#[derive(Serialize, Deserialize, Debug)]
struct ConfigFile {
    pub addons: Vec<Addon>,
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

fn main() {
    let cli_yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(cli_yaml)
        .setting(AppSettings::ArgRequiredElseHelp)
        .get_matches();

    if let Some(_) = matches.subcommand_matches("install") {
        match install() {
            Err(err) => println!("an error occurred: {:?}", err),
            _ => println!("all done!"),
        };
    }

    delete_temp_dir().unwrap();
}

fn install() -> Result<(), Box<Error>> {
    let mut f = File::open(CONFIG_FILE_PATH)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let parsed: ConfigFile = toml::from_str(&contents)?;

    let addon_dir: &'static Path = &Path::new(ADDONS_DIR);
    if !addon_dir.is_dir() {
        fs::create_dir_all(addon_dir).unwrap()
    }

    let _temp_dir = create_temp_dir()?;

    let install_future = futures::stream::iter_ok::<_, String>(parsed.addons)
        .filter_map(move |addon| {
            println!("processing {}", addon.name);
            let found = LOCK.addons.iter().find(|it| {
                // addons are unique over their name and provider
                format!("{}/{}", addon.provider, addon.name) == it.name
            });

            let addon_lock = match found {
                Some(found) => Some(found.clone()),
                _ => providers::get_lock(&addon, None),
            };

            match addon_lock {
                Some(addon_lock) => providers::download_addon(
                    &addon,
                    &addon_lock
                ),
                _ => panic!("error"),
            }
        })
        .buffer_unordered(8)
        .map(move |(downloaded, lock)| {
            println!("downloaded {}, extracting...", lock.name);
            extract::extract_zip(downloaded, addon_dir.to_path_buf());
            println!("done with {}", lock.name);
            lock
        })
        .collect()
        .map_err(|err| println!("error: {:?}", err))
        .then(move |new_locks| {
            println!("got new locks: {:?}", new_locks);
            let lock_path = Path::new(&LOCK_FILE_PATH);
            let _ = save_lock_file(&lock_path, &LOCK, &new_locks.unwrap());

            Ok(())
        });

    tokio::run(install_future);

    Ok(())
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
