use super::chrono::prelude::*;
use super::select::document::Document;
use super::select::predicate::*;

use futures::{Async, Future, Stream};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use {Addon, AddonLock};

use reqwest::async::{Chunk, Client, Response};

use std::sync::mpsc::Sender;
use ProgressUpdate;

pub const ADDON_DL_URL_TEMPLATE: &'static str = "https://www.tukui.org/addons.php?download={}";

const UI_DL_URL_TEMPLATE: &'static str = "https://www.tukui.org/download.php?ui={}";

const SEARCH_URL_TEMPLATE: &'static str = "https://www.tukui.org/addons.php?search={}";

const ADDON_URL_TEMPLATE: &'static str = "https://www.tukui.org/addons.php?id={}";

const HOME_URL: &'static str = "https://www.tukui.org/welcome.php";
const BASE_URL_TEMPLATE: &'static str = "https://www.tukui.org{}";

pub struct TukDownloadFuture {
    inner: DownloadInner,
}

pub fn download_addon(
    addon: Addon,
    lock: AddonLock,
    tx: Sender<ProgressUpdate>,
) -> TukDownloadFuture {
    let name = addon.name.clone();
    let client = Client::new();

    let inner = match name.as_str() {
        "tukui" | "elvui" => DownloadInner::HomeDownloadFuture(HomeDownloadFuture {
            lock,
            addon,
            client,
            inner: HomeDownloadInner::Idle,
            filename: None,
            progress_tx: tx,
        }),
        _ => DownloadInner::AddonDownloadFuture(AddonDownloadFuture {
            lock,
            addon,
            client,
            inner: AddonDownloadInner::Idle,
            filename: None,
            progress_tx: tx,
        }),
    };

    TukDownloadFuture { inner }
}

impl Future for TukDownloadFuture {
    type Item = (PathBuf, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock, Sender<ProgressUpdate>)>, String> {
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
    progress_tx: Sender<ProgressUpdate>,
}

enum HomeDownloadInner {
    Idle,
    GettingDownloadLink(Box<Future<Item = Chunk, Error = String> + Send>),
    Downloading(Box<Future<Item = Chunk, Error = String> + Send>),
}

impl Future for HomeDownloadFuture {
    type Item = (PathBuf, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::HomeDownloadInner::*;

        loop {
            let next = match self.inner {
                Idle => {
                    let pending = self
                        .client
                        .get(HOME_URL)
                        .send()
                        .and_then(|res| res.into_body().concat2())
                        .map_err(|err| format!("{}", err));

                    self.progress_tx
                        .send(ProgressUpdate::Started {
                            subject: self.addon.name.clone(),
                            step: "getting download link",
                        })
                        .unwrap();

                    GettingDownloadLink(Box::new(pending))
                }
                GettingDownloadLink(ref mut f) => {
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
                            }
                            _ => {}
                        };
                    }

                    let pending = self
                        .client
                        .get(&url.unwrap())
                        .send()
                        .and_then(|res| res.into_body().concat2())
                        .map_err(|err| format!("{}", err));

                    self.progress_tx
                        .send(ProgressUpdate::Progressed {
                            subject: self.addon.name.clone(),
                            step: "downloading",
                        })
                        .unwrap();

                    Downloading(Box::new(pending))
                }
                Downloading(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let filename = self.filename.take().unwrap();
                    let filepath = Path::new(&".wam-temp").join(&filename);
                    let mut file = File::create(&filepath).expect("could not create file");
                    file.write_all(&body).expect("could not write to file");

                    self.progress_tx
                        .send(ProgressUpdate::Done {
                            subject: self.addon.name.clone(),
                        })
                        .unwrap();

                    return Ok(Async::Ready((
                        filepath,
                        self.lock.clone(),
                        self.progress_tx.clone(),
                    )));
                }
            };

            self.inner = next;
        }
    }
}

struct AddonDownloadFuture {
    inner: AddonDownloadInner,
    client: Client,
    addon: Addon,
    lock: AddonLock,
    filename: Option<String>,
    progress_tx: Sender<ProgressUpdate>,
}

enum AddonDownloadInner {
    Idle,
    ReadingFilename(Box<Future<Item = Response, Error = String> + Send>),
    Downloading(Box<Future<Item = Chunk, Error = String> + Send>),
}

impl Future for AddonDownloadFuture {
    type Item = (PathBuf, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::AddonDownloadInner::*;

        loop {
            let next = match self.inner {
                Idle => {
                    let url = ADDON_DL_URL_TEMPLATE.replace("{}", &self.lock.resolved);
                    let pending = self
                        .client
                        .get(&url)
                        .send()
                        .map_err(|err| format!("{}", err));

                    self.progress_tx
                        .send(ProgressUpdate::Started {
                            subject: self.addon.name.clone(),
                            step: "reading filename",
                        })
                        .unwrap();

                    ReadingFilename(Box::new(pending))
                }
                ReadingFilename(ref mut f) => {
                    let res = try_ready!(f.poll());
                    let filename = {
                        let header = res.headers()["content-disposition"].to_str().unwrap();
                        let filename = header.split("filename=").last().unwrap();
                        String::from(filename)
                    };

                    self.filename = Some(filename);
                    let body = res.into_body().concat2().map_err(|err| format!("{}", err));

                    self.progress_tx
                        .send(ProgressUpdate::Progressed {
                            subject: self.addon.name.clone(),
                            step: "downloading",
                        })
                        .unwrap();

                    Downloading(Box::new(body))
                }
                Downloading(ref mut f) => {
                    let body = try_ready!(f.poll());
                    let filename = self.filename.take().unwrap();
                    let filepath = Path::new(".wam-temp").join(&filename);
                    let mut file = File::create(&filepath).expect("could not create file");
                    file.write_all(&body).expect("could not write to file");

                    self.progress_tx
                        .send(ProgressUpdate::Done {
                            subject: self.addon.name.clone(),
                        })
                        .unwrap();

                    return Ok(Async::Ready((
                        filepath,
                        self.lock.clone(),
                        self.progress_tx.clone(),
                    )));
                }
            };

            self.inner = next;
        }
    }
}

pub struct TukLockFuture {
    inner: LockInner,
}

pub fn get_lock(
    addon: Addon,
    old_lock: Option<AddonLock>,
    tx: Sender<ProgressUpdate>,
) -> TukLockFuture {
    let name = addon.name.clone();
    let client = Client::new();

    let inner = match name.as_str() {
        "tukui" | "elvui" => LockInner::HomeLockFuture(HomeLockFuture {
            inner: HomeLockInner::Idle,
            client,
            addon,
            progress_tx: tx,
        }),
        _ => LockInner::AddonLockFuture(AddonLockFuture {
            inner: AddonLockInner::Idle,
            resolved: old_lock.and_then(|it| Some(it.resolved)),
            client,
            addon,
            progress_tx: tx,
        }),
    };

    TukLockFuture { inner }
}

impl Future for TukLockFuture {
    type Item = (Addon, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock, Sender<ProgressUpdate>)>, String> {
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
    client: Client,
    addon: Addon,
    progress_tx: Sender<ProgressUpdate>,
}

enum HomeLockInner {
    Idle,
    Downloading(Box<Future<Item = Chunk, Error = String> + Send>),
}

impl Future for HomeLockFuture {
    type Item = (Addon, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::HomeLockInner::*;

        loop {
            let next = match self.inner {
                Idle => {
                    let url = UI_DL_URL_TEMPLATE.replace("{}", &self.addon.name);
                    let pending = self
                        .client
                        .get(&url)
                        .send()
                        .and_then(|res| res.into_body().concat2())
                        .map_err(|err| format!("{}", err));

                    self.progress_tx
                        .send(ProgressUpdate::Started {
                            subject: self.addon.name.clone(),
                            step: "downloading",
                        })
                        .unwrap();

                    Downloading(Box::new(pending))
                }
                Downloading(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let page = String::from_utf8(body.to_vec()).unwrap();
                    let doc = Document::from(page.as_str());

                    let mut version_els =
                        doc.find(Attr("id", "version").descendant(Name("b").and(Class("Premium"))));

                    let version = version_els.next().unwrap().text();
                    let date = version_els.next().unwrap().text();
                    let date = format!("{} 00:00:00", date);

                    let parsed_date = Utc.datetime_from_str(&date, "%Y-%m-%d %H:%M:%S").unwrap();
                    let timestamp = parsed_date.timestamp() as u64;

                    let result = AddonLock {
                        name: format!("tukui/{}", self.addon.name),
                        resolved: self.addon.name.clone(),
                        version,
                        timestamp,
                    };

                    self.progress_tx
                        .send(ProgressUpdate::Done {
                            subject: self.addon.name.clone(),
                        })
                        .unwrap();

                    return Ok(Async::Ready((
                        self.addon.clone(),
                        result,
                        self.progress_tx.clone(),
                    )));
                }
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
    progress_tx: Sender<ProgressUpdate>,
}

enum AddonLockInner {
    Idle,
    Resolving(Box<Future<Item = Chunk, Error = String> + Send>),
    Downloading(Box<Future<Item = Chunk, Error = String> + Send>),
}

impl Future for AddonLockFuture {
    type Item = (Addon, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::AddonLockInner::*;

        loop {
            let next = match self.inner {
                Idle => {
                    if self.resolved.is_some() {
                        let url = ADDON_DL_URL_TEMPLATE.replace("{}", &self.addon.name);
                        let pending = self
                            .client
                            .get(&url)
                            .send()
                            .and_then(|res| res.into_body().concat2())
                            .map_err(|err| format!("{}", err));

                        self.progress_tx
                            .send(ProgressUpdate::Started {
                                subject: self.addon.name.clone(),
                                step: "downloading",
                            })
                            .unwrap();

                        Downloading(Box::new(pending))
                    } else {
                        let search_term = self.addon.name.replace(" ", "+").to_lowercase();

                        let url = SEARCH_URL_TEMPLATE.replace("{}", &search_term);
                        let pending = self
                            .client
                            .get(&url)
                            .send()
                            .and_then(|res| res.into_body().concat2())
                            .map_err(|err| format!("{}", err));

                        self.progress_tx
                            .send(ProgressUpdate::Started {
                                subject: self.addon.name.clone(),
                                step: "resolving",
                            })
                            .unwrap();

                        Resolving(Box::new(pending))
                    }
                }
                Resolving(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());
                    let page = String::from_utf8(body.to_vec()).unwrap();

                    let doc = Document::from(page.as_str());
                    let result_node =
                        doc.find(
                            Class("addons")
                                .and(Class("addons-list"))
                                .descendant(Name("a")),
                        ).next()
                            .unwrap();

                    let href = result_node.attr("href").unwrap();
                    let resolved = String::from(href.split("?id=").last().unwrap());

                    let addon_url = ADDON_URL_TEMPLATE.replace("{}", &resolved);
                    let pending = self
                        .client
                        .get(&addon_url)
                        .send()
                        .and_then(|res| res.into_body().concat2())
                        .map_err(|err| format!("{}", err));

                    self.resolved = Some(resolved);

                    self.progress_tx
                        .send(ProgressUpdate::Progressed {
                            subject: self.addon.name.clone(),
                            step: "downloading",
                        })
                        .unwrap();

                    Downloading(Box::new(pending))
                }
                Downloading(ref mut f) => {
                    let body = try_ready!(f.map_err(|err| format!("{}", err)).poll());

                    self.progress_tx
                        .send(ProgressUpdate::Progressed {
                            subject: self.addon.name.clone(),
                            step: "parsing metadata",
                        })
                        .unwrap();

                    let page = String::from_utf8(body.to_vec()).unwrap();
                    let doc = Document::from(page.as_str());

                    let mut version_els =
                        doc.find(Attr("id", "extras").descendant(Name("b").and(Class("VIP"))));

                    // TODO: why is version not there wtf
                    let version = version_els.next().unwrap().text();
                    let date = version_els.next().unwrap().text();
                    let time = version_els.next().unwrap().text();

                    let date_str = format!("{} {}:00", date, time);

                    let parsed_date = Utc
                        .datetime_from_str(&date_str, "%b %e, %Y %H:%M:%S")
                        .unwrap();
                    let timestamp = parsed_date.timestamp() as u64;

                    let result = AddonLock {
                        name: format!("tukui/{}", self.addon.name),
                        resolved: self.resolved.take().unwrap(),
                        version,
                        timestamp,
                    };

                    self.progress_tx
                        .send(ProgressUpdate::Done {
                            subject: self.addon.name.clone(),
                        })
                        .unwrap();

                    return Ok(Async::Ready((
                        self.addon.clone(),
                        result,
                        self.progress_tx.clone(),
                    )));
                }
            };

            self.inner = next;
        }
    }
}
