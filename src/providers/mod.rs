extern crate select;
extern crate chrono;

mod tuk;
mod curse;

use super::{Addon, AddonLock};
use ::std::path::PathBuf;

use ::futures::{Future, Async};

use self::tuk::TukDownloadFuture;

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
    // CurseDownloadFuture(CurseDownloadFuture),
    TukDownloadFuture(TukDownloadFuture),
}

impl Future for DownloadAddonFuture {
    type Item = (PathBuf, AddonLock);
    type Error = String;

    fn poll(&mut self) -> Result<Async<(PathBuf, AddonLock)>, String> {
        use self::Inner::*;

        match self.inner {
            // CurseDownloadFuture(f) => f.poll(),
            TukDownloadFuture(ref mut f) => f.poll(),
        }
    }
}

pub fn download_addon(
    addon: &Addon, lock: &AddonLock
) -> DownloadAddonFuture {
    let tuk_future = TukDownloadFuture::new(lock.clone(), addon.clone());
    DownloadAddonFuture {
        inner: Inner::TukDownloadFuture(tuk_future),
    }

    // let download_link = match addon.provider.as_str() {
    //     "curse" => curse::CURSE_DL_URL_TEMPLATE.replace("{}", &addon.name),
    //     "ace" => curse::ACE_DL_URL_TEMPLATE.replace("{}", &addon.name),
    //     "tukui" => match addon.name.as_str() {
    //         // check we're getting tukui or elvui, those are "special"
    //         "elvui" | "tukui" => tuk::get_quick_download_link(addon.name.as_str()),
    //         _ => tuk::ADDON_DL_URL_TEMPLATE.replace("{}", &lock.resolved)
    //     },
    //     _ => panic!("unknown provider: {}", addon.provider),
    // };

    // let temp_dir = temp_dir.clone();

    // let lock = lock.clone();
    // CLIENT.get(&download_link).send()
    //     .map_err(|err| println!("error: {}", err))
    //     .and_then(move |res| {
    //         let filename = if let Some(disp_header) = res.headers().get("content-disposition") {
    //             let disp_header = String::from(disp_header.to_str().unwrap());
    //             String::from(disp_header.split("filename=").last().unwrap())
    //         } else {
    //             let final_url = String::from(res.url().as_str());
    //             String::from(final_url.split("/").last().unwrap())
    //         };

    //         let path = temp_dir.join(&filename);
    //         let mut file = File::create(&path).expect("could not create file");
    //         res.into_body()
    //             .map_err(|err| println!("error: {}", err))
    //             .for_each(move |chunk| {
    //                 file
    //                     .write_all(&chunk)
    //                     .map_err(|err| println!("couldnt write: {}", err))
    //             })
    //             .then(|_| Ok((path, lock)))
    //     })

    // match file {
    //     Some(file) => super::extract::extract_zip(&file, &addon_dir),
    //     _ => {},
    // }
}

// fn download_direct(url: &str, dir: &Path) -> Option<PathBuf> {
//     let mut res = ::reqwest::get(url).unwrap();
//     let final_url = String::from(res.url().as_str());
//     let filename = final_url.split("/").last().unwrap();

//     if !filename.ends_with(".zip") {
//         println!("{} not a zip file, skipping", filename);
//         return None;
//     }

//     let path = dir.join(filename);
//     let mut addon_file = File::create(&path).expect("could not write file");
//     let _ = res.copy_to(&mut addon_file).expect("couldnt not write to file");

//     Some(path)
// }

// fn download_attachment(url: &str, dir: &Path) -> Option<PathBuf> {
//     let mut res = ::reqwest::get(url).unwrap();
// }
