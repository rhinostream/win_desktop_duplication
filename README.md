# Windows Desktop Duplication

![docs.rs](https://img.shields.io/docsrs/win_desktop_duplication)
![Crates.io](https://img.shields.io/crates/v/win_desktop_duplication)
![Crates.io](https://img.shields.io/crates/l/win_desktop_duplication)

This is meant to provide a low latency, low level access to desktop frames for use
in applications like Game streaming (e.g., Google Stadia, Microsoft XCloud).

Crate provides convenient wrapper for acquiring gpu
textures
from [Windows desktop duplication api](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api).
The crate includes some convenient features that the source api does not provide

## Async Example

> Although this example shows using `TextureReader`, for best performance, you want to use the texture directly to
> encode via one of the hardware based encoders like nvenc or quick-sync.

```rust
use win_desktop_duplication::*;
use win_desktop_duplication::{tex_reader::*, devices::*};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // this is required to be able to use desktop duplication api
    set_process_dpi_awareness();
    co_init();


    // select gpu and output you want to use.
    let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
    let output = adapter.get_display_by_idx(0).unwrap();


    // get output duplication api
    let mut dupl = DesktopDuplicationApi::new(adapter, output).unwrap();

    // Optional: get TextureReader to read GPU textures into CPU.
    let (device, ctx) = dupl.get_device_and_ctx();
    let mut texture_reader = TextureReader::new(device, ctx);


    // create a vector to hold picture data;
    let mut pic_data: Vec<u8> = vec![0; 0];
    loop {
        // this api send one frame per vsync. the frame also has cursor pre drawn
        let tex = dupl.acquire_next_vsync_frame().await;
        if let Ok(tex) = tex {
            let mut tex = tex;
            texture_reader.get_data(&mut pic_data, &mut tex);
            // use pic_data as necessary
        }
    }
}
```

## Sync Example

> Although this example shows using `TextureReader`, for best performance, you want to use the texture directly to
> encode
> via one of the hardware based encoders like nvenc or quick-sync.

```rust
use win_desktop_duplication::*;
use win_desktop_duplication::{tex_reader::*, devices::*};

fn main() {
    // this is required to be able to use desktop duplication api
    set_process_dpi_awareness();
    co_init();

    // select gpu and output you want to use.
    let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
    let output = adapter.get_display_by_idx(0).unwrap();

    // get output duplication api
    let mut dupl = DesktopDuplicationApi::new(adapter, output.clone()).unwrap();

    // Optional: get TextureReader to read GPU textures into CPU.
    let (device, ctx) = dupl.get_device_and_ctx();
    let mut texture_reader = TextureReader::new(device, ctx);


    // create a vector to hold picture data;
    let mut pic_data: Vec<u8> = vec![0; 0];
    loop {
        // this api send one frame per vsync. the frame also has cursor pre drawn
        output.wait_for_vsync().unwrap();
        let tex = dupl.acquire_next_frame_now();

        if let Ok(tex) = tex {
            let mut tex = tex;
            texture_reader.get_data(&mut pic_data, &mut tex);
            // use pic_data as necessary
        }
    }
}
```

## Features

- [x] VSync when providing frames
- [x] Auto draw cursor onto the frame
- [x] Handle desktop switch automatically
- [x] Convenient functions to copy pixel data in cpu memory
- [ ] Scale and color conversion.
