//! Build script: derives the Windows exe icon (.ico) and a small runtime icon
//! from `assets/icon.png`, so only a few KB are embedded instead of the full image.

use image::imageops::FilterType;
use image::{ExtendedColorType, ImageFormat, RgbaImage, codecs::ico::{IcoEncoder, IcoFrame}};
use std::path::PathBuf;

/// The artwork sits on a black background; make the black that touches the edge transparent so the
/// rounded corners are clean.
fn clear_outer_black(img: &mut RgbaImage) {
    let (w, h) = img.dimensions();
    let dark = |p: &image::Rgba<u8>| p[0] < 28 && p[1] < 28 && p[2] < 28;
    let mut seen = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if seen[i] || !dark(img.get_pixel(x, y)) {
            continue;
        }
        seen[i] = true;
        img.get_pixel_mut(x, y)[3] = 0;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut src = image::open("assets/icon.png").expect("assets/icon.png").to_rgba8();
    clear_outer_black(&mut src);

    // Runtime icon (window / title bar).
    let rt = image::imageops::resize(&src, 256, 256, FilterType::Lanczos3);
    rt.save_with_format(out.join("icon256.png"), ImageFormat::Png).unwrap();

    // Multi-size .ico for the executable.
    let mut frames = Vec::new();
    for size in [256u32, 64, 48, 32, 24, 16] {
        let img = image::imageops::resize(&src, size, size, FilterType::Lanczos3);
        frames.push(IcoFrame::as_png(img.as_raw(), size, size, ExtendedColorType::Rgba8).unwrap());
    }
    let ico_path = out.join("app.ico");
    let f = std::fs::File::create(&ico_path).unwrap();
    IcoEncoder::new(f).encode_images(&frames).unwrap();

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().unwrap());
        res.set("ProductName", "Rotor Controller");
        res.set("FileDescription", "Rotor Controller");
        res.compile().expect("compile windows resources");
    }
}
