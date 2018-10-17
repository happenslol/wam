extern crate chrono;
extern crate select;

mod curse;
mod tuk;

use std::path::PathBuf;
use {Addon, AddonLock};

use futures::{Async, Future};
use std::sync::mpsc::Sender;

use self::curse::{CurseDownloadFuture, CurseLockFuture};
use self::tuk::{TukDownloadFuture, TukLockFuture};

use ProgressUpdate;

pub struct AddonLockFuture {
    inner: LockInner,
}

enum LockInner {
    CurseLockFuture(CurseLockFuture),
    TukLockFuture(TukLockFuture),
}

impl Future for AddonLockFuture {
    type Item = (Addon, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::LockInner::*;

        match self.inner {
            CurseLockFuture(ref mut f) => f.poll(),
            TukLockFuture(ref mut f) => f.poll(),
        }
    }
}

pub fn get_lock(
    all: (Addon, Option<AddonLock>, Sender<ProgressUpdate>),
) -> Option<AddonLockFuture> {
    let (addon, old_lock, tx) = all;
    match addon.provider.as_str() {
        "curse" | "ace" => {
            let inner = LockInner::CurseLockFuture(curse::get_lock(addon, tx));
            Some(AddonLockFuture { inner })
        }
        "tukui" => {
            let inner = LockInner::TukLockFuture(tuk::get_lock(addon, old_lock, tx));
            Some(AddonLockFuture { inner })
        }
        _ => {
            println!(
                "skipping unkown provider: {}/{}",
                addon.name, addon.provider
            );
            None
        }
    }
}

pub struct DownloadAddonFuture {
    inner: DownloadInner,
}

enum DownloadInner {
    CurseDownloadFuture(CurseDownloadFuture),
    TukDownloadFuture(TukDownloadFuture),
}

impl Future for DownloadAddonFuture {
    type Item = (PathBuf, AddonLock, Sender<ProgressUpdate>);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock, Sender<ProgressUpdate>)>, String> {
        use self::DownloadInner::*;

        match self.inner {
            CurseDownloadFuture(ref mut f) => f.poll(),
            TukDownloadFuture(ref mut f) => f.poll(),
        }
    }
}

pub fn download_addon(
    all: (Addon, AddonLock, Sender<ProgressUpdate>),
) -> Option<DownloadAddonFuture> {
    let (addon, lock, tx) = all;
    let provider = addon.provider.clone();
    let inner = match provider.as_str() {
        "curse" | "ace" => {
            DownloadInner::CurseDownloadFuture(curse::download_addon(addon, lock, tx))
        }
        "tukui" => DownloadInner::TukDownloadFuture(tuk::download_addon(addon, lock, tx)),
        _ => return None,
    };

    Some(DownloadAddonFuture { inner })
}
