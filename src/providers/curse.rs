use super::select::predicate::*;
use super::select::document::Document;

use ::{Addon, AddonLock};

pub const CURSE_DL_URL_TEMPLATE: &'static str =
    "https://wow.curseforge.com/projects/{}/files/latest";

pub const ACE_DL_URL_TEMPLATE: &'static str =
    "https://wowace.com/projects/{}/files/latest";

const CURSE_FILES_URL_TEMPLATE: &'static str =
    "https://wow.curseforge.com/projects/{}/files?sort=releasetype";

const ACE_FILES_URL_TEMPLATE: &'static str =
    "https://wowace.com/projects/{}/files?sort=releasetype";

pub fn get_lock(addon: &Addon) -> Option<AddonLock> {
    // by sorting by release type, we get releases before alphas and avoid a problem
    // where the first page could be filled with alpha releases (thanks dbm very cool)
    let files_url = if addon.provider == "curse" {
        CURSE_FILES_URL_TEMPLATE.replace("{}", &addon.name)
    } else {
        ACE_FILES_URL_TEMPLATE.replace("{}", &addon.name)
    };

    let files_page = ::reqwest::get(&files_url).unwrap().text().unwrap();

    let doc = Document::from(files_page.as_str());

    let (version, timestamp) = doc.find(Class("project-file-list-item"))
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