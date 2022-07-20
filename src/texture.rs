//! contains convenience wrappers and utility functions for handling directx textures.

use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_420_OPAQUE, DXGI_FORMAT_AYUV, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_Y410};

/// Convenient wrapper over ID3D11Texture2D interface to retrieve dimensions, pixel format, read
/// pixels to system memory or store texture as an image.
#[repr(C)]
#[derive(Clone)]
pub struct Texture {
    tex: ID3D11Texture2D,
    desc: Option<TextureDesc>,
}

impl Texture {
    /// create new instance of texture
    pub fn new(tex: ID3D11Texture2D) -> Self {
        Texture {
            tex,
            desc: None,
        }
    }

    /// retrieve description of current texture
    pub fn desc(&mut self) -> TextureDesc {
        if self.desc.is_none() {
            let mut desc = Default::default();
            unsafe { self.tex.GetDesc(&mut desc); }

            self.desc = Some(TextureDesc {
                height: desc.Height,
                width: desc.Width,
                format: ColorFormat::from(desc.Format),
            })
        }
        self.desc.unwrap()
    }

    /// get reference of internal texture instance
    pub fn as_raw_ref(&self) -> &ID3D11Texture2D {
        &self.tex
    }
}

/// Describes a texture's basic properties.
#[repr(C)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct TextureDesc {
    pub height: u32,
    pub width: u32,
    pub format: ColorFormat,
}


/// enumeration of color formats. this is mainly used to convert color formats
/// from different libraries into a common format.
///
/// when using this in your own project, feel free to implement From and Into
/// traits that convert from other packages like nvenc or intel quick sync.
#[repr(u32)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ColorFormat {
    Unknown,

    // regular formats
    RGBA8UNorm,
    YUV444,
    YUV420,
    NV12,

    // 10 bit options
    RGBA16Float,
    RGBA10UNorm,
    Y410,
}

impl From<DXGI_FORMAT> for ColorFormat {
    fn from(f: DXGI_FORMAT) -> Self {
        match f {
            DXGI_FORMAT_B8G8R8A8_UNORM => {
                Self::RGBA8UNorm
            }
            DXGI_FORMAT_AYUV => {
                Self::YUV444
            }
            DXGI_FORMAT_NV12 => {
                Self::NV12
            }
            DXGI_FORMAT_R16G16B16A16_FLOAT => {
                Self::RGBA16Float
            }
            DXGI_FORMAT_R10G10B10A2_UNORM => {
                Self::RGBA10UNorm
            }
            DXGI_FORMAT_Y410 => {
                Self::Y410
            }
            _ => {
                Self::Unknown
            }
        }
    }
}

impl From<ColorFormat> for DXGI_FORMAT {
    fn from(f: ColorFormat) -> Self {
        match f {
            ColorFormat::RGBA8UNorm => {
                DXGI_FORMAT_B8G8R8A8_UNORM
            }
            ColorFormat::YUV444 => {
                DXGI_FORMAT_AYUV
            }
            ColorFormat::NV12 => {
                DXGI_FORMAT_NV12
            }
            ColorFormat::RGBA16Float => {
                DXGI_FORMAT_R16G16B16A16_FLOAT
            }
            ColorFormat::RGBA10UNorm => {
                DXGI_FORMAT_R10G10B10A2_UNORM
            }
            ColorFormat::Y410 => {
                DXGI_FORMAT_Y410
            }
            ColorFormat::Unknown => {
                DXGI_FORMAT(0)
            }
            ColorFormat::YUV420 => {
                DXGI_FORMAT_420_OPAQUE
            }
        }
    }
}