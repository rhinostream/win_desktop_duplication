use std::thread;
use std::time::Duration;
use windows::core::imp::HANDLE;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::Win32::Graphics::Dxgi::IDXGIOutputDuplication;

struct InternalDesktopDuplStream {
    d3d_device: ID3D11Device,
    d3d_ctx: ID3D11DeviceContext,
    dupl: IDXGIOutputDuplication,
}

impl InternalDesktopDuplStream {
    pub fn new_with(d3d_device: ID3D11Device, d3d_ctx: ID3D11DeviceContext, dupl: IDXGIOutputDuplication) -> crate::Result<Self> {
        Ok(Self {
            d3d_ctx,
            d3d_device,
            dupl,
        })
    }

    pub fn start(self) -> (std::sync::mpsc::Receiver<windows::core::Result<HANDLE>>, std::sync::mpsc::SyncSender<Duration>) {
        let (tx_frames, rx_frames) = std::sync::mpsc::sync_channel(0);
        let (tx_ready, rx_ready) = std::sync::mpsc::sync_channel(0);
        thread::spawn(move || {
            self.run_loop(tx_frames, rx_ready)
        });
        (rx_frames, tx_ready)
    }
    fn run_loop(self, tx: std::sync::mpsc::SyncSender<windows::core::Result<HANDLE>>, rx: std::sync::mpsc::Receiver<Duration>) {

        // TODO:

    }
}


pub(crate) struct DesktopDuplicationStream {
    d3d_device: ID3D11Device,
    d3d_ctx: ID3D11DeviceContext,
    dupl: IDXGIOutputDuplication,

    rx: std::sync::mpsc::Receiver<windows::core::Result<HANDLE>>,
    tx: std::sync::mpsc::SyncSender<Duration>,
}

impl DesktopDuplicationStream {
    pub fn new(d3d_device: ID3D11Device, d3d_ctx: ID3D11DeviceContext, dupl:IDXGIOutputDuplication) -> crate::Result<Self> {
        let st= InternalDesktopDuplStream::new_with(d3d_device.clone(), d3d_ctx.clone(), dupl.clone())?;
        let (rx, tx) = st.start();
        Ok(Self{
            d3d_device,
            d3d_ctx,
            dupl,
            rx,
            tx
        })

    }

    pub async fn get_next_frame(&mut self, timeout: Duration) -> crate::Result<u32> {
        self.tx.send(timeout);

        Ok(1)
    }
}