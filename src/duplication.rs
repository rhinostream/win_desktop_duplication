//! # Windows Desktop Duplication
//! Module provides a convenient wrapper for [windows desktop duplication api](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
//! while adding few features to it.
//!
//! For more information on how to use check [DesktopDuplicationApi]

#[cfg(test)]
mod test {
    use std::sync::Once;
    use std::time::Duration;
    use futures::select;
    use crate::devices::AdapterFactory;
    use crate::duplication::DesktopDuplicationApi;
    use futures::FutureExt;
    use log::LevelFilter::Debug;
    use tokio::time::interval;
    use crate::utils::{co_init, set_process_dpi_awareness};


    static INIT: Once = Once::new();

    pub fn initialize() {
        INIT.call_once(|| {
            let _ = env_logger::builder().is_test(true).filter_level(Debug).try_init();
        });
    }

    #[test]
    fn test_duplication() {
        initialize();
        set_process_dpi_awareness();

        let rt = tokio::runtime::Builder::new_current_thread()
            .thread_name("graphics_thread".to_owned()).enable_time().build().unwrap();

        rt.block_on(async {
            co_init();

            let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
            let output = adapter.get_display_by_idx(0).unwrap();
            let mut dupl = DesktopDuplicationApi::new(adapter, output).unwrap();

            let mut counter = 0;
            let mut secs = 0;
            let mut interval = interval(Duration::from_secs(1));
            loop {
                select! {
                    tex = dupl.acquire_next_vsync_frame().fuse()=>{
                        tex.unwrap();
                        counter = counter+1;
                    },
                    _ = interval.tick().fuse() => {
                        println!("fps: {}",counter);
                        counter = 0;
                        secs+=1;
                        if secs == 5 {
                            break;
                        }
                    }
                }
                ;
            };
        });
    }
}


use std::mem::{size_of, swap};
use std::ptr::null;
use futures::{StreamExt};
use log::{debug, error, trace, warn};
use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_FLAG, D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_FLAG, D3D11_RESOURCE_MISC_FLAG, D3D11_RESOURCE_MISC_GDI_COMPATIBLE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device4, ID3D11DeviceContext4};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_SESSION_DISCONNECTED, IDXGIDevice4, IDXGIOutputDuplication, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_ACCESS_DENIED, DXGI_ERROR_INVALID_CALL, IDXGISurface1, DXGI_ERROR_WAIT_TIMEOUT};
use windows::core::Interface;
use windows::core::Result as WinResult;
use windows::Win32::Foundation::{E_INVALIDARG, E_ACCESSDENIED, POINT, GetLastError, BOOL};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::System::StationsAndDesktops::{OpenInputDesktop, SetThreadDesktop};
use windows::Win32::System::SystemServices::GENERIC_READ;
use windows::Win32::UI::WindowsAndMessaging::{CURSOR_SHOWING, CURSORINFO, DF_ALLOWOTHERACCOUNTHOOK, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HCURSOR};
use crate::devices::Adapter;
use crate::errors::DDApiError;
use crate::outputs::{Display, DisplayVSyncStream};
use crate::Result;
use crate::texture::{Texture, TextureDesc};

/// Provides asynchronous api for windows desktop duplication with additional features such as
/// cursor pre-drawn, frame rate synced to desktop refresh rate.
///
/// please note that this api works best if created and called from a single thread.
/// Ideal scenario would be to maintain a "Graphics thread" in your application where all the
/// Graphics related tasks are performed asynchronously.
///
/// acquire_next_frame especially should be called from only one thread because it only works if the
/// thread calling it is marked as desktop thread. Although the application attempts to set any
/// thread you call this method from as desktop thread, it's not usually a good idea.
///
/// # Example
/// ```
/// use win_desktop_duplication::duplication::DesktopDuplicationApi;
/// async {
///     let mut duplication = DesktopDuplicationApi::new(adapter, output)?;
///     loop {
///         let tex = duplication.acquire_next_vsync_frame().await?;
///         // use the texture to encode video
///     }
///
/// }
/// ```
pub struct DesktopDuplicationApi {
    d3d_device: ID3D11Device4,
    d3d_ctx: ID3D11DeviceContext4,
    output: Display,
    vsync_stream: DisplayVSyncStream,
    dupl: IDXGIOutputDuplication,

    options: DuplicationApiOptions,

    state: DuplicationState,

}

unsafe impl Send for DesktopDuplicationApi {}

unsafe impl Sync for DesktopDuplicationApi {}


impl DesktopDuplicationApi {
    /// Create a new instance of Desktop Duplication api from the provided [adapter][Adapter] and
    /// [display][Display]. The application auto creates directx device and context from provided
    /// adapter.
    ///
    /// If you wish to use your own directx device, context, use [new_with][Self::new_with] method
    pub fn new(adapter: Adapter, output: Display) -> Result<Self> {
        let (device, ctx) = Self::create_device(&adapter)?;
        Self::new_with(device, ctx, output)
    }

    /// Creates a new instance of the api from provided device and context.
    pub fn new_with(d3d_device: ID3D11Device4, ctx: ID3D11DeviceContext4, output: Display) -> Result<Self> {
        // Self::switch_thread_desktop()?;
        let dupl = Self::create_dupl_output(&d3d_device, &output)?;
        Ok(Self {
            d3d_device,
            d3d_ctx: ctx,
            vsync_stream: output.get_vsync_stream(),
            output,
            dupl,
            options: Default::default(),
            state: Default::default(),
        })
    }

    /// Acquire next frame from the desktop duplication api after waiting for vsync refresh.
    /// this helps application acquire frames with same rate as display's native refresh-rate.
    ///
    /// this is an asynchronous method. check example in the [doc][DesktopDuplicationApi] for more details
    pub async fn acquire_next_vsync_frame(&mut self) -> Result<Texture> {
        // wait for vsync
        if (self.vsync_stream.next().await).is_none() {
            return Err(DDApiError::Unexpected("DisplayVSyncStream failed unexpectedly".to_owned()));
        }

        // acquire next_frame
        self.acquire_next_frame()
    }

    fn create_device(adapter: &Adapter) -> Result<(ID3D11Device4, ID3D11DeviceContext4)> {
        let feature_levels = [D3D_FEATURE_LEVEL_11_1];
        let mut feature_level: D3D_FEATURE_LEVEL = Default::default();
        let mut d3d_device = None;
        let mut d3d_ctx = None;

        let resp = unsafe {
            D3D11CreateDevice(&adapter.0, D3D_DRIVER_TYPE_UNKNOWN,
                              None, D3D11_CREATE_DEVICE_FLAG(0),
                              &feature_levels, D3D11_SDK_VERSION,
                              &mut d3d_device, &mut feature_level,
                              &mut d3d_ctx)
        };
        if resp.is_err() {
            Err(DDApiError::Unexpected(format!("faild d3d11 create device. {:?}", resp)))
        } else {
            Ok((d3d_device.unwrap().cast().unwrap(), d3d_ctx.unwrap().cast().unwrap()))
        }
    }

    fn create_dupl_output(dev: &ID3D11Device4, output: &Display) -> Result<IDXGIOutputDuplication> {
        let supported_formats = [DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT];
        let device: IDXGIDevice4 = dev.cast().unwrap();
        let dupl: WinResult<IDXGIOutputDuplication> = unsafe { output.0.DuplicateOutput1(&device, 0, &supported_formats) };

        if let Err(err) = dupl {
            return match err.code() {
                E_INVALIDARG => {
                    Err(DDApiError::BadParam(format!("failed to create duplicate output. {:?}", err)))
                }
                E_ACCESSDENIED => {
                    Err(DDApiError::AccessDenied)
                }
                DXGI_ERROR_UNSUPPORTED => {
                    Err(DDApiError::Unsupported)
                }
                DXGI_ERROR_SESSION_DISCONNECTED => {
                    Err(DDApiError::Disconnected)
                }
                _ => {
                    Err(DDApiError::Unexpected(err.to_string()))
                }
            };
        }
        Ok(dupl.unwrap())
    }

    fn acquire_next_frame(&mut self) -> Result<Texture> {
        self.release_locked_frame();

        let mut frame_info = Default::default();
        let mut last_resource = None;

        let status = unsafe { self.dupl.AcquireNextFrame(0, &mut frame_info, &mut last_resource) };

        if let Err(e) = status {
            match e.code() {
                DXGI_ERROR_ACCESS_LOST => {
                    warn!("display access lost. maybe desktop mode switch?");
                    self.reacquire_dup()?
                }
                DXGI_ERROR_ACCESS_DENIED => {
                    warn!("display access is denied. Maybe running in a secure environment?");
                    self.reacquire_dup()?
                }
                DXGI_ERROR_INVALID_CALL => {
                    warn!("dxgi_error_invalid_call. maybe forgot to ReleaseFrame()?");
                    let _ = unsafe { self.dupl.ReleaseFrame() };
                    return Err(DDApiError::AccessLost);
                }
                DXGI_ERROR_WAIT_TIMEOUT => {
                    trace!("no new frame is available")
                }
                _ => {
                    return Err(DDApiError::Unexpected(format!("acquire frame failed {:?}", e)));
                }
            }
        }

        if let Some(resource) = last_resource {
            self.state.frame_locked = true;
            let mut new_frame = Texture::new(resource.cast().unwrap());
            self.ensure_cache_frame(&mut new_frame)?;
            unsafe { self.d3d_ctx.CopyResource(self.state.frame.as_ref().unwrap().as_raw_ref(), new_frame.as_raw_ref()); }
        }
        let mut cache_frame = self.state.frame.clone().unwrap();
        self.ensure_cache_cursor_frame(&mut cache_frame)?;
        let cache_cursor_frame = self.state.cursor_frame.clone().unwrap();

        unsafe {
            self.d3d_ctx.CopyResource(
                cache_cursor_frame.as_raw_ref(),
                cache_frame.as_raw_ref())
        }

        if !self.options.skip_cursor && frame_info.PointerShapeBufferSize != 0 {
            self.draw_cursor(&cache_cursor_frame)?
        }
        Ok(cache_cursor_frame)
    }

    fn draw_cursor(&mut self, tex: &Texture) -> Result<()> {
        let mut cursor_info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let cursor_present = unsafe { GetCursorInfo(&mut cursor_info as *mut CURSORINFO) };

        // if cursor is not present, return raw frame.
        if !cursor_present.as_bool()
            || (cursor_info.flags.0 & CURSOR_SHOWING.0 != CURSOR_SHOWING.0)
        {
            warn!("cursor is absent but attempted to draw");
            return Ok(());
        }

        if self.state.cursor.is_none() || cursor_info.hCursor != *self.state.cursor.as_ref().unwrap() {
            self.state.cursor = Some(cursor_info.hCursor);
            let point = Self::get_icon_hotspot(cursor_info.hCursor)?;
            self.state.hotspot_x = point.x as _;
            self.state.hotspot_y = point.y as _;
        }

        let surface: IDXGISurface1 = tex.as_raw_ref().cast().unwrap();
        let hdc = unsafe { surface.GetDC(BOOL::from(false)) };
        if let Err(err) = hdc {
            return Err(DDApiError::Unexpected(format!("failed to get DC for cursor image. {:?}", err)));
        }
        let hdc = hdc.unwrap();

        let ok = unsafe {
            DrawIconEx(
                hdc,
                cursor_info.ptScreenPos.x - self.state.hotspot_x,
                cursor_info.ptScreenPos.y - self.state.hotspot_y,
                self.state.cursor.unwrap(),
                0, 0, 0, None, DI_NORMAL,
            )
        };

        if !ok.as_bool() {
            unsafe { return Err(DDApiError::Unexpected(format!("failed to draw icon. {:?}", GetLastError()))); }
        }

        let _ = unsafe { surface.ReleaseDC(null()) };
        Ok(())
    }

    fn get_icon_hotspot(cursor: HCURSOR) -> Result<POINT> {
        // get icon information
        let mut icon_info = Default::default();
        let ok = unsafe { GetIconInfo(cursor, &mut icon_info) };
        if !ok.as_bool() {
            unsafe { return Err(DDApiError::Unexpected(format!("failed to get icon info. `{:?}`", GetLastError()))); }
        }

        if !icon_info.hbmMask.is_invalid() {
            unsafe { DeleteObject(icon_info.hbmMask); }
        }
        if !icon_info.hbmColor.is_invalid() {
            unsafe { DeleteObject(icon_info.hbmColor); }
        }

        Ok(POINT { x: icon_info.xHotspot as _, y: icon_info.yHotspot as _ })
    }

    fn reacquire_dup(&mut self) -> Result<()> {
        self.state.reset();

        Self::switch_thread_desktop()?;

        let mut dupl = Self::create_dupl_output(&self.d3d_device, &self.output)?;

        swap(&mut self.dupl, &mut dupl);

        Ok(())
    }

    fn release_locked_frame(&mut self) {
        if self.state.frame_locked {
            unsafe { self.dupl.ReleaseFrame().unwrap() };
            self.state.frame_locked = false;
        }
    }

    fn ensure_cache_frame(&mut self, frame: &mut Texture) -> Result<()> {
        if self.state.frame.is_none() {
            let tex = self.create_texture(frame.desc(), D3D11_USAGE_DEFAULT,
                                          D3D11_BIND_RENDER_TARGET,
                                          Default::default())?;
            self.state.frame = Some(tex);
        }
        Ok(())
    }

    fn ensure_cache_cursor_frame(&mut self, frame: &mut Texture) -> Result<()> {
        if self.state.cursor_frame.is_none() {
            let tex = self.create_texture(frame.desc(), D3D11_USAGE_DEFAULT,
                                          D3D11_BIND_RENDER_TARGET,
                                          D3D11_RESOURCE_MISC_GDI_COMPATIBLE)?;
            self.state.cursor_frame = Some(tex);
        }
        Ok(())
    }

    fn create_texture(&self, tex_desc: TextureDesc, usage: D3D11_USAGE, bind_flags: D3D11_BIND_FLAG,
                      misc_flag: D3D11_RESOURCE_MISC_FLAG) -> Result<Texture> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: tex_desc.width,
            Height: tex_desc.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: tex_desc.format.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: usage,
            BindFlags: bind_flags,
            CPUAccessFlags: Default::default(),
            MiscFlags: misc_flag,
        };
        let result = unsafe { self.d3d_device.CreateTexture2D(&desc, null()) };
        if let Err(e) = result {
            Err(DDApiError::Unexpected(format!("failed to create texture. {:?}", e)))
        } else {
            Ok(Texture::new(result.unwrap()))
        }
    }

    fn switch_thread_desktop() -> Result<()> {
        debug!("trying to switch Thread desktop");
        let desk = unsafe { OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK as _, true, GENERIC_READ) };
        if let Err(err) = desk {
            error!("dint get desktop : {:?}", err);
            return Err(DDApiError::AccessDenied);
        }
        let result = unsafe { SetThreadDesktop(desk.unwrap()) };
        if !result.as_bool() {
            error!("dint switch desktop:");
            return Err(DDApiError::AccessDenied);
        }
        Ok(())
    }
}


/// Settings to configure Desktop duplication api. these can be configured even after initialized.
///
/// currently it only supports option to skip drawing cursor
#[derive(Default)]
pub struct DuplicationApiOptions {
    skip_cursor: bool,
}

// these are state variables for duplication sync stream
#[derive(Default)]
struct DuplicationState {
    frame_locked: bool,
    frame: Option<Texture>,
    cursor_frame: Option<Texture>,

    cursor: Option<HCURSOR>,
    hotspot_x: i32,
    hotspot_y: i32,
}

impl DuplicationState {
    pub fn reset(&mut self) {
        self.frame = None;
        self.cursor_frame = None;
        self.frame_locked = false;
    }
}