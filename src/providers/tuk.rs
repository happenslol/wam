use super::select::predicate::*;
use super::select::document::Document;
use super::chrono::prelude::*;

use ::{Addon, AddonLock};

pub fn get_lock(addon: &Addon, old_lock: Option<AddonLock>) -> Option<AddonLock> {
    if addon.name.as_str() == "elvui" || addon.name.as_str() == "tukui" {
        let url = format!("https://www.tukui.org/download.php?ui={}", addon.name);
        let ui_page = ::reqwest::get(&url).unwrap().text().unwrap();
        let doc = Document::from(ui_page.as_str());

        let mut version_els = doc.find(
            Attr("id", "version").descendant(
                Name("b").and(Class("Premium"))
            )
        );

        let version = version_els.next().unwrap().text();
        let date = version_els.next().unwrap().text();
        let date = format!("{} 00:00:00", date);

        let parsed_date = Utc.datetime_from_str(&date, "%Y-%m-%d %H:%M:%S").unwrap();
        let timestamp = parsed_date.timestamp() as u64;

        Some(AddonLock {
            name: format!("tukui/{}", addon.name),
            resolved: addon.name.clone(),
            version, timestamp,
        })
    } else {
        let resolved_id = match old_lock {
            Some(old) => old.resolved,
            None => {
                // TODO: lowercase this all
                let search_term = addon.name.replace(" ", "+");
                let search_url = format!("https://www.tukui.org/addons.php?search={}", search_term);
                let search_page = ::reqwest::get(&search_url).unwrap().text().unwrap();

                let doc = Document::from(search_page.as_str());
                let result_node = doc.find(
                    Class("addons")
                        .and(Class("addons-list"))
                        .descendant(Name("a"))
                ).next().unwrap();

                let href = result_node.attr("href").unwrap();
                String::from(href.split("?id=").last().unwrap())
            }
        };

        let version_url = format!("https://www.tukui.org/addons.php?id={}", resolved_id);
        let version_page = ::reqwest::get(&version_url).unwrap().text().unwrap();
        let doc = Document::from(version_page.as_str());

        let mut version_els = doc.find(
            Attr("id", "extras").descendant(
                Name("b").and(Class("VIP"))
            )
        );

        // TODO: why is version not there wtf
        let _version = version_els.next().unwrap().text();
        let date = version_els.next().unwrap().text();
        let time = version_els.next().unwrap().text();

        let version = String::from("TODO");
        let date_str = format!("{} {}:00", date, time);

        let parsed_date = Utc.datetime_from_str(&date_str, "%b %e, %Y %H:%M:%S").unwrap();
        let timestamp = parsed_date.timestamp() as u64;

        Some(AddonLock {
            name: format!("tukui/{}", addon.name),
            resolved: resolved_id,
            version, timestamp,
        })
    }
}

pub fn get_quick_download_link(addon: &str) -> String {
    let homepage_body = ::reqwest::get("https://www.tukui.org/welcome.php")
        .unwrap().text().unwrap();

    let doc = Document::from(homepage_body.as_str());
    let dl_start = format!("/downloads/{}", addon);

    for link in doc.find(Name("a")) {
        match link.attr("href") {
            Some(href) => {
                if href.starts_with(&dl_start) && href.ends_with(".zip") {
                    return format!("https://www.tukui.org{}", href);
                }
            },
            _ => {},
        };
    }

    String::from("")
}
