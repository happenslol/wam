extern crate select;
extern crate chrono;

mod tuk;
mod curse;

use super::{Addon, AddonLock};
use ::std::path::{Path, PathBuf};
use ::std::fs::File;

pub fn get_lock(addon: &Addon, old_lock: Option<AddonLock>) -> Option<AddonLock> {
    match addon.provider.as_str() {
        "curse" | "ace" => curse::get_lock(addon),
        "tukui" => tuk::get_lock(addon, old_lock),
        _ => {
            println!("unknown provider for lock get: {}", addon.provider);
            None
        }
    }
}

pub fn has_update(addon: &Addon, lock: &AddonLock) -> (bool, Option<AddonLock>) {
    let new_lock = get_lock(addon, Some(lock.clone())).unwrap();
    if new_lock.timestamp > lock.timestamp {
        return (true, Some(new_lock));
    }

    (false, None)
}

pub fn download_addon(addon: &Addon, lock: &AddonLock, temp_dir: &Path, addon_dir: &Path) {
    let file = match addon.provider.as_str() {
        "curse" => {
            let url = format!(
                "https://wow.curseforge.com/projects/{}/files/latest",
                addon.name
            );

            download_direct(&url, temp_dir)
        },
        "ace" => {
            let url = format!(
                "https://wowace.com/projects/{}/files/latest",
                addon.name
            );

            download_direct(&url, temp_dir)
        },
        "tukui" => {
            // check we're getting tukui or elvui, those are "special"
            if addon.name.as_str() == "elvui" || addon.name.as_str() == "tukui" {
                let url = tuk::get_quick_download_link(addon.name.as_str());
                download_direct(&url, temp_dir)
            } else {
                let url = format!("https://www.tukui.org/addons.php?download={}", lock.resolved);
                download_attachment(&url, temp_dir)
            }
        },
        _ => {
            println!(
                "unknown provider for addon {}: {}",
                addon.name,
                addon.provider
            );

            None
        }
    };

    match file {
        Some(file) => super::extract::extract_zip(&file, &addon_dir),
        _ => {},
    }
}

fn download_direct(url: &str, dir: &Path) -> Option<PathBuf> {
    let mut res = ::reqwest::get(url).unwrap();
    let final_url = String::from(res.url().as_str());
    let filename = final_url.split("/").last().unwrap();

    if !filename.ends_with(".zip") {
        println!("{} not a zip file, skipping", filename);
        return None;
    }

    let path = dir.join(filename);
    let mut addon_file = File::create(&path).expect("could not write file");
    let _ = res.copy_to(&mut addon_file).expect("couldnt not write to file");

    Some(path)
}

fn download_attachment(url: &str, dir: &Path) -> Option<PathBuf> {
    let mut res = ::reqwest::get(url).unwrap();
    let disp_header = String::from(res.headers()["content-disposition"].to_str().unwrap());
    let filename = disp_header.split("filename=").last().unwrap();

    if !filename.ends_with(".zip") {
        println!("{} not a zip file, skipping", filename);
        return None;
    }

    let path = dir.join(filename);
    let mut addon_file = File::create(&path).expect("could not write file");
    let _ = res.copy_to(&mut addon_file).expect("couldnt not write to file");

    Some(path)
}
