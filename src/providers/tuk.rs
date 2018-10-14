use super::select::predicate::*;
use super::select::document::Document;
use super::chrono::prelude::*;

use ::{Addon, AddonLock};
use ::futures::{Future, Async, Stream};
use ::futures::stream::Concat2;
use ::std::path::{Path, PathBuf};
use ::std::fs::File;
use ::std::io::Write;

use ::reqwest::Error as ReqwestError;
use ::reqwest::async::{Response, Decoder, Client};

pub const ADDON_DL_URL_TEMPLATE: &'static str =
    "https://www.tukui.org/addons.php?download={}";

const UI_DL_URL_TEMPLATE: &'static str =
    "https://www.tukui.org/download.php?ui={}";

const SEARCH_URL_TEMPLATE: &'static str =
    "https://www.tukui.org/addons.php?search={}";

const ADDON_URL_TEMPLATE: &'static str =
    "https://www.tukui.org/addons.php?id={}";

const HOME_URL: &'static str = "https://www.tukui.org/welcome.php";
const BASE_URL_TEMPLATE: &'static str = "https://www.tukui.org{}";

pub struct TukDownloadFuture {
    inner: DownloadInner,
}

pub fn download_addon(addon: Addon, lock: AddonLock) -> TukDownloadFuture {
    let name = addon.name.clone();
    let client = Client::new();

    let inner = match name.as_str() {
        "tukui" | "elvui" => {
            let pending = client.get(HOME_URL).send();
            let inner = HomeDownloadInner::GettingDownloadLink(Box::new(pending));

            DownloadInner::HomeDownloadFuture(HomeDownloadFuture {
                inner, lock, addon, client,
                filename: None,
            })
        },
        _ => {
            let url = ADDON_DL_URL_TEMPLATE.replace("{}", &lock.resolved);
            let pending = client.get(&url).send();
            let inner = AddonDownloadInner::Downloading(Box::new(pending));

            DownloadInner::AddonDownloadFuture(AddonDownloadFuture {
                inner, lock,
                filename: None,
            })
        },
    };

    TukDownloadFuture { inner }
}

impl Future for TukDownloadFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::DownloadInner::*;

        match self.inner {
            HomeDownloadFuture(ref mut f) => f.poll(),
            AddonDownloadFuture(ref mut f) => f.poll(),
        }
    }
}

enum DownloadInner {
    HomeDownloadFuture(HomeDownloadFuture),
    AddonDownloadFuture(AddonDownloadFuture),
}

struct HomeDownloadFuture {
    inner: HomeDownloadInner,
    client: Client,
    lock: AddonLock,
    filename: Option<String>,
    addon: Addon,
}

enum HomeDownloadInner {
    GettingDownloadLink(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    GettingDownloadLinkBody(Concat2<Decoder>),
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl Future for HomeDownloadFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::HomeDownloadInner::*;

        loop {
            let next = match self.inner {
                GettingDownloadLink(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    GettingDownloadLinkBody(res.into_body().concat2())
                },
                GettingDownloadLinkBody(ref mut f) => {
                    let homepage = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let homepage = String::from_utf8(homepage.to_vec()).unwrap();

                    let doc = Document::from(homepage.as_str());
                    let dl_start = format!("/downloads/{}", self.addon.name);

                    let mut url = None;
                    for link in doc.find(Name("a")) {
                        match link.attr("href") {
                            Some(href) => {
                                if href.starts_with(&dl_start) && href.ends_with(".zip") {
                                    let filename = href.split("/").last().unwrap();
                                    self.filename = Some(String::from(filename));
                                    url = Some(BASE_URL_TEMPLATE.replace("{}", &href));
                                }
                            },
                            _ => {},
                        };
                    }

                    let pending = self.client.get(&url.unwrap()).send();
                    Downloading(Box::new(pending))
                },
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    DownloadingBody(res.into_body().concat2())
                },
                DownloadingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let filename = self.filename.take().unwrap();
                    let filepath = Path::new(&".wam-temp").join(&filename);
                    let mut file = File::create(&filepath).expect("could not create file");
                    file.write_all(&body).expect("could not write to file");

                    return Ok(Async::Ready((filepath, self.lock.clone())));
                },
            };

            self.inner = next;
        }
    }
}

struct AddonDownloadFuture {
    inner: AddonDownloadInner,
    lock: AddonLock,
    filename: Option<String>,
}

enum AddonDownloadInner {
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl Future for AddonDownloadFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::AddonDownloadInner::*;

        loop {
            let next = match self.inner {
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let filename = {
                        let header = res.headers()["content-disposition"].to_str().unwrap();
                        let filename = header.split("filename=").last().unwrap();
                        String::from(filename)
                    };

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

pub struct TukLockFuture {
    inner: LockInner,
}

pub fn get_lock(addon: Addon, old_lock: Option<AddonLock>) -> TukLockFuture {
    let name = addon.name.clone();
    let client = Client::new();

    let inner = match name.as_str() {
        "tukui" | "elvui" => {
            let url = UI_DL_URL_TEMPLATE.replace("{}", &addon.name);
            let pending = client.get(&url).send();
            let inner = HomeLockInner::Downloading(Box::new(pending));

            LockInner::HomeLockFuture(HomeLockFuture {
                inner, addon,
            })
        },
        _ => {
            if let Some(old_lock) = old_lock {
                let resolved = old_lock.resolved.clone();
                let url = ADDON_DL_URL_TEMPLATE.replace("{}", &addon.name);
                let pending = client.get(&url).send();
                let inner = AddonLockInner::Downloading(Box::new(pending));

                LockInner::AddonLockFuture(AddonLockFuture {
                    inner, client, addon,
                    resolved: Some(resolved),
                })
            } else {
                let url = SEARCH_URL_TEMPLATE.replace("{}", &addon.name);
                let pending = client.get(&url).send();
                let inner = AddonLockInner::Resolving(Box::new(pending));

                LockInner::AddonLockFuture(AddonLockFuture {
                    inner, client, addon,
                    resolved: None,
                })
            }
        },
    };

    TukLockFuture { inner }
}

impl Future for TukLockFuture {
    type Item = (Addon, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock)>, String> {
        use self::LockInner::*;

        match self.inner {
            HomeLockFuture(ref mut f) => f.poll(),
            AddonLockFuture(ref mut f) => f.poll(),
        }
    }
}

enum LockInner {
    HomeLockFuture(HomeLockFuture),
    AddonLockFuture(AddonLockFuture),
}

struct HomeLockFuture {
    inner: HomeLockInner,
    addon: Addon,
}

enum HomeLockInner {
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl Future for HomeLockFuture {
    type Item = (Addon, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock)>, String> {
        use self::HomeLockInner::*;

        loop {
            let next = match self.inner {
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    DownloadingBody(res.into_body().concat2())
                },
                DownloadingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let page = String::from_utf8(body.to_vec()).unwrap();
                    let doc = Document::from(page.as_str());

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

                    let result = AddonLock {
                        name: format!("tukui/{}", self.addon.name),
                        resolved: self.addon.name.clone(),
                        version, timestamp,
                    };

                    return Ok(Async::Ready((self.addon.clone(), result)));
                },
            };

            self.inner = next;
        }
    }
}

struct AddonLockFuture {
    inner: AddonLockInner,
    addon: Addon,
    client: Client,
    resolved: Option<String>,
}

enum AddonLockInner {
    Resolving(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    ResolvingBody(Concat2<Decoder>),
    Downloading(Box<Future<Item = Response, Error = ReqwestError> + Send>),
    DownloadingBody(Concat2<Decoder>),
}

impl Future for AddonLockFuture {
    type Item = (Addon, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock)>, String> {
        use self::AddonLockInner::*;

        loop {
            let next = match self.inner {
                Resolving(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    ResolvingBody(res.into_body().concat2())
                },
                ResolvingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let page = String::from_utf8(body.to_vec()).unwrap();

                    // TODO: lowercase this all
                    // let search_term = addon.name.replace(" ", "+");
                    // let search_url = SEARCH_URL_TEMPLATE.replace("{}", &search_term);
                    // let search_page = ::reqwest::get(&search_url).unwrap().text().unwrap();

                    let doc = Document::from(page.as_str());
                    let result_node = doc.find(
                        Class("addons")
                            .and(Class("addons-list"))
                            .descendant(Name("a"))
                    ).next().unwrap();

                    let href = result_node.attr("href").unwrap();
                    let resolved = String::from(href.split("?id=").last().unwrap());

                    let addon_url = ADDON_URL_TEMPLATE.replace("{}", &resolved);
                    let pending = self.client.get(&addon_url).send();
                    self.resolved = Some(resolved);

                    Downloading(Box::new(pending))
                },
                Downloading(ref mut f) => {
                    let res = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    DownloadingBody(res.into_body().concat2())
                },
                DownloadingBody(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let page = String::from_utf8(body.to_vec()).unwrap();
                    let doc = Document::from(page.as_str());

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

                    let result = AddonLock {
                        name: format!("tukui/{}", self.addon.name),
                        resolved: self.resolved.take().unwrap(),
                        version, timestamp,
                    };

                    return Ok(Async::Ready((self.addon.clone(), result)));
                },
            };

            self.inner = next;
        }
    }
}
