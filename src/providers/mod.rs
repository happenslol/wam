extern crate select;
extern crate chrono;

mod tuk;
mod curse;

use super::{Addon, AddonLock};
use ::std::path::PathBuf;

use ::futures::{Future, Async};

use self::tuk::{TukDownloadFuture, TukLockFuture};
use self::curse::{CurseDownloadFuture, CurseLockFuture};

pub struct AddonLockFuture {
    inner: LockInner,
}

enum LockInner {
    CurseLockFuture(CurseLockFuture),
    TukLockFuture(TukLockFuture),
}

impl Future for AddonLockFuture {
    type Item = (Addon, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(Addon, AddonLock)>, String> {
        use self::LockInner::*;

        match self.inner {
            CurseLockFuture(ref mut f) => f.poll(),
            TukLockFuture(ref mut f) => f.poll(),
        }
    }
}

pub fn get_lock(
    addon: &Addon,
    old_lock: Option<AddonLock>
) -> Option<AddonLockFuture> {
    match addon.provider.as_str() {
        "curse" | "ace" => {
            let inner = LockInner::CurseLockFuture(curse::get_lock(addon.clone()));
            Some(AddonLockFuture { inner })
        },
        "tukui" => {
            let inner = LockInner::TukLockFuture(tuk::get_lock(addon.clone(), old_lock));
            Some(AddonLockFuture { inner })
        },
        _ => None,
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
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::DownloadInner::*;

        match self.inner {
            CurseDownloadFuture(ref mut f) => f.poll(),
            TukDownloadFuture(ref mut f) => f.poll(),
        }
    }
}

pub fn download_addon(
    addon: &Addon, lock: &AddonLock
) -> Option<DownloadAddonFuture> {
    let provider = addon.provider.clone();
    let inner = match provider.as_str() {
        "curse" | "ace" => {
            DownloadInner::CurseDownloadFuture(
                curse::download_addon(addon.clone(), lock.clone())
            )
        },
        "tukui" => {
            DownloadInner::TukDownloadFuture(
                tuk::download_addon(addon.clone(), lock.clone())
            )
        }
        _ => return None,
    };

    Some(DownloadAddonFuture { inner })
}
