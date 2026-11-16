//! Writes the bundled studio panorama in the other formats environments read, for the
//! examples that compare them: `assets/environments/studio.exr` with the same linear light,
//! and `studio.png` and `studio.jpg` in sRGB, whose light stops at 1 as a photograph's does.
//!
//! Run after `python3 tools/generate_studio_hdr.py`:
//! `cargo run -p far-core --example studio_environments`

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/environments");
    let mut reader = image::ImageReader::open(folder.join("studio.hdr"))?;
    reader.set_format(image::ImageFormat::Hdr);
    let light = reader.decode()?.into_rgb32f();

    // 32-bit floats, compressed without loss, so the EXR holds the HDR's light exactly.
    use exr::prelude::*;
    let (width, height) = (light.width() as usize, light.height() as usize);
    let channels = SpecificChannels::rgb(|Vec2(x, y): Vec2<usize>| {
        let [r, g, b] = light.get_pixel(x as u32, y as u32).0;
        (r, g, b)
    });
    let attributes = LayerAttributes { software_name: Some(Text::from("Far Ray Tracer")), ..LayerAttributes::default() };
    Image::from_layer(Layer::new((width, height), attributes, Encoding::SMALL_LOSSLESS, channels))
        .write().to_file(folder.join("studio.exr"))?;

    let encode = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    let srgb = image::RgbImage::from_fn(light.width(), light.height(), |x, y| {
        image::Rgb(light.get_pixel(x, y).0.map(encode))
    });
    srgb.save_with_format(folder.join("studio.png"), image::ImageFormat::Png)?;
    let mut jpeg = std::fs::File::create(folder.join("studio.jpg"))?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92).encode_image(&srgb)?;

    for name in ["studio.exr", "studio.png", "studio.jpg"] {
        println!("{}", folder.join(name).display());
    }
    Ok(())
}
