//! # Windows Desktop Duplication
//! Module provides a convenient wrapper for [windows desktop duplication api](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
//! while adding few features to it.
//!
//! For more information on how to use check [DesktopDuplicationApi]

use std::mem::size_of;
use std::ptr::null;
use std::time::Duration;

use futures::StreamExt;
use log::{debug, error, trace, warn};
use tokio::time;
use tokio::time::{Interval, MissedTickBehavior, sleep};
use windows::core::Interface;
use windows::core::Result as WinResult;
use windows::Win32::Foundation::{BOOL, E_ACCESSDENIED, E_INVALIDARG, GENERIC_READ, GetLastError, POINT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_1};
use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_FLAG, D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_FLAG, D3D11_RESOURCE_MISC_FLAG, D3D11_RESOURCE_MISC_GDI_COMPATIBLE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device4, ID3D11DeviceContext4};
use windows::Win32::Graphics::Dxgi::{DXGI_ERROR_ACCESS_DENIED, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL, DXGI_ERROR_SESSION_DISCONNECTED, DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT, IDXGIDevice4, IDXGIOutputDuplication, IDXGIResource, IDXGISurface1};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R10G10B10A2_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::System::StationsAndDesktops::{DESKTOP_ACCESS_FLAGS, OpenInputDesktop, SetThreadDesktop};
use windows::Win32::System::StationsAndDesktops::DF_ALLOWOTHERACCOUNTHOOK;
use windows::Win32::UI::WindowsAndMessaging::{CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HCURSOR};

use crate::devices::Adapter;
use crate::errors::DDApiError;
use crate::outputs::{Display, DisplayVSyncStream};
use crate::Result;
use crate::texture::{Texture, TextureDesc};

#[cfg(test)]
mod test {
    use std::sync::Once;
    use std::time::{Duration, Instant};

    use futures::FutureExt;
    use futures::select;
    use log::LevelFilter::Debug;
    use tokio::time::interval;

    use crate::{DDApiError, DuplicationApiOptions};
    use crate::devices::AdapterFactory;
    use crate::duplication::DesktopDuplicationApi;
    use crate::outputs::DisplayMode;
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

        let rt = tokio::runtime::Builder::new_current_thread()
            .thread_name("graphics_thread".to_owned()).enable_time().build().unwrap();

        rt.block_on(async {
            set_process_dpi_awareness();
            co_init();

            let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
            let output = adapter.get_display_by_idx(0).unwrap();
            let mut dupl = DesktopDuplicationApi::new(adapter, output.clone()).unwrap();
            let curr_mode = output.get_current_display_mode().unwrap();
            dupl.configure(DuplicationApiOptions {
                skip_cursor: true
            });
            let new_mode = DisplayMode {
                width: 2560,
                height: 1440,
                orientation: Default::default(),
                refresh_num: curr_mode.refresh_num,
                refresh_den: curr_mode.refresh_den,
                hdr: false,
            };

            let mut counter = 0;
            let mut secs = 0;
            let mut interval = interval(Duration::from_secs(1));
            loop {
                select! {
                    tex = dupl.acquire_next_vsync_frame().fuse()=>{
                        match &tex {
                            Err(DDApiError::AccessDenied)| Err(DDApiError::AccessLost)  =>  {
                                println!("error: {:?}",tex.err())
                            }
                            Err(e)=>{
                                println!("error: {:?}",e)
                            }
                            Ok(_)=>{
                                counter += 1;
                            }
                        }
                    },
                    _ = interval.tick().fuse() => {
                        println!("fps: {}",counter);
                        counter = 0;
                        secs+=1;
                        if secs == 5 {
                            println!("5 secs");
                        } else if secs ==10 {
                            break;
                        }
                    }
                }
                ;
            };
        });
    }

    #[test]
    fn test_duplication_blocking() {
        initialize();

        set_process_dpi_awareness();
        co_init();

        let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
        let output = adapter.get_display_by_idx(0).unwrap();
        let mut dupl = DesktopDuplicationApi::new(adapter, output.clone()).unwrap();
        let curr_mode = output.get_current_display_mode().unwrap();
        let new_mode = DisplayMode {
            width: 1920,
            height: 1080,
            orientation: Default::default(),
            refresh_num: curr_mode.refresh_num,
            refresh_den: curr_mode.refresh_den,
            hdr: false,
        };

        let mut counter = 0;
        let mut secs = 0;
        let instant = Instant::now();
        loop {
            let _ = output.wait_for_vsync();
            let tex = dupl.acquire_next_frame_now();
            if let Err(e) = tex {
                println!("error: {:?}", e)
            } else {
                counter += 1;
            };
            if secs != instant.elapsed().as_secs() {
                println!("fps: {}", counter);
                counter = 0;
                secs += 1;
                if secs == 1 {
                    println!("1 secs");
                    output.set_display_mode(&new_mode).unwrap();
                } else if secs == 5 {
                    output.set_display_mode(&curr_mode).unwrap();
                    break;
                }
            }
        }
    }
}


/// Provides asynchronous, synchronous api for windows desktop duplication with additional features such as
/// cursor pre-drawn, frame rate synced to desktop refresh rate.
///
/// please note that this api works best if created and called from a single thread.
/// Ideal scenario would be to maintain a "Graphics thread" in your application where all the
/// Graphics related tasks are performed asynchronously.
///
/// acquire_next_frame_now especially should be called from only one thread because it only works if the
/// thread calling it is marked as desktop thread. Although the application attempts to set any
/// thread you call this method from as desktop thread, it's not usually a good idea.
///
/// # Async Example
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
///
/// ```
///
/// # Sync Example
/// ```
///     use win_desktop_duplication::DesktopDuplicationApi;
///     // ....
///     {
///         let mut duplication = DesktopDuplicationApi::new(adapter, output)?;
///         loop {
///             output.wait_for_vsync();
///             let tex = duplication.acquire_next_frame_now()?;
///             // use the texture to encode video
///             //...
///         }
///     }
/// ```
pub struct DesktopDuplicationApi {
    d3d_device: ID3D11Device4,
    d3d_ctx: ID3D11DeviceContext4,
    output: Display,
    vsync_stream: DisplayVSyncStream,
    dupl: Option<IDXGIOutputDuplication>,

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
    ///
    /// this method fails with
    /// * [DDApiError::Unsupported] when the application's dpi awareness is not set. use [crate::set_process_dpi_awareness]
    pub fn new(adapter: Adapter, output: Display) -> Result<Self> {
        let (device, ctx) = Self::create_device(&adapter)?;
        Self::new_with(device, ctx, output)
    }

    /// Creates a new instance of the api from provided device and context.
    pub fn new_with(d3d_device: ID3D11Device4, ctx: ID3D11DeviceContext4, output: Display) -> Result<Self> {
        let dupl = Self::create_dupl_output(&d3d_device, &output)?;
        Ok(Self {
            d3d_device,
            d3d_ctx: ctx,
            vsync_stream: output.get_vsync_stream(),
            output,
            dupl: Some(dupl),
            options: Default::default(),
            state: Default::default(),
        })
    }

    /// Acquire next frame from the desktop duplication api after waiting for vsync refresh.
    /// this helps application acquire frames with same rate as display's native refresh-rate.
    ///
    /// this is an asynchronous method. check example in the [doc][DesktopDuplicationApi] for more details
    ///
    /// This method fails with following errors
    ///
    /// ## Recoverable errors
    /// these can be recovered by just calling the function again after this error.
    /// * [DDApiError::AccessLost] - when desktop mode switch happens (resolution change) or desktop
    /// changes. (going to lock screen etc).
    /// * [DDApiError::AccessDenied] - when windows opens a secure environment, this application
    /// will be denied access.
    ///
    /// ## Non-recoverable errors
    /// * [DDApiError::Unexpected] - this type of error cant be recovered from. the application should
    /// drop the struct and re create a new instance.
    pub async fn acquire_next_vsync_frame(&mut self) -> Result<Texture> {
        // wait for vsync
        if (self.vsync_stream.next().await).is_some_and(|r| r.is_err()) {
            return Err(DDApiError::Unexpected("DisplayVSyncStream failed unexpectedly".to_owned()));
        }
        // acquire next_frame
        let res = self.acquire_next_frame_now();
        if res.is_err() {
            trace!("something went wrong with acquiring next frame. probably desktop duplication \
            instance failed. waiting for 200ms");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        res
    }

    fn create_device(adapter: &Adapter) -> Result<(ID3D11Device4, ID3D11DeviceContext4)> {
        let feature_levels = [D3D_FEATURE_LEVEL_11_1];
        let mut feature_level: D3D_FEATURE_LEVEL = Default::default();
        let mut d3d_device = None;
        let mut d3d_ctx = None;

        let resp = unsafe {
            D3D11CreateDevice(adapter.as_raw_ref(), D3D_DRIVER_TYPE_UNKNOWN,
                              None, D3D11_CREATE_DEVICE_FLAG(0),
                              Some(&feature_levels), D3D11_SDK_VERSION,
                              Some(&mut d3d_device), Some(&mut feature_level),
                              Some(&mut d3d_ctx))
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
        let dupl: WinResult<IDXGIOutputDuplication> = unsafe { output.as_raw_ref().DuplicateOutput1(&device, 0, &supported_formats) };

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

    /// unlike [acquire_next_vsync_frame][Self::acquire_next_vsync_frame], this is a blocking call and immediately returns the texture
    /// without waiting for vsync.
    ///
    /// the method handles any switches in desktop automatically.
    ///
    /// this fails with following results:
    ///
    /// ## Recoverable errors
    /// these can be recovered by just calling the function again after this error.
    /// * [DDApiError::AccessLost] - when desktop mode switch happens (resolution change) or desktop
    /// changes. (going to lock screen etc).
    /// * [DDApiError::AccessDenied] - when windows opens a secure environment, this application
    /// will be denied access.
    ///
    /// ## Non-recoverable errors
    /// * [DDApiError::Unexpected] - this type of error cant be recovered from. the application should
    /// drop the struct and re create a new instance.
    pub fn acquire_next_frame_now(&mut self) -> Result<Texture> {
        let mut frame_info = Default::default();

        if self.dupl.is_none() {
            self.reacquire_dup()?;
        }
        let dupl = self.dupl.as_ref().unwrap();
        let status = unsafe { dupl.AcquireNextFrame(0, &mut frame_info, &mut self.state.last_resource) };
        if let Err(e) = status {
            match e.code() {
                DXGI_ERROR_ACCESS_LOST => {
                    warn!("display access lost. maybe desktop mode switch?, {:?}",e);
                    self.reacquire_dup()?;
                    return Err(DDApiError::AccessLost);
                }
                DXGI_ERROR_ACCESS_DENIED => {
                    warn!("display access is denied. Maybe running in a secure environment?");
                    self.reacquire_dup()?;
                    return Err(DDApiError::AccessDenied);
                }
                DXGI_ERROR_INVALID_CALL => {
                    warn!("dxgi_error_invalid_call. maybe forgot to ReleaseFrame()?");
                    self.reacquire_dup()?;
                    return Err(DDApiError::AccessLost);
                }
                DXGI_ERROR_WAIT_TIMEOUT => {
                    trace!("no new frame is available");
                }
                _ => {
                    return Err(DDApiError::Unexpected(format!("acquire frame failed {:?}", e)));
                }
            }
        }


        if let Some(resource) = self.state.last_resource.as_ref() {
            debug!("got fresh resource. accumulated {} frames",frame_info.AccumulatedFrames);
            self.state.frame_locked = true;
            let new_frame = Texture::new(resource.cast().unwrap());
            self.ensure_cache_frame(&new_frame).inspect_err(|_| {
                self.release_locked_frame();
            })?;
            unsafe { self.d3d_ctx.CopyResource(self.state.frame.as_ref().unwrap().as_raw_ref(), new_frame.as_raw_ref()); }
            self.release_locked_frame();
        } else {
            debug!("no fresh resource. accumulated {} frames",frame_info.AccumulatedFrames);
        }
        if self.state.frame.is_none() {
            return Err(DDApiError::AccessLost);
        }

        let cache_frame = self.state.frame.clone().unwrap();

        if !self.options.skip_cursor {
            self.ensure_cache_cursor_frame(&cache_frame)?;
            let cache_cursor_frame = self.state.cursor_frame.clone().unwrap();

            unsafe {
                self.d3d_ctx.CopyResource(
                    cache_cursor_frame.as_raw_ref(),
                    cache_frame.as_raw_ref())
            }

            self.draw_cursor(&cache_cursor_frame)?;
            Ok(cache_cursor_frame)
        } else {
            Ok(cache_frame)
        }
    }


    /// this method is used to retrieve device and context used in this api. These can be used
    /// to build directx color conversion and image scale.
    pub fn get_device_and_ctx(&self) -> (ID3D11Device4, ID3D11DeviceContext4) {
        return (self.d3d_device.clone(), self.d3d_ctx.clone());
    }

    /// configure duplication manager with given options.
    pub fn configure(&mut self, opt: DuplicationApiOptions) {
        self.options = opt;
    }

    fn draw_cursor(&mut self, tex: &Texture) -> Result<()> {
        trace!("drawing cursor");
        let mut cursor_info = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        let cursor_present = unsafe { GetCursorInfo(&mut cursor_info as *mut CURSORINFO) };

        // if cursor is not present, return raw frame.
        if cursor_present.is_err()
            || (cursor_info.flags.0 & CURSOR_SHOWING.0 != CURSOR_SHOWING.0)
        {
            debug!("cursor is absent so not drawing anything");
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

        let result = unsafe {
            DrawIconEx(
                hdc,
                cursor_info.ptScreenPos.x - self.state.hotspot_x,
                cursor_info.ptScreenPos.y - self.state.hotspot_y,
                self.state.cursor.unwrap(),
                0, 0, 0, None, DI_NORMAL,
            )
        };

        if result.is_err() {
            unsafe { return Err(DDApiError::Unexpected(format!("failed to draw icon. {:?}", GetLastError()))); }
        }

        let _ = unsafe { surface.ReleaseDC(None) };
        Ok(())
    }

    fn get_icon_hotspot(cursor: HCURSOR) -> Result<POINT> {
        // get icon information
        let mut icon_info = Default::default();
        let result = unsafe { GetIconInfo(cursor, &mut icon_info) };
        if result.is_err() {
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
        self.dupl = None;

        let dupl = Self::create_dupl_output(&self.d3d_device, &self.output);
        if dupl.is_err() {
            let _ = Self::switch_thread_desktop();
        }
        let dupl = dupl?;
        debug!("successfully acquired new duplication instance");
        self.dupl = Some(dupl);
        Ok(())
    }

    fn release_locked_frame(&mut self) {
        if self.state.last_resource.is_some() {
            self.state.last_resource = None;
        }
        if self.dupl.is_some() {
            if self.state.frame_locked {
                let _ = unsafe { self.dupl.as_ref().unwrap().ReleaseFrame() };
                self.state.frame_locked = false;
            }
        }
    }

    fn ensure_cache_frame(&mut self, frame: &Texture) -> Result<()> {
        if self.state.frame.is_none() {
            let tex = self.create_texture(frame.desc(), D3D11_USAGE_DEFAULT,
                                          D3D11_BIND_RENDER_TARGET,
                                          Default::default())?;
            self.state.frame = Some(tex);
        }
        Ok(())
    }

    fn ensure_cache_cursor_frame(&mut self, frame: &Texture) -> Result<()> {
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
            BindFlags: bind_flags.0 as u32,
            CPUAccessFlags: Default::default(),
            MiscFlags: misc_flag.0 as u32,
        };
        let mut tex = None;
        let result = unsafe { self.d3d_device.CreateTexture2D(&desc, None, Some(&mut tex)) };
        if let Err(e) = result {
            Err(DDApiError::Unexpected(format!("failed to create texture. {:?}", e)))
        } else {
            Ok(Texture::new(tex.unwrap()))
        }
    }

    fn switch_thread_desktop() -> Result<()> {
        debug!("trying to switch Thread desktop");
        let desk = unsafe { OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK as _, true, DESKTOP_ACCESS_FLAGS(GENERIC_READ.0)) };
        if let Err(err) = desk {
            error!("dint get desktop : {:?}", err);
            return Err(DDApiError::AccessDenied);
        }
        let result = unsafe { SetThreadDesktop(desk.unwrap()) };
        if result.is_err() {
            error!("dint switch desktop: {:?}",unsafe{GetLastError().to_hresult()});
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
    pub skip_cursor: bool,
}

// these are state variables for duplication sync stream
#[derive(Default)]
struct DuplicationState {
    frame_locked: bool,
    last_resource: Option<IDXGIResource>,

    frame: Option<Texture>,
    cursor_frame: Option<Texture>,

    cursor: Option<HCURSOR>,
    hotspot_x: i32,
    hotspot_y: i32,
}

impl DuplicationState {
    pub fn reset(&mut self) {
        self.frame = None;
        self.last_resource = None;
        self.cursor_frame = None;
        self.frame_locked = false;
    }
}