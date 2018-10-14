extern crate select;
extern crate chrono;

mod tuk;
mod curse;

use super::{Addon, AddonLock};
use ::std::path::PathBuf;

use ::futures::{Future, Async};

use self::tuk::TukDownloadFuture;
use self::curse::CurseDownloadFuture;

pub fn get_lock(
    addon: &Addon,
    old_lock: Option<AddonLock>
) -> Option<AddonLock> {
    match addon.provider.as_str() {
        "curse" | "ace" => curse::get_lock(addon),
        "tukui" => tuk::get_lock(addon, old_lock),
        _ => {
            println!("unknown provider for lock get: {}", addon.provider);
            None
        }
    }
}

pub struct DownloadAddonFuture {
    inner: Inner,
}

enum Inner {
    CurseDownloadFuture(CurseDownloadFuture),
    TukDownloadFuture(TukDownloadFuture),
}

impl Future for DownloadAddonFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::Inner::*;

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
            Inner::CurseDownloadFuture(CurseDownloadFuture::new(
                lock.clone(), addon.clone()
            ))
        },
        "tukui" => {
            Inner::TukDownloadFuture(TukDownloadFuture::new(
                lock.clone(), addon.clone()
            ))
        }
        _ => return None,
    };

    Some(DownloadAddonFuture { inner })
}
