//! Provides convenient tools for handling directx textures. [`TextureReader`][TextureReader] can be used to read
//! textures.

#[cfg(test)]
mod test {
    use std::sync::Once;
    use std::time::Duration;
    use futures::{select, FutureExt};
    use log::LevelFilter::Debug;
    use tokio::time::interval;
    use crate::{co_init, DesktopDuplicationApi, set_process_dpi_awareness};
    use crate::devices::AdapterFactory;
    use crate::tex_reader::TextureReader;

    static INIT: Once = Once::new();

    pub fn initialize() {
        INIT.call_once(|| {
            let _ = env_logger::builder().is_test(true).filter_level(Debug).try_init();
        });
    }

    #[test]
    fn test_texture_reader() {
        initialize();

        let rt = tokio::runtime::Builder::new_current_thread()
            .thread_name("graphics_thread".to_owned()).enable_time().build().unwrap();

        rt.block_on(async {
            set_process_dpi_awareness();
            co_init();

            let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
            let output = adapter.get_display_by_idx(0).unwrap();
            let mut dupl = DesktopDuplicationApi::new(adapter, output.clone()).unwrap();

            let (dev, ctx) = dupl.get_device_and_ctx();
            let mut reader = TextureReader::new(dev, ctx);

            let mut counter = 0;
            let mut secs = 0;
            let mut interval = interval(Duration::from_secs(1));
            let mut data = Vec::<u8>::new();
            loop {
                select! {
                    tex = dupl.acquire_next_vsync_frame().fuse()=>{
                        if let Err(e) = tex {
                            println!("error: {:?}",e)
                        } else {
                            let tex = tex.unwrap();
                            reader.get_data(&mut data,&tex).unwrap();
                            let pitch = tex.desc().width as usize *4;
                            println!("pitch: {}",pitch);
                            for i in 0..4{
                                for j in 0..12{
                                    print!("{}\t",data[pitch*(i+1)-(12-(j))]);
                                }
                                print!("\n");
                            }
                            print!("\n");
                            counter += 1;
                        };
                    },
                    _ = interval.tick().fuse() => {
                        println!("fps: {}",counter);
                        counter = 0;
                        secs+=1;
                        if secs ==5 {
                            break;
                        }
                    }
                }
                ;
            };
        });
    }
}

use std::ptr::{copy, null};
use windows::Win32::Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING, ID3D11Device4, ID3D11DeviceContext4};
use crate::texture::{ColorFormat, Texture};
use crate::{DDApiError, Result};


/// Tool for reading GPU only directx textures.
///
/// # Example usage
///
/// ```
/// use win_desktop_duplication::tex_reader::TextureReader;
///
/// let mut reader = TextureReader::new(device, context);
///
/// // using same vector will be so much efficient.
/// let mut data:Vec<u8> = Vec::new();
///
/// loop {
///     let tex = // some way to acquire texture like DesktopDuplicationApi;
///
///     reader.get_data(&mut data,&tex).unwrap();
///
///     // use image data here. send it to client etc whatever
/// }
/// ```
pub struct TextureReader {
    device: ID3D11Device4,
    ctx: ID3D11DeviceContext4,
    tex: Option<Texture>,
}

unsafe impl Sync for TextureReader {}

unsafe impl Send for TextureReader {}

impl TextureReader {
    /// create new instance of TextureReader
    pub fn new(device: ID3D11Device4, ctx: ID3D11DeviceContext4) -> Self {
        Self {
            device,
            ctx,
            tex: None,
        }
    }

    /// retrieve data from texture and store it in vector
    pub fn get_data(&mut self, vec: &mut Vec<u8>, tex: &Texture) -> Result<()> {
        self.ensure_shape(tex)?;
        unsafe { self.ctx.CopyResource(self.tex.as_mut().unwrap().as_raw_ref(), tex.as_raw_ref()); }
        unsafe { self.ctx.Flush() }
        let raw_tex = self.tex.as_mut().unwrap().as_raw_ref();
        let mut sub_res = D3D11_MAPPED_SUBRESOURCE::default();
        if let Err(e) = unsafe { self.ctx.Map(raw_tex, 0, D3D11_MAP_READ, 0, Some(&mut sub_res)) } {
            return Err(DDApiError::Unexpected(format!("failed to map to cpu {:?}", e)));
        }
        let desc = tex.desc();

        match desc.format {
            ColorFormat::ABGR8UNorm | ColorFormat::ARGB8UNorm | ColorFormat::AYUV => {
                let total_size = desc.width * desc.height * 4;
                vec.resize(total_size as usize, 0);
                for i in 0..desc.height {
                    unsafe { copy(sub_res.pData.add((i * sub_res.RowPitch) as usize) as *const u8, vec.as_mut_ptr().add((i * desc.width * 4) as _), (desc.width * 4) as usize); }
                }
            }
            ColorFormat::YUV444 => {
                let total_size = desc.width * desc.height * 3;
                vec.resize(total_size as usize, 0);
                for i in 0..(desc.height * 3) {
                    unsafe { copy(sub_res.pData.add((i * sub_res.RowPitch) as usize) as *const u8, vec.as_mut_ptr().add((i * desc.width) as _), (desc.width) as usize); }
                }
            }
            ColorFormat::NV12 => {
                let total_size = desc.width * desc.height * 3 / 2;
                vec.resize(total_size as usize, 0);
                for i in 0..(3 * desc.height / 2) {
                    unsafe { copy(sub_res.pData.add((i * sub_res.RowPitch) as usize) as *const u8, vec.as_mut_ptr().add((i * desc.width) as _), (desc.width) as usize); }
                }
            }

            _ => unimplemented!()
        }
        unsafe { self.ctx.Unmap(raw_tex, 0); }

        Ok(())
    }

    fn ensure_shape(&mut self, tex: &Texture) -> Result<()> {
        if self.tex.is_none() || self.tex.as_mut().unwrap().desc() != tex.desc() {
            self.tex = None;
            let mut desc = Default::default();
            unsafe { tex.as_raw_ref().GetDesc(&mut desc) };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = Default::default();
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            desc.MiscFlags = Default::default();

            let mut new_tex = None;

            if let Err(e) = unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut new_tex)) } {
                return Err(DDApiError::Unexpected(format!("failed to create texture. {:?}", e)));
            }
            self.tex = Some(Texture::new(new_tex.unwrap()))
        }

        Ok(())
    }
}