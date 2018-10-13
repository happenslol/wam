extern crate select;

use super::{Addon, AddonLock};
use ::std::path::{Path, PathBuf};
use ::std::fs::File;

use self::select::predicate::*;
use self::select::document::Document;

pub fn get_lock(addon: &Addon) -> Option<AddonLock> {
    match addon.provider.as_str() {
        "curse" | "ace" => get_curse_lock(addon),
        "tukui" => get_tuk_lock(addon),
        _ => {
            println!("unknown provider for lock get: {}", addon.provider);
            None
        }
    }
}

fn get_curse_lock(addon: &Addon) -> Option<AddonLock> {
    // by sorting by release type, we get releases before alphas and avoid a problem
    // where the first page could be filled with alpha releases (thanks dbm very cool)
    let files_url = format!("https://wow.curseforge.com/projects/{}/files?sort=releasetype", addon.name);
    let files_page = ::reqwest::get(&files_url).unwrap().text().unwrap();

    let doc = Document::from(files_page.as_str());

    let (version, timestamp) = doc.find(Class("project-file-list-item"))
        .filter(|version_item| {
            // filter for release versions
            version_item.find(
                Class("project-file-release-type").descendant(Class("release-phase"))
            ).next().is_some()
        })
        .map(|version_item| {
            let version_name = version_item.find(
                Class("project-file-name").descendant(Attr("data-action", "file-link"))
            ).next().unwrap().text();

            let uploaded_abbr = version_item.find(
                Class("project-file-date-uploaded").descendant(Name("abbr"))
            ).next().unwrap();

            let uploaded_epoch = uploaded_abbr.attr("data-epoch").unwrap();
            (String::from(version_name), uploaded_epoch.parse::<u64>().unwrap())
        })
        .max_by_key(|item| item.1).unwrap();

    Some(AddonLock {
        // for curse, addon name and resolved are the same since they have
        // proper unique identifiers
        name: format!("{}/{}", addon.provider, addon.name),
        resolved: addon.name.clone(),
        version, timestamp,
    })
}

fn get_tuk_lock(addon: &Addon) -> Option<AddonLock> {
    None
}

pub fn has_update(addon: &Addon, lock: &AddonLock) -> (bool, Option<AddonLock>) {
    match addon.provider.as_str() {
        "curse" | "ace" => check_curse_update(addon, lock),
        "tukui" => check_tuk_update(addon, lock),
        _ => {
            println!("unknown provider for update check: {}", addon.provider);
            (false, None)
        }
    }
}

fn check_curse_update(addon: &Addon, lock: &AddonLock) -> (bool, Option<AddonLock>) {
    let new_lock = get_curse_lock(addon).unwrap();
    if lock.timestamp > lock.timestamp {
        return (true, Some(new_lock));
    }

    (false, None)
}

fn check_tuk_update(addon: &Addon, lock: &AddonLock) -> (bool, Option<AddonLock>) {
    (false, None)
}

fn download_from_url(url: &str, dir: &Path) -> Option<PathBuf> {
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

pub fn download_addon(addon: &Addon, temp_dir: &Path, addon_dir: &Path) {
    let file = match addon.provider.as_str() {
        "curse" | "ace" => {
            let url = format!(
                "https://wow.curseforge.com/projects/{}/files/latest",
                addon.name
            );

            Some(download_from_url(&url, temp_dir).unwrap())
        },
        "tukui" => {
            // check we're getting tukui or elvui, those are "special"
            if addon.name.as_str() == "elvui" || addon.name.as_str() == "tukui" {
                let url = get_tukui_quick_download_link(addon.name.as_str());
                Some(download_from_url(&url, temp_dir).unwrap())
            } else {
                let url = format!("https://www.tukui.org/addons.php?download={}", addon.name);
                let mut res = ::reqwest::get(&url).unwrap();
                let disp_header = String::from(res.headers()["content-disposition"].to_str().unwrap());
                let filename = disp_header.split("filename=").last().unwrap();

                if !filename.ends_with(".zip") {
                    println!("{} not a zip file, skipping", filename);
                    return;
                }

                let path = temp_dir.join(filename);
                let mut addon_file = File::create(&path).expect("could not write file");
                let _ = res.copy_to(&mut addon_file).expect("couldnt not write to file");

                Some(path)
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

    if file.is_none() {
        return;
    }

    super::extract::extract_zip(&file.unwrap(), &addon_dir);
}

fn get_tukui_quick_download_link(addon: &str) -> String {
    let homepage_body = ::reqwest::get("https://www.tukui.org/welcome.php")
        .unwrap().text().unwrap();

    let doc = Document::from(homepage_body.as_str());
    let dl_start = format!("/downloads/{}", addon);

    for link in doc.find(Name("a")) {
        match link.attr("href") {
            Some(href) => {
                if href.starts_with(&dl_start) && href.ends_with(".zip") {
                    println!("{} download link found: {}", addon, href);
                    return format!("https://www.tukui.org{}", href);
                }
            },
            _ => {},
        };
    }

    String::from("")
}
