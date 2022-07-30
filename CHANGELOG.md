# Change log

## v 0.10.4

1. removed `mut` requirement on `Texture::desc`.
2. added support for reading `NV12`,`AYUV` formats in `TextureReader`
3. fixed major bugs with 0.10.3

## v 0.10.2

2. Updated `ColorFormat` enum and added docs for each format. 
2. added `as_raw_ref` for all wrapper structs.
3. Fixed bugs in `0.10.1` and dropped that version

## v 0.10.0

1. Added orientation support for `Display`. their size will now accurately 
   represent size of texture that desktop duplication api returns.
2. `DisplayOrientation` enum is used to represent various mode.
3. Updated `windows` crate version

## v 0.9.0

1. Added synchronous api for `DesktopDuplicationApi`
2. Added ability for `DesktopDuplicationApi` struct to be configured while running.
3. Added ability for `DesktopDuplicationApi` to share device and context. They can be used with systems like directx and
   nvenc
4. Add `TextureReader` which can be used to read GPU textures into vectors.
5. Added more documentation.
6. Fixed multiple bugs