use super::{admission, aperture};

pub enum DesktopAperture {
    Fits,
    Short { free_mib: f64, need_mib: f64 },
    TimedOut,
}

pub fn desktop_aperture(host_margin_mib: Option<u32>) -> DesktopAperture {
    let need_mib = admission::aperture_need_mib(1, 0, host_margin_mib);
    match aperture::bar1() {
        aperture::Bar1::Free(free_mib) if free_mib < need_mib => DesktopAperture::Short { free_mib, need_mib },
        aperture::Bar1::TimedOut => DesktopAperture::TimedOut,
        _ => DesktopAperture::Fits,
    }
}
