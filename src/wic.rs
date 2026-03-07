use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::HRESULT;

const RPC_E_CHANGED_MODE_HRESULT: HRESULT = HRESULT(0x80010106u32 as i32);

pub struct WicContext {
    should_uninitialize: bool,
}

impl WicContext {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let res = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if res == RPC_E_CHANGED_MODE_HRESULT {
                return Ok(Self {
                    should_uninitialize: false,
                });
            }

            if res.is_err() {
                return Err(format!("Failed to initialize COM: {:?}", res));
            }
        }
        Ok(Self {
            should_uninitialize: true,
        })
    }

    pub fn load_from_memory(&self, bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("Failed to create WIC Factory: {:?}", e))?;

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
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
