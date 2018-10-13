#[macro_use]
extern crate serde_derive;

extern crate reqwest;
extern crate serde;
extern crate toml;

mod extract;
mod providers;

use std::fs::{self, File};
use std::path::Path;
use std::io::prelude::*;

#[derive(Serialize, Deserialize, Debug)]
struct ConfigFile {
    pub addons: Vec<Addon>,
}

#[derive(Serialize, Deserialize, Debug)]
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
    // do i even need this? timestamp is always better for
    // comparing
    pub version: String,
    pub timestamp: u64,
}

fn main() {
    let mut f = File::open("wam.toml").expect("file not found");

    let lock_path = Path::new("wam-lock.toml");
    let lock = if !lock_path.exists() || !lock_path.is_file() {
        LockFile { addons: Vec::new() }
    } else {
        let mut contents = String::new();
        let mut f = File::open(lock_path).expect("lock file could not be opened");
        f.read_to_string(&mut contents).expect("something went wrong reading the file");

        let parsed: LockFile =
            toml::from_str(&contents).expect("something went wrong parsing");

        parsed
    };

    let temp_dir = Path::new(".updater-temp");
    if temp_dir.exists() && temp_dir.is_dir() {
        fs::remove_dir_all(temp_dir).expect("could not remove existing temp dir");
    }

    fs::create_dir(temp_dir).expect("could not create temp dir");

    let addon_dir = Path::new("Interface/AddOns");

    let mut contents = String::new();
    f.read_to_string(&mut contents).expect("something went wrong reading the file");

    let parsed: ConfigFile =
        toml::from_str(&contents).expect("something went wrong parsing");

    let mut new_locks = Vec::new();
    for addon in parsed.addons.iter() {
        println!("getting {} from {}", addon.name, addon.provider);
        let found = lock.addons.iter().find(|it| {
            // addons are unique over their name and provider
            format!("{}/{}", addon.provider, addon.name) == it.name
        });

        match found {
            Some(lock) => {
                match providers::has_update(&addon, &lock) {
                    (true, Some(new_lock)) => {
                        println!("got new lock for {}: {:?}", addon.name, new_lock);
                    },
                    _ => {
                        println!("{} was up to date", addon.name);
                        continue;
                    },
                }
            },
            None => {
                match providers::get_lock(addon) {
                    Some(new_lock) => {
                        providers::download_addon(addon, &temp_dir, &addon_dir);
                        println!("got new lock for {}: {:?}", addon.name, new_lock);
                        new_locks.push(new_lock);
                    },
                    None => {
                        println!("no lock found for {}", addon.name);
                        continue;
                    },
                }
            }
        }
    }

    fs::remove_dir_all(temp_dir).expect("could not remove temp dir");
    save_lock_file(&lock_path, lock, new_locks);
}

fn save_lock_file(path: &Path, old_lock: LockFile, new_locks: Vec<AddonLock>) {
    let mut locks = old_lock.clone();
    for lock in new_locks {
        let existing = old_lock.addons.iter().enumerate().find(|(_, it)| {
            it.name == lock.name
        });

        if existing.is_some() {
            let (i, _) = existing.unwrap();
            locks.addons[i] = lock.clone();
        } else {
            locks.addons.push(lock.clone());
        }
    }

    let lock_str = toml::to_string(&locks).unwrap();
    
    // recreate the file because we want to overwrite anyways
    let mut f = File::create(path).unwrap();
    f.write_all(lock_str.as_bytes()).unwrap();
}
