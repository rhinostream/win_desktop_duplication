//! Provides wrapper for windows IDXGIOutput and few convenience functions for them.
//!
//! * [Display] - basic wrapper for output with options to change resolution and refresh-rates
//! * [DisplayVSyncStream] - provides async [Stream][futures::Stream] that ticks at every
//!                          display vsync event.
use std::cmp::max;
use std::ffi::CString;
use std::mem::{size_of, swap};
use std::pin::Pin;
use std::ptr::{null, null_mut};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::task::{Context, Poll, Waker};
use std::thread::spawn;

use futures::Stream;
use log::{error, trace};
use windows::core::{PCSTR, Result as WinResult};
use windows::Win32::Graphics::Dxgi::{DXGI_MODE_DESC1, DXGI_OUTPUT_DESC1, IDXGIOutput6};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM};
use windows::Win32::Graphics::Gdi::{CDS_TYPE, ChangeDisplaySettingsExA, DEVMODE_DISPLAY_ORIENTATION, DEVMODEA, DISP_CHANGE_SUCCESSFUL, DM_BITSPERPEL, DM_DISPLAYFREQUENCY, DM_DISPLAYORIENTATION, DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS, EnumDisplaySettingsExA};

use crate::errors::DDApiError;
use crate::utils::convert_u16_to_string;

#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::thread::sleep;
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::runtime::Builder;
    use tokio::time;

    use crate::devices::AdapterFactory;
    use crate::outputs::{DisplayMode, DisplayOrientation};

    #[test]
    fn test_display_names() {
        for adapter in AdapterFactory::new() {
            println!("{}", adapter.name());
            for display in adapter.iter_displays() {
                println!("\t{}", display.name())
            }
        }
    }

    #[test]
    fn test_display_modes() {
        for display in AdapterFactory::new().get_adapter_by_idx(0).unwrap().iter_displays() {
            println!("{}", display.name());
            println!("{:?}", display.get_display_modes().unwrap());
        }
    }

    #[test]
    fn test_display_setting_change() {
        let disp = AdapterFactory::new().get_adapter_by_idx(0).unwrap().get_display_by_idx(0).unwrap();
        let curr_settings = disp.get_current_display_mode().unwrap();

        let mode = DisplayMode {
            width: 1920,
            height: 1080,
            orientation: DisplayOrientation::Rotate180,
            refresh_num: 60,
            refresh_den: 1,
            hdr: false,
        };

        disp.set_display_mode(&mode).unwrap();
        sleep(Duration::from_secs(5));
        disp.set_display_mode(&curr_settings).unwrap();
        println!("{:?}", curr_settings);
    }

    #[test]
    fn test_get_display_mode() {
        let disp = AdapterFactory::new().get_adapter_by_idx(0).unwrap().get_display_by_idx(0).unwrap();
        let curr_settings = disp.get_current_display_mode().unwrap();
        println!("{:?}", curr_settings);
    }


    #[test]
    fn test_display_sync_stream() {
        let disp = AdapterFactory::new().get_adapter_by_idx(0).unwrap().get_display_by_idx(0).unwrap();
        let rt = Builder::new_current_thread().enable_time().build().unwrap();
        let counter = Arc::new(AtomicI32::new(0));
        let counter2 = counter.clone();
        rt.spawn(async move {
            let disp = disp;
            let counter = counter2;

            let mut s = disp.get_vsync_stream();
            while let Some(()) = s.next().await {
                let _ = counter.fetch_add(1, Ordering::Release);
            }
        });

        let counter2 = counter.clone();
        rt.block_on(async move {
            let counter = counter2;
            let total = 5;
            let mut interval = time::interval(Duration::from_secs(1));
            interval.tick().await;
            for _ in 0..total {
                interval.tick().await;
                let read_refresh = counter.load(Ordering::Acquire);
                println!("{}", read_refresh);
                counter.store(0, Ordering::Release);
            }
        });
    }
}


/// Display represents a monitor connected to a single [Adapter][crate::devices::Adapter] (GPU). this instance is
/// used to create a output duplication instance, change display mode and few other options.
///
/// > *setting or detecting hdr display mode is currently not working.*
#[repr(transparent)]
#[derive(Clone)]
pub struct Display(IDXGIOutput6);

impl Display {
    /// create a new instance of Display.
    pub fn new(output: IDXGIOutput6) -> Self {
        Self(output)
    }

    /// returns name of this monitor
    pub fn name(&self) -> String {
        let mut desc: DXGI_OUTPUT_DESC1 = Default::default();
        unsafe { self.0.GetDesc1(&mut desc).unwrap() };
        convert_u16_to_string(&desc.DeviceName)
    }

    /// get supported display modes
    pub fn get_display_modes(&self) -> Result<Vec<DisplayMode>, DDApiError> {
        // SDR display modes.
        let mut out = Vec::new();
        self.fill_modes(DXGI_FORMAT_R8G8B8A8_UNORM, false, &mut out)?;
        self.fill_modes(DXGI_FORMAT_R16G16B16A16_FLOAT, true, &mut out)?;
        Ok(out)
    }

    /// set a specific mode to display
    pub fn set_display_mode(&self, mode: &DisplayMode) -> Result<(), DDApiError> {
        let name = self.name();
        let name = CString::new(name).unwrap();
        let mut display_mode = DEVMODEA {
            ..Default::default()
        };
        display_mode.dmSize = size_of::<DEVMODEA>() as _;
        match mode.orientation {
            DisplayOrientation::NoRotation | DisplayOrientation::Rotate180 => {
                display_mode.dmPelsHeight = mode.height;
                display_mode.dmPelsWidth = mode.width;
            }
            DisplayOrientation::Rotate90 | DisplayOrientation::Rotate270 => {
                display_mode.dmPelsHeight = mode.width;
                display_mode.dmPelsWidth = mode.height;
            }
        }
        display_mode.dmBitsPerPel = if mode.hdr { 64 } else { 32 };
        display_mode.dmDisplayFrequency = mode.refresh_num / mode.refresh_den;
        unsafe {
            display_mode.Anonymous1.Anonymous2.dmDisplayOrientation = mode.orientation.into();
        }

        display_mode.dmFields |= DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY | DM_BITSPERPEL | DM_DISPLAYORIENTATION;


        let resp = unsafe { ChangeDisplaySettingsExA(PCSTR(name.as_ptr() as _), Some(&display_mode), None, CDS_TYPE(0), None) };

        if resp != DISP_CHANGE_SUCCESSFUL {
            Err(DDApiError::BadParam(format!("failed to change display settings. DISP_CHANGE={}", resp.0)))
        } else {
            Ok(())
        }
    }

    /// get current [display mode][DisplayMode] of this monitor.
    pub fn get_current_display_mode(&self) -> Result<DisplayMode, DDApiError> {
        let name = self.name();
        let name = CString::new(name).unwrap();

        let mut mode: DEVMODEA = DEVMODEA {
            dmSize: size_of::<DEVMODEA>() as _,
            dmDriverExtra: 0,
            ..Default::default()
        };
        let success = unsafe { EnumDisplaySettingsExA(PCSTR(name.as_c_str().as_ptr() as _), ENUM_CURRENT_SETTINGS, &mut mode, ENUM_DISPLAY_SETTINGS_FLAGS(0)) };
        if !success.as_bool() {
            Err(DDApiError::Unexpected("Failed to retrieve display settings for output".to_string()))
        } else {
            let mut dm = DisplayMode {
                width: mode.dmPelsWidth,
                height: mode.dmPelsHeight,
                orientation: unsafe { mode.Anonymous1.Anonymous2.dmDisplayOrientation }.into(),
                refresh_num: mode.dmDisplayFrequency,
                refresh_den: 1,
                hdr: mode.dmBitsPerPel != 32,
            };
            if matches!(dm.orientation,DisplayOrientation::Rotate90|DisplayOrientation::Rotate270) {
                dm.height = mode.dmPelsWidth;
                dm.width = mode.dmPelsHeight;
            }
            Ok(dm)
        }
    }

    /// get refresh rate signal stream. check docs of [DisplayVSyncStream] for usage examples.
    pub fn get_vsync_stream(&self) -> DisplayVSyncStream {
        DisplayVSyncStream::new(self.clone())
    }

    /// this is not very async friendly use [get_vsync_stream][Display::get_vsync_stream]
    pub fn wait_for_vsync(&self) -> Result<(), DDApiError> {
        let err = unsafe { self.0.WaitForVBlank() };
        if err.is_err() {
            return Err(DDApiError::Unexpected(format!("DisplaySyncStream received a sync error. Maybe monitor disconnected? {:?}", err)));
        } else {
            Ok(())
        }
    }

    /// returns internal IDXGIOutput6 reference
    pub fn as_raw_ref(&self) -> &IDXGIOutput6 {
        &self.0
    }

    // internal function
    fn fill_modes(&self, format: DXGI_FORMAT, hdr: bool, mode_list: &mut Vec<DisplayMode>) -> Result<(), DDApiError> {
        let mut num_modes: u32 = 0;
        if let Err(e) = unsafe { self.0.GetDisplayModeList1(format, 0, &mut num_modes, None) } {
            return Err(DDApiError::Unexpected(format!("{:?}", e)));
        }

        let mut modes: Vec<DXGI_MODE_DESC1> = Vec::with_capacity(num_modes as _);
        if let Err(e) = unsafe { self.0.GetDisplayModeList1(format, 0, &mut num_modes, Some(modes.as_mut_ptr())) } {
            return Err(DDApiError::Unexpected(format!("{:?}", e)));
        }

        unsafe { modes.set_len(num_modes as _) };
        let reserve = max(0, num_modes as usize - mode_list.capacity() + mode_list.len());
        mode_list.reserve(reserve);
        for mode in modes.iter() {
            mode_list.push(DisplayMode {
                width: mode.Width,
                height: mode.Height,
                refresh_num: mode.RefreshRate.Numerator,
                refresh_den: mode.RefreshRate.Denominator,
                hdr,
                ..Default::default()
            })
        }
        Ok(())
    }
}

unsafe impl Send for Display {}

unsafe impl Sync for Display {}


/// Enum for display orientation
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default)]
pub enum DisplayOrientation {
    /// Landscape mode
    #[default]
    NoRotation,

    /// Portrait mode
    Rotate90,

    /// Landscape (flipped) mode
    Rotate180,

    /// Portrait (flipped) mode
    Rotate270,
}

impl From<DEVMODE_DISPLAY_ORIENTATION> for DisplayOrientation {
    fn from(i: DEVMODE_DISPLAY_ORIENTATION) -> Self {
        match i.0 {
            1 => Self::Rotate90,
            2 => Self::Rotate180,
            3 => Self::Rotate270,
            _ => Self::NoRotation,
        }
    }
}

impl From<DisplayOrientation> for DEVMODE_DISPLAY_ORIENTATION {
    fn from(i: DisplayOrientation) -> Self {
        DEVMODE_DISPLAY_ORIENTATION(match i {
            DisplayOrientation::NoRotation => { 0 }
            DisplayOrientation::Rotate90 => { 1 }
            DisplayOrientation::Rotate180 => { 2 }
            DisplayOrientation::Rotate270 => { 3 }
        })
    }
}


#[repr(C)]
#[derive(Clone, Default, Debug)]
/**
DisplayMode represents one display mode of monitor. It contains resolution, refresh-rate and orientation.
The resolution contains width and height of display for their default orientation.

For example, a 1920 x 1080 monitor will have width 1920 and height 1080 irrespective of the orientation of
the monitor.
 */
pub struct DisplayMode {
    /// width of the given display in pixels
    pub width: u32,
    /// height of the given display in pixels
    pub height: u32,

    /// orientation of the display
    pub orientation: DisplayOrientation,

    /// refresh-rate is usually represented as a fraction. refresh_num is numerator of that fraction
    pub refresh_num: u32,
    /// refresh_den is denominator of refresh-rate fraction.
    pub refresh_den: u32,

    /// this determines if the display is using 8bit or 16bit output mode. (10 bit is
    /// represented as 16 bit in windows)
    pub hdr: bool,
}


/// used to receive sync signal with vsync. this is a async stream.
/// it receives signal after every frame.
///
/// it implements stream api to use in async. The function creates a separate thread to wait
/// for sync events because they are not implemented in async way in the windows os.
///
/// the new thread auto cleans up item goes out of scope.
///
/// # Example:
/// ```
/// while let Some(()) = stream.next().await {
/// // ... do something here
/// // this loop only exits when there is an unexpected error in the stream.
/// }
/// ```
pub struct DisplayVSyncStream {
    sync_rx: Receiver<Result<(), DDApiError>>,
    thread_fn: Option<Box<dyn FnOnce(Waker)>>,
}

unsafe impl Send for DisplayVSyncStream {}

unsafe impl Sync for DisplayVSyncStream {}

impl DisplayVSyncStream {
    /// generates a new sync stream for a given display.
    pub fn new(output: Display) -> Self {
        let (sync_tx, sync_rx) = channel::<Result<(), DDApiError>>();
        // the thread auto stops when this object goes out of scope.
        let thread_fn = move |wake: Waker| {
            spawn(move || {
                let output = output;
                let wake = wake;
                loop {
                    let mut out = Ok(());
                    let res = unsafe { output.0.WaitForVBlank() };
                    if let Err(e) = res {
                        out = Err(DDApiError::Unexpected(format!("{:?}", e)));
                    }
                    wake.wake_by_ref();
                    let err = sync_tx.send(out);
                    if err.is_err() {
                        trace!("exiting display sync wait thread");
                        return;
                    }
                }
            });
        };

        Self {
            sync_rx,
            thread_fn: Some(Box::new(thread_fn)),
        }
    }
}

impl Stream for DisplayVSyncStream {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut out = Poll::Pending;
        let sync_signal = self.sync_rx.try_recv();

        match sync_signal {
            Err(TryRecvError::Empty) => {
                // nosignal is pending. so nothing to do. only once we spawn the thread that
                // waits on display refresh rate and sends signals.
                if self.thread_fn.is_some() {
                    let self_mut = unsafe { self.get_unchecked_mut() };
                    let mut f: Option<Box<dyn FnOnce(Waker)>> = None;
                    swap(&mut self_mut.thread_fn, &mut f);
                    let f = f.unwrap();
                    f(cx.waker().clone())
                }
            }
            Err(TryRecvError::Disconnected) => {
                panic!("DisplayVSyncStream sync thread quit unexpectedly.")
            }
            Ok(Err(e)) => {
                error!("DisplayVSyncStream received a sync error. Maybe monitor disconnected? {:?}", e);
                out = Poll::Ready(None);
            }
            Ok(Ok(())) => {
                out = Poll::Ready(Some(()));
            }
        }
        out
    }
}