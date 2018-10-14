use super::select::predicate::*;
use super::select::document::Document;

use ::{Addon, AddonLock};
use ::futures::{Future, Async, Stream};
use ::futures::stream::Concat2;
use ::std::path::{Path, PathBuf};
use ::std::fs::File;
use ::std::io::Write;

use ::reqwest::Error as ReqwestError;
use ::reqwest::async::{Response, Decoder, Client};

pub const CURSE_DL_URL_TEMPLATE: &'static str =
    "https://wow.curseforge.com/projects/{}/files/latest";

pub const ACE_DL_URL_TEMPLATE: &'static str =
    "https://wowace.com/projects/{}/files/latest";

// by sorting by release type, we get releases before alphas and avoid a problem
// where the first page could be filled with alpha releases (thanks dbm very cool)
const CURSE_FILES_URL_TEMPLATE: &'static str =
    "https://wow.curseforge.com/projects/{}/files?sort=releasetype";

const ACE_FILES_URL_TEMPLATE: &'static str =
    "https://wowace.com/projects/{}/files?sort=releasetype";

pub struct CurseDownloadFuture {
    inner: DownloadInner,
    lock: AddonLock,
    filename: Option<String>,
}

pub fn download_addon(addon: Addon, lock: AddonLock) -> CurseDownloadFuture {
    let url = if addon.provider == "curse" {
        CURSE_DL_URL_TEMPLATE.replace("{}", &addon.name)
    } else {
        ACE_DL_URL_TEMPLATE.replace("{}", &addon.name)
    };

    let client = Client::new();
    let pending = client.get(&url).send();
    let inner = DownloadInner::Downloading(Box::new(pending));

    CurseDownloadFuture {
        inner, lock,
        filename: None,
    }
}

enum DownloadInner {
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl Future for CurseDownloadFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::DownloadInner::*;

        loop {
            let next = match self.inner {
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let final_url = String::from(res.url().as_str());
                    let filename = String::from(final_url.split("/").last().unwrap());
                    self.filename = Some(filename);

                    DownloadingBody(res.into_body().concat2())
                },
                DownloadingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let filename = self.filename.take().unwrap();
                    let filepath = Path::new(".wam-temp").join(&filename);
                    let mut file = File::create(&filepath).expect("could not create file");
                    file.write_all(&body).expect("could not write to file");

                    return Ok(Async::Ready((filepath, self.lock.clone())));
                },
            };

            self.inner = next;
        }
    }
}

pub struct CurseLockFuture {
    inner: LockInner,
    addon: Addon,
}

enum LockInner {
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl CurseLockFuture {
    pub fn new(addon: Addon) -> CurseLockFuture {
        let client = Client::new();

        let url = if addon.provider == "curse" {
            CURSE_FILES_URL_TEMPLATE.replace("{}", &addon.name)
        } else {
            ACE_FILES_URL_TEMPLATE.replace("{}", &addon.name)
        };

        let pending = client.get(&url).send();
        let inner = LockInner::Downloading(Box::new(pending));

        CurseLockFuture { inner, addon }
    }
}

impl Future for CurseLockFuture {
    type Item = (Addon, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock)>, String> {
        use self::LockInner::*;

        loop {
            let next = match self.inner {
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    DownloadingBody(res.into_body().concat2())
                },
                DownloadingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let files_page = String::from_utf8(body.to_vec()).unwrap();

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

                    let result = AddonLock {
                        // for curse, addon name and resolved are the same since they have
                        // proper unique identifiers
                        name: format!("{}/{}", self.addon.provider, self.addon.name),
                        resolved: self.addon.name.clone(),
                        version, timestamp,
                    };

                    println!("got curse lock: {:?}", result);
                    return Ok(Async::Ready((self.addon.clone(), result)));
                },
            };

            self.inner = next;
        }
    }
}
