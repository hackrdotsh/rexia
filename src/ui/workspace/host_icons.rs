//! Local host artwork. GPUI caches the decoded frames and animates only the active image.
use super::Workspace;
use anyhow::{Result, ensure};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Asset, Context, IntoElement, ObjectFit, PathPromptOptions, RenderImage,
    Styled, Window, img, px,
};
use image::{
    AnimationDecoder, Frame, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage,
    codecs::gif::GifDecoder,
};
use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FRAMES: usize = 240;
const MAX_ANIMATED_PIXELS: u64 = 64 * 1024 * 1024;
const ICON_PIXELS: u32 = 64;

#[derive(Clone)]
struct HostImage {
    still: Arc<RenderImage>,
    animated: Arc<RenderImage>,
}

impl HostImage {
    fn source(&self, animate: bool) -> Arc<RenderImage> {
        if animate {
            self.animated.clone()
        } else {
            self.still.clone()
        }
    }
}

fn load_image(path: &Path) -> Result<HostImage> {
    ensure!(path.metadata()?.is_file(), "Choose an image file");
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "Choose an image smaller than 8 MiB"
    );
    let format = image::guess_format(&bytes)?;
    ensure!(
        matches!(
            format,
            ImageFormat::Gif | ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
        ),
        "Choose a PNG, JPEG, WebP, or GIF image"
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    let mut frames = Vec::new();
    if format == ImageFormat::Gif {
        let mut gif = GifDecoder::new(BufReader::new(Cursor::new(bytes)))?;
        gif.set_limits(limits)?;
        let (width, height) = gif.dimensions();
        let frame_pixels = u64::from(width) * u64::from(height);
        for frame in gif.into_frames() {
            ensure!(
                frames.len() < MAX_FRAMES,
                "Choose a GIF with at most 240 frames"
            );
            ensure!(
                frame_pixels * (frames.len() as u64 + 1) <= MAX_ANIMATED_PIXELS,
                "Choose a shorter or smaller GIF"
            );
            let frame = frame?;
            let delay = Duration::from(frame.delay()).max(Duration::from_millis(20));
            frames.push(Frame::from_parts(
                bgra_icon(frame.into_buffer()),
                0,
                0,
                image::Delay::from_saturating_duration(delay),
            ));
        }
    } else {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits);
        frames.push(Frame::new(bgra_icon(reader.decode()?.into_rgba8())));
    }
    let first = frames
        .first()
        .ok_or_else(|| anyhow::anyhow!("This image has no frames"))?;
    let still = Arc::new(RenderImage::new(vec![first.clone()]));
    let animated = if frames.len() == 1 {
        still.clone()
    } else {
        Arc::new(RenderImage::new(frames))
    };
    Ok(HostImage { still, animated })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Delay, Rgba, codecs::gif::GifEncoder};

    #[test]
    fn host_icons_keep_gif_frames_only_for_playback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("host.gif");
        let mut gif = GifEncoder::new(File::create(&path).unwrap());
        for (rgba, ms) in [([255, 0, 0, 255], 50), ([0, 0, 255, 255], 120)] {
            gif.encode_frame(Frame::from_parts(
                RgbaImage::from_pixel(128, 64, Rgba(rgba)),
                0,
                0,
                Delay::from_numer_denom_ms(ms, 1),
            ))
            .unwrap();
        }
        drop(gif);
        let artwork = load_image(&path).unwrap();
        let playback = artwork.source(true);
        let still = artwork.source(false);
        assert_eq!(playback.frame_count(), 2);
        assert_eq!(still.frame_count(), 1);
        assert_eq!(still.as_bytes(0), playback.as_bytes(0));
        assert_eq!(&still.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        assert_eq!(&playback.as_bytes(1).unwrap()[..4], &[255, 0, 0, 255]);
        assert_eq!(playback.size(0).width.0, 64);
        assert_eq!(playback.size(0).height.0, 32);
        assert_eq!(
            Duration::from(playback.delay(1)),
            Duration::from_millis(120)
        );
    }

    #[test]
    fn host_icons_save_local_paths_and_validate_static_images() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("host.png");
        RgbaImage::from_pixel(16, 16, Rgba([80, 120, 180, 255]))
            .save(&path)
            .unwrap();
        let image = load_image(&path).unwrap();
        assert!(Arc::ptr_eq(&image.source(true), &image.source(false)));

        let settings_path = directory.path().join("settings.toml");
        let mut settings = crate::settings::Settings::default();
        settings
            .host_icons
            .insert("host:stored-id".into(), path.clone());
        settings.save(&settings_path).unwrap();
        let loaded = crate::settings::Settings::load(&settings_path).unwrap();
        assert_eq!(loaded.host_icons, settings.host_icons);
        assert!(load_image(&settings_path).is_err());
        assert!(load_image(directory.path()).is_err());
        assert!(load_image(&directory.path().join("missing.gif")).is_err());
    }
}

fn bgra_icon(image: RgbaImage) -> RgbaImage {
    let mut icon = image::DynamicImage::ImageRgba8(image)
        .thumbnail(ICON_PIXELS, ICON_PIXELS)
        .into_rgba8();
    for pixel in icon.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    icon
}

enum HostImageAsset {}

impl Asset for HostImageAsset {
    type Source = PathBuf;
    type Output = Option<HostImage>;

    #[expect(
        clippy::manual_async_fn,
        reason = "the future must not capture the App reference"
    )]
    fn load(path: PathBuf, _: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        async move { load_image(&path).ok() }
    }
}

impl Workspace {
    pub(super) fn host_icon(&self, host_id: &str, selected: bool) -> Option<AnyElement> {
        let path = self.settings.host_icons.get(host_id)?.clone();
        let id = gpui::SharedString::from(format!("host-image:{host_id}"));
        Some(
            img(move |window: &mut Window, cx: &mut App| {
                let animate = selected && window.is_window_active();
                window
                    .use_asset::<HostImageAsset>(&path, cx)
                    .flatten()
                    .map(|image| Ok(image.source(animate)))
            })
            .id(id)
            .flex_none()
            .size(px(18.))
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        )
    }

    pub(super) fn choose_host_icon(
        &mut self,
        host_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose Host Icon".into()),
        });
        let executor = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let path = match picker.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    let _ = this.update_in(cx, |ws, _, cx| {
                        ws.notice = Some(format!("Couldn't open the image picker: {error}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(path) = path else { return };
            let check = path.clone();
            let result = executor
                .spawn(async move { load_image(&check).map(|_| ()) })
                .await;
            let _ = this.update_in(cx, |ws, _, cx| match result {
                Ok(()) if ws.hosts.iter().any(|host| host.config.id == host_id) => {
                    cx.remove_asset::<HostImageAsset>(&path);
                    ws.update_settings(
                        |s| {
                            s.host_icons.insert(host_id, path);
                        },
                        cx,
                    );
                }
                Ok(()) => {}
                Err(error) => {
                    ws.notice = Some(format!("Couldn't use this host icon: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
