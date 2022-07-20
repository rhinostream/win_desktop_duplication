# Windows Desktop Duplication

This is meant to provide a low latency, low level access to desktop frames for use
in applications like Game streaming (e.g., Google Stadia, Microsoft XCloud).

Crate provides convenient wrapper for acquiring gpu
textures
from [windows desktop duplication api](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api).
The crate includes some convenient features that the source api does not provide

## Usage

```rust
#[tokio::main]
fn main() {
    set_process_dpi_awareness();
    co_init();
    let adapter = AdapterFactory::new().get_adapter_by_idx(0).unwrap();
    let output = adapter.get_display_by_idx(0).unwrap();
    let mut dupl = DesktopDuplicationApi::new(adapter, output).unwrap();
    loop {
        // this api send one frame per vsync. the frame also has cursor pre drawn 
        let tex = dupl.acquire_next_vsync_frame().await?;

        // .. use the texture
    }
}
```

## Features

- [x] VSync when providing frames
- [x] Auto draw cursor onto the frame
- [x] Handle desktop switch automatically
- [ ] Convenient functions to copy pixel data in cpu memory
- [ ] Scale and color conversion.
