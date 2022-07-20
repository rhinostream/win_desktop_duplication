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
                            let mut tex = tex.unwrap();
                            reader.get_data(&mut data,&mut tex).unwrap();
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
use crate::texture::Texture;
use crate::{DDApiError, Result};


pub struct TextureReader {
    device: ID3D11Device4,
    ctx: ID3D11DeviceContext4,
    tex: Option<Texture>,
}

unsafe impl Sync for TextureReader {}

unsafe impl Send for TextureReader {}

impl TextureReader {
    pub fn new(device: ID3D11Device4, ctx: ID3D11DeviceContext4) -> Self {
        Self {
            device,
            ctx,
            tex: None,
        }
    }

    pub fn get_data(&mut self, vec: &mut Vec<u8>, tex: &mut Texture) -> Result<()> {
        self.ensure_shape(tex)?;
        unsafe { self.ctx.CopyResource(self.tex.as_mut().unwrap().as_raw_ref(), tex.as_raw_ref()); }
        unsafe { self.ctx.Flush() }
        let raw_tex = self.tex.as_mut().unwrap().as_raw_ref();
        let sub_res = unsafe { self.ctx.Map(raw_tex, 0, D3D11_MAP_READ, 0) };
        if sub_res.is_err() {
            return Err(DDApiError::Unexpected(format!("failed to map to cpu {:?}", sub_res)));
        }
        let sub_res: D3D11_MAPPED_SUBRESOURCE = sub_res.unwrap();

        let desc = tex.desc();
        let total_size = desc.width * desc.height * 4;
        vec.resize(total_size as usize, 0);
        for i in 0..desc.height {
            unsafe { copy(sub_res.pData.add((i * sub_res.RowPitch) as usize) as *const u8, vec.as_mut_ptr().add((i * desc.width * 4) as _), (desc.width * 4) as usize); }
        }
        unsafe { self.ctx.Unmap(raw_tex, 0); }

        Ok(())
    }

    fn ensure_shape(&mut self, tex: &mut Texture) -> Result<()> {
        if self.tex.is_none() || self.tex.as_mut().unwrap().desc() != tex.desc() {
            self.tex = None;
            let mut desc = Default::default();
            unsafe { tex.as_raw_ref().GetDesc(&mut desc) };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = Default::default();
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
            desc.MiscFlags = Default::default();

            let new_tex = unsafe { self.device.CreateTexture2D(&desc, null()) };
            if new_tex.is_err() {
                return Err(DDApiError::Unexpected(format!("failed to create texture. {:?}", new_tex)));
            }
            self.tex = Some(Texture::new(new_tex.unwrap()))
        }

        Ok(())
    }
}