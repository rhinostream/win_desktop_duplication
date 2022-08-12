//! contains convenience wrappers and utility functions for handling directx textures.

use std::sync::{Arc, RwLock};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_AYUV, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_FORMAT_P010, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_Y410};

/// Convenient wrapper over ID3D11Texture2D interface to retrieve dimensions, pixel format, read
/// pixels to system memory or store texture as an image.
#[repr(C)]
#[derive(Clone)]
pub struct Texture {
    tex: ID3D11Texture2D,
    desc: Arc<RwLock<Option<TextureDesc>>>,
}

impl Texture {
    /// create new instance of texture
    pub fn new(tex: ID3D11Texture2D) -> Self {
        Texture {
            tex,
            desc: Arc::new(RwLock::new(None)),
        }
    }

    /// retrieve description of current texture
    pub fn desc(&self) -> TextureDesc {
        {
            let desc = self.desc.read().unwrap();
            if desc.is_some() {
                return desc.unwrap();
            }
        }
        let mut desc = Default::default();
        unsafe { self.tex.GetDesc(&mut desc); }
        let mut tex_desc = TextureDesc {
            height: desc.Height,
            width: desc.Width,
            format: ColorFormat::from(desc.Format),
        };
        if matches!(tex_desc.format, ColorFormat::YUV444|ColorFormat::YUV444_10bit) {
            tex_desc.height = tex_desc.height / 3;
        }

        let mut desc_wr = self.desc.write().unwrap();

        (*desc_wr) = Some(tex_desc);
        return desc_wr.unwrap();
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
/// you can convert between DXGI_FORMAT and this format using into.
///
/// For example:
///
/// ```
///     let format_dxgi: DXGI_FORMAT = ColorFormat::ARGBUNorm.into();
///     let format: ColorFormat = DXGI_FORMAT_R8G8B8A8_UNORM.into();
/// ```
///
/// when using this in your own project, feel free to implement From and Into
/// traits that convert from other packages like nvenc or intel quick sync.
#[repr(u32)]
#[derive(Clone, Copy, Eq, PartialEq, Default, Debug)]
pub enum ColorFormat {
    #[default]
    Unknown,

    // regular formats
    /// Packed 8bit per pixel ARGB unsigned normalized int format
    ARGB8UNorm,

    /// Packed 8bit per pixel ABGR unsigned normalized int format
    ABGR8UNorm,

    /// planar 8bit per pixel YUV 4:4:4 format
    YUV444,

    /// packed 8bit per pixel AYUV 4:4:4 format with alpha channel
    AYUV,

    /// planar 8bit per pixel YUV 4:2:0 format u,v planes have half height and half width of Y plane
    YUV420,

    /// semi planar 8bit per pixel YUV 4:2:0. Y followed by interleaved u,v plane.
    NV12,

    // 10 bit options
    /// packed 16 bits per pixel ARGB float format.
    ARGB16Float,

    /// packed 10 bits per channel for R,G,B channels and 2 bits for alpha channel. total 32 bits per pixel
    ARGB10UNorm,

    /// packed 10 bits per channel for YUV and 2 bits for alpha channel. YUV 4:4:4 format
    Y410,

    /// planar 16bit per pixel YUV 4:4:4 format. (only 10 significant bits will be used)
    YUV444_10bit,

    /// 16 bit Semi-Planar YUV. Y plane followed by interleaved UV plane . Each pixel of size 2 bytes. Most Significant 10 bits contain pixel data.
    /// this format is also called P010
    YUV420_10bit,
}

#[macro_use]
mod gen {
    macro_rules! generate_map {
        ($t1:ident $t2:ident {$(($o1:path, $o2:path)),+}) =>{
            impl From<$t1> for $t2 {
                fn from (f: $t1)->$t2 {
                    match f {
                        $(
                        $o1 => {
                            $o2
                        }
                        )*
                        _ => {
                            Default::default()
                        }
                    }
                }
            }
            impl From<$t2> for $t1 {
                fn from (f: $t2)->$t1 {
                    match f {
                        $(
                        $o2  => {
                            $o1
                        }
                        )*
                        _ => {
                            Default::default()
                        }
                    }
                }
            }

        }
    }
}

// implements from trait for both types.
generate_map!(DXGI_FORMAT ColorFormat {
    (DXGI_FORMAT_R8G8B8A8_UNORM, ColorFormat::ARGB8UNorm),

    (DXGI_FORMAT_B8G8R8A8_UNORM, ColorFormat::ABGR8UNorm),

    (DXGI_FORMAT_AYUV, ColorFormat::AYUV),

    (DXGI_FORMAT_R8_UNORM, ColorFormat::YUV444),

    (DXGI_FORMAT_R16_UNORM, ColorFormat::YUV444_10bit),

    (DXGI_FORMAT_NV12, ColorFormat::NV12),

    (DXGI_FORMAT_R16G16B16A16_FLOAT, ColorFormat::ARGB16Float),

    (DXGI_FORMAT_R10G10B10A2_UNORM, ColorFormat::ARGB10UNorm),

    (DXGI_FORMAT_Y410, ColorFormat::Y410),

    (DXGI_FORMAT_P010, ColorFormat::YUV420_10bit)
});
