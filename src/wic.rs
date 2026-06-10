use eframe::egui::Color32;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::HRESULT;

pub(crate) const RPC_E_CHANGED_MODE_HRESULT: HRESULT = HRESULT(0x80010106u32 as i32);

/// Premultiplied-alpha pixels, ready to use as an egui texture without any
/// further per-pixel conversion.
pub struct DecodedImage {
    pub pixels: Vec<Color32>,
    pub width: u32,
    pub height: u32,
}

pub struct WicContext {
    factory: Option<IWICImagingFactory>,
    should_uninitialize: bool,
}

impl WicContext {
    pub fn new() -> Result<Self, String> {
        let should_uninitialize = unsafe {
            let res = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if res == RPC_E_CHANGED_MODE_HRESULT {
                false
            } else if res.is_err() {
                return Err(format!("Failed to initialize COM: {:?}", res));
            } else {
                true
            }
        };

        unsafe {
            let factory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create WIC Factory: {:?}", e))?;

            Ok(Self {
                factory: Some(factory),
                should_uninitialize,
            })
        }
    }

    fn factory(&self) -> Result<&IWICImagingFactory, String> {
        self.factory
            .as_ref()
            .ok_or_else(|| "WIC factory is unavailable".to_string())
    }

    /// Reads the image dimensions from the header without decoding pixels.
    pub fn image_size(&self, bytes: &[u8]) -> Result<(u32, u32), String> {
        let factory = self.factory()?;

        unsafe {
            let stream = factory
                .CreateStream()
                .map_err(|e| format!("Failed to create stream: {:?}", e))?;
            stream
                .InitializeFromMemory(bytes)
                .map_err(|e| format!("Failed to init stream: {:?}", e))?;
            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .map_err(|e| format!("Failed to create decoder: {:?}", e))?;
            let frame = decoder
                .GetFrame(0)
                .map_err(|e| format!("Failed to get frame: {:?}", e))?;

            let mut width = 0;
            let mut height = 0;
            frame
                .GetSize(&mut width, &mut height)
                .map_err(|e| format!("Failed to get size: {:?}", e))?;
            if width == 0 || height == 0 {
                return Err("Image has zero dimensions".to_string());
            }
            Ok((width, height))
        }
    }

    /// Decodes the image scaled down to at most `target_width` x `target_height`
    /// (clamped to the native size) using WIC's Fant resampler.
    pub fn decode_scaled(
        &self,
        bytes: &[u8],
        target_width: u32,
        target_height: u32,
    ) -> Result<DecodedImage, String> {
        let factory = self.factory()?;

        unsafe {
            let stream = factory
                .CreateStream()
                .map_err(|e| format!("Failed to create stream: {:?}", e))?;
            stream
                .InitializeFromMemory(bytes)
                .map_err(|e| format!("Failed to init stream: {:?}", e))?;
            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .map_err(|e| format!("Failed to create decoder: {:?}", e))?;
            let frame = decoder
                .GetFrame(0)
                .map_err(|e| format!("Failed to get frame: {:?}", e))?;

            let mut native_width = 0;
            let mut native_height = 0;
            frame
                .GetSize(&mut native_width, &mut native_height)
                .map_err(|e| format!("Failed to get size: {:?}", e))?;
            if native_width == 0 || native_height == 0 {
                return Err("Image has zero dimensions".to_string());
            }

            let width = target_width.clamp(1, native_width);
            let height = target_height.clamp(1, native_height);

            let converter = factory
                .CreateFormatConverter()
                .map_err(|e| format!("Failed to create format converter: {:?}", e))?;

            // Premultiplied RGBA matches egui's Color32 byte layout, so the
            // decoded buffer is used as texture pixels with no conversion pass.
            if width == native_width && height == native_height {
                converter
                    .Initialize(
                        &frame,
                        &GUID_WICPixelFormat32bppPRGBA,
                        WICBitmapDitherTypeNone,
                        None,
                        0.0,
                        WICBitmapPaletteTypeMedianCut,
                    )
                    .map_err(|e| format!("Failed to initialize format converter: {:?}", e))?;
            } else {
                let scaler = factory
                    .CreateBitmapScaler()
                    .map_err(|e| format!("Failed to create scaler: {:?}", e))?;
                scaler
                    .Initialize(&frame, width, height, WICBitmapInterpolationModeFant)
                    .map_err(|e| format!("Failed to initialize scaler: {:?}", e))?;
                converter
                    .Initialize(
                        &scaler,
                        &GUID_WICPixelFormat32bppPRGBA,
                        WICBitmapDitherTypeNone,
                        None,
                        0.0,
                        WICBitmapPaletteTypeMedianCut,
                    )
                    .map_err(|e| format!("Failed to initialize format converter: {:?}", e))?;
            }

            let mut pixels = vec![Color32::TRANSPARENT; width as usize * height as usize];
            let stride = width * 4;
            let buffer = std::slice::from_raw_parts_mut(
                pixels.as_mut_ptr().cast::<u8>(),
                pixels.len() * 4,
            );
            converter
                .CopyPixels(std::ptr::null(), stride, buffer)
                .map_err(|e| format!("Failed to copy pixels: {:?}", e))?;

            Ok(DecodedImage {
                pixels,
                width,
                height,
            })
        }
    }

    pub fn load_from_memory(&self, bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
        let factory = self.factory()?;

        unsafe {
            let stream = factory
                .CreateStream()
                .map_err(|e| format!("Failed to create stream: {:?}", e))?;

            stream
                .InitializeFromMemory(bytes)
                .map_err(|e| format!("Failed to init stream: {:?}", e))?;

            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .map_err(|e| format!("Failed to create decoder: {:?}", e))?;

            let frame = decoder
                .GetFrame(0)
                .map_err(|e| format!("Failed to get frame: {:?}", e))?;

            let mut width = 0;
            let mut height = 0;
            frame
                .GetSize(&mut width, &mut height)
                .map_err(|e| format!("Failed to get size: {:?}", e))?;

            let converter = factory
                .CreateFormatConverter()
                .map_err(|e| format!("Failed to create format converter: {:?}", e))?;

            converter
                .Initialize(
                    &frame,
                    &GUID_WICPixelFormat32bppRGBA,
                    WICBitmapDitherTypeNone,
                    None, // pIPalette
                    0.0,
                    WICBitmapPaletteTypeMedianCut,
                )
                .map_err(|e| format!("Failed to initialize format converter: {:?}", e))?;

            let stride = width * 4;
            let mut buffer = vec![0u8; (stride * height) as usize];

            converter
                .CopyPixels(
                    std::ptr::null(), // prc
                    stride,
                    &mut buffer,
                )
                .map_err(|e| format!("Failed to copy pixels: {:?}", e))?;

            Ok((buffer, width, height))
        }
    }
}

impl Drop for WicContext {
    fn drop(&mut self) {
        // Ensure COM objects are released before COM uninitialization.
        self.factory.take();

        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
