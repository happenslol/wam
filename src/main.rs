#[macro_use]
extern crate serde_derive;

extern crate reqwest;
extern crate serde;
extern crate toml;

extern crate futures;
extern crate tokio;
extern crate hyper;
extern crate tokio_tls;

#[macro_use]
extern crate clap;
use clap::{App, AppSettings};

mod extract;
mod providers;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::io::prelude::*;
use std::error::Error;
use std::sync::Arc;

use futures::{Future, Stream};

const TEMP_DIR: &'static str = ".wam-temp";
const ADDONS_DIR: &'static str = "Interface/Addons";

const CONFIG_FILE: &'static str = "wam.toml";
const LOCK_FILE: &'static str = "wam-lock.toml";

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

    // let _ = delete_temp_dir();
}

fn install() -> Result<(), Box<Error>> {
    let mut f = File::open(CONFIG_FILE)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    let parsed: ConfigFile = toml::from_str(&contents)?;

    let lock_path = Path::new(LOCK_FILE);
    let lock = if !lock_path.exists() || !lock_path.is_file() {
        LockFile { addons: Vec::new() }
    } else {
        let mut contents = String::new();
        let mut f = File::open(lock_path)?;
        f.read_to_string(&mut contents)?;
        toml::from_str::<LockFile>(&contents)?
    };

    let addon_dir: &'static Path = &Path::new(ADDONS_DIR);
    if !addon_dir.is_dir() {
        fs::create_dir_all(addon_dir).unwrap()
    }

    let temp_dir = create_temp_dir()?;

    let lock_arc = Arc::new(lock);
    let lock_for_addons = lock_arc.clone();
    let lock_for_file = lock_arc.clone();
    let install_future = futures::stream::iter_ok::<_, ()>(parsed.addons)
        .map(move |addon| {
            println!("processing {:?}", addon);
            // TODO: fuck i feel dirty
            let lock = &lock_for_addons;
            let found = lock.addons.iter().find(|it| {
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
                    &addon_lock,
                    temp_dir.to_path_buf(),
                ),
                _ => panic!("error"),
            }

            // match found {
            //     Some(lock) => {
            //         match providers::has_update(&addon, &lock) {
            //             (true, Some(new_lock)) => {
            //                 println!("got update: {:?}", new_lock);
            //                 providers::download_addon(
            //                     &addon, &new_lock,
            //                     &temp_dir, &addon_dir
            //                 ).map(|_| {
            //                     Some(new_lock)
            //                 })
            //             },
            //             _ => {
            //                 println!("{} was up to date", addon.name);
            //                 None
            //             },
            //         }
            //     },
            //     None => {
            //         match providers::get_lock(&addon, None) {
            //             Some(new_lock) => {
            //                 println!("downloading new: {:?}", new_lock);
            //                 providers::download_addon(
            //                     &addon, &new_lock,
            //                     &temp_dir, &addon_dir
            //                 );
            //                 Some(new_lock)
            //             },
            //             None => {
            //                 println!("no lock found for {}", addon.name);
            //                 None
            //             },
            //         }
            //     }
            // }
        })
        .buffer_unordered(10)
        .map(move |(downloaded, lock)| {
            extract::extract_zip(downloaded, addon_dir.to_path_buf());
            lock
        })
        .collect()
        .map_err(|err| println!("error: {:?}", err))
        .then(move |new_locks| {
            let lock = &lock_for_file;
            let _ = save_lock_file(&lock_path, &lock, &new_locks.unwrap());
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
