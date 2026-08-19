//! GPU cursor compositor.
//!
//! Composites the DXGI pointer shape onto the desktop frame with a D3D11 pixel
//! shader. This replaces the old GDI path (`IDXGISurface1::GetDC` + `DrawIconEx`),
//! which required `D3D11_RESOURCE_MISC_GDI_COMPATIBLE` and therefore only worked
//! for `DXGI_FORMAT_B8G8R8A8_UNORM`. The shader path works for all duplication
//! formats (8-bit `B8G8R8A8_UNORM`, 10-bit `R10G10B10A2_UNORM`, 16-bit-float
//! `R16G16B16A16_FLOAT`) because it samples the clean frame as a shader resource
//! and writes the blended result to a separate render target. Sampling the
//! destination is what makes monochrome / masked-color XOR inversion possible
//! without GDI.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::mem::size_of;

use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, D3D_SRV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11Device4, ID3D11DeviceContext4, ID3D11RenderTargetView,
    ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_FLOAT32_MAX, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SAMPLER_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SUBRESOURCE_DATA,
    D3D11_TEX2D_RTV, D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, D3D11_COMPARISON_NEVER,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use crate::duplication::{CursorKind, CursorShape};
use crate::errors::DDApiError;
use crate::shader::{PixelShader, VertexShader};
use crate::texture::{ColorFormat, Texture, TextureDesc};
use crate::Result;

// `compile_shader!` calls `D3DCompile2` (Windows-only) at build time; gate it out
// of doc builds (docs.rs runs on a non-Windows host).
#[cfg(not(doc))]
use shader_macro::compile_shader;

/// Constant buffer payload. Must match the `CursorCB` cbuffer in
/// `shaders/cursor_ps.hlsl`. 40 bytes of data; the constant buffer itself is
/// sized up to 48 (a multiple of 16, as D3D11 requires).
#[repr(C)]
#[derive(Clone, Copy)]
struct CursorCB {
    frame_size: [u32; 2],
    cursor_rect: [i32; 2],
    cursor_size: [u32; 2],
    kind: u32,
    visible: u32,
    frame_format: u32,
    pad0: u32,
}

/// Cheap identity for a cursor shape, used to avoid re-uploading the cursor
/// texture unless the shape actually changed.
type CursorSignature = (u32, u32, u32, u32, usize, u64);

fn signature_of(shape: &CursorShape) -> CursorSignature {
    let mut h = DefaultHasher::new();
    shape.buffer.hash(&mut h);
    (
        kind_u32(&shape.kind),
        shape.width,
        shape.height,
        shape.pitch,
        shape.buffer.len(),
        h.finish(),
    )
}

fn kind_u32(k: &CursorKind) -> u32 {
    match k {
        CursorKind::SingleBit => 1,
        CursorKind::ARGB => 2,
        CursorKind::Masked => 4,
    }
}

fn frame_format_of(f: ColorFormat) -> u32 {
    match f {
        ColorFormat::ABGR8UNorm => 0,   // B8G8R8A8_UNORM
        ColorFormat::ARGB10UNorm => 1,  // R10G10B10A2_UNORM
        ColorFormat::ARGB16Float => 2,  // R16G16B16A16_FLOAT
        _ => 0,
    }
}

/// Holds the cached D3D11 state needed to composite the cursor: the shaders,
/// sampler, constant buffer, and (re)created SRV/RTV/cursor-texture views.
pub struct CursorRenderer {
    device: ID3D11Device4,
    ctx: ID3D11DeviceContext4,
    vs: VertexShader,
    ps: PixelShader,
    sampler: ID3D11SamplerState,
    cb: ID3D11Buffer,

    frame_srv: Option<ID3D11ShaderResourceView>,
    rtv: Option<ID3D11RenderTargetView>,
    last_desc: Option<TextureDesc>,

    cursor_tex: Option<ID3D11Texture2D>,
    cursor_srv: Option<ID3D11ShaderResourceView>,
    cursor_signature: Option<CursorSignature>,

    // Cached metadata of the currently uploaded cursor. `last_cursor_shape` (the
    // input to `update_cursor_shape`) is a transient "a new shape arrived this
    // frame" signal and is `None` on non-change frames, so the renderer must
    // remember the cursor it last uploaded to keep drawing it across frames with
    // no shape change.
    cursor_kind: u32,
    cursor_hotspot: [i32; 2],
    cursor_visual_size: [u32; 2],

    // Stored cursor position/visibility. `frame_info.PointerPosition` is only
    // valid when `frame_info.LastMouseUpdateTime > 0`; on frames where it is 0 the
    // pointer data is garbage, so the producer calls `update_cursor_info` only on
    // valid frames and `composite` uses whatever was last stored here. This is a
    // cursor-only store, separate from `last_frame_info` (which the consumer
    // replaces each frame).
    cursor_visible: bool,
    cursor_pos: [i32; 2],
}

#[cfg(not(doc))]
fn make_vs(device: &ID3D11Device4) -> Result<VertexShader> {
    VertexShader::new(
        &compile_shader! {
            src_file: "src\\shaders\\simple_vs.hlsl",
            entry_point: "main",
            target: "vs_5_0"
        },
        device,
    )
}

#[cfg(not(doc))]
fn make_ps(device: &ID3D11Device4) -> Result<PixelShader> {
    PixelShader::new(
        &compile_shader! {
            src_file: "src\\shaders\\cursor_ps.hlsl",
            entry_point: "main",
            target: "ps_5_0"
        },
        device,
    )
}

#[cfg(doc)]
fn make_vs(_device: &ID3D11Device4) -> Result<VertexShader> {
    unimplemented!()
}
#[cfg(doc)]
fn make_ps(_device: &ID3D11Device4) -> Result<PixelShader> {
    unimplemented!()
}

impl CursorRenderer {
    /// Compile shaders and create the sampler + constant buffer. SRV/RTV/cursor
    /// views are created lazily on the first `composite` (when the frame desc and
    /// cursor shape are known).
    pub fn new(device: ID3D11Device4, ctx: ID3D11DeviceContext4) -> Result<Self> {
        let vs = make_vs(&device)?;
        let ps = make_ps(&device)?;

        // Linear, clamp sampler (matches dxfilter's create_tex_sampler).
        let sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 0,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [255.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        let mut sampler = None;
        if let Err(e) = unsafe { device.CreateSamplerState(&sampler_desc, Some(&mut sampler)) } {
            return Err(DDApiError::Unexpected(format!(
                "failed to create cursor sampler. {:?}",
                e
            )));
        }

        // Dynamic constant buffer, written per-frame via Map(WRITE_DISCARD).
        let cb_byte_width = ((size_of::<CursorCB>() + 15) & !15) as u32;
        let cb_desc = D3D11_BUFFER_DESC {
            ByteWidth: cb_byte_width,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: Default::default(),
            StructureByteStride: 0,
        };
        let mut cb = None;
        if let Err(e) = unsafe { device.CreateBuffer(&cb_desc, None, Some(&mut cb)) } {
            return Err(DDApiError::Unexpected(format!(
                "failed to create cursor constant buffer. {:?}",
                e
            )));
        }

        Ok(Self {
            device,
            ctx,
            vs,
            ps,
            sampler: sampler.unwrap(),
            cb: cb.unwrap(),
            frame_srv: None,
            rtv: None,
            last_desc: None,
            cursor_tex: None,
            cursor_srv: None,
            cursor_signature: None,
            cursor_kind: 2,
            cursor_hotspot: [0, 0],
            cursor_visual_size: [1, 1],
            cursor_visible: false,
            cursor_pos: [0, 0],
        })
    }

    /// Recreate the frame SRV / output RTV when the frame desc changes. Cursor
    /// shape/position are owned by this renderer and updated via
    /// `update_cursor_shape` / `update_cursor_info`.
    fn ensure_state(&mut self, frame_tex: &Texture, shared_tex: &Texture) -> Result<()> {
        let desc = frame_tex.desc();
        if self.last_desc != Some(desc) {
            self.frame_srv = Some(create_srv(
                &self.device,
                frame_tex,
                desc.format.into(),
            )?);
            self.rtv = Some(create_rtv(
                &self.device,
                shared_tex,
                desc.format.into(),
            )?);
            self.last_desc = Some(desc);
        }
        Ok(())
    }

    /// Store the current cursor position/visibility. Call this only when
    /// `frame_info.LastMouseUpdateTime > 0` — the pointer position is invalid
    /// otherwise. The stored values are used by `composite` on every frame,
    /// including ones where no new mouse update arrived.
    pub fn update_cursor_info(&mut self, visible: bool, pos_x: i32, pos_y: i32) {
        self.cursor_visible = visible;
        self.cursor_pos = [pos_x, pos_y];
    }

    /// Upload a new cursor shape and cache its metadata (kind, hotspot, visual
    /// size). Call this when a new pointer shape is available
    /// (`PointerShapeBufferSize != 0`). Re-uploads only when the shape actually
    /// changed; the cached metadata is what `composite` uses on every frame,
    /// including non-change frames.
    pub fn update_cursor_shape(&mut self, shape: &CursorShape) -> Result<()> {
        let sig = signature_of(shape);
        if self.cursor_signature != Some(sig) {
            self.cursor_kind = kind_u32(&shape.kind);
            self.cursor_hotspot = [shape.hotspot.cx, shape.hotspot.cy];
            let (w, h) = cursor_visual_size(shape);
            self.cursor_visual_size = [w, h];
            self.upload_cursor(shape)?;
            self.cursor_signature = Some(sig);
        }
        Ok(())
    }

    /// CPU-preprocess the cursor shape into a tightly-packed `B8G8R8A8_UNORM`
    /// buffer and upload it to a fresh `D3D11_BIND_SHADER_RESOURCE` texture.
    fn upload_cursor(&mut self, shape: &CursorShape) -> Result<()> {
        let (data, width, height) = preprocess(shape);

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: Default::default(),
            MiscFlags: Default::default(),
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: data.as_ptr() as _,
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        };
        let mut tex = None;
        if let Err(e) =
            unsafe { self.device.CreateTexture2D(&desc, Some(&init), Some(&mut tex)) }
        {
            return Err(DDApiError::Unexpected(format!(
                "failed to create cursor texture. {:?}",
                e
            )));
        }
        let tex = tex.unwrap();
        let srv = create_srv_raw(&self.device, &tex, DXGI_FORMAT_B8G8R8A8_UNORM)?;

        self.cursor_tex = Some(tex);
        self.cursor_srv = Some(srv);
        Ok(())
    }

    /// Composite the stored cursor onto `shared_tex`, sampling `frame_tex` as the
    /// destination. The caller must hold the keyed mutex on `shared_tex` for the
    /// duration of this call (the GPU draw is queued before the subsequent
    /// `ReleaseSync`, exactly like the old `CopyResource` path).
    ///
    /// Uses the cursor position/visibility stored by `update_cursor_info` and the
    /// shape metadata cached by `update_cursor_shape`. If the cursor is not
    /// visible or no shape has been uploaded yet, the shader runs in passthrough
    /// mode (copies `frame_tex` into `shared_tex`), keeping the GPU pipeline
    /// uniform.
    pub fn composite(&mut self, frame_tex: &Texture, shared_tex: &Texture) -> Result<()> {
        self.ensure_state(frame_tex, shared_tex)?;

        let desc = frame_tex.desc();
        let have_cursor = self.cursor_srv.is_some();
        let show = self.cursor_visible && have_cursor;

        let cb = CursorCB {
            frame_size: [desc.width, desc.height],
            cursor_rect: [
                self.cursor_pos[0] - self.cursor_hotspot[0],
                self.cursor_pos[1] - self.cursor_hotspot[1],
            ],
            cursor_size: self.cursor_visual_size,
            kind: self.cursor_kind,
            visible: show as u32,
            frame_format: frame_format_of(desc.format),
            pad0: 0,
        };
        self.update_cb(&cb)?;

        let vp = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: desc.width as f32,
            Height: desc.height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };

        unsafe {
            let ctx = &self.ctx;
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.VSSetShader(self.vs.as_raw_ref(), Some(&[]));
            ctx.PSSetShader(self.ps.as_raw_ref(), Some(&[]));
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            let srvs: [Option<ID3D11ShaderResourceView>; 2] =
                [self.frame_srv.clone(), self.cursor_srv.clone()];
            ctx.PSSetShaderResources(0, Some(&srvs));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            ctx.RSSetViewports(Some(&[vp]));
            ctx.OMSetRenderTargets(Some(&[self.rtv.clone()]), None);
            ctx.Draw(4, 0);

            // Unbind to avoid a "resource still bound" hazard on the next
            // CopyResource / view creation that touches these textures.
            let null_srvs: [Option<ID3D11ShaderResourceView>; 2] = [None, None];
            ctx.PSSetShaderResources(0, Some(&null_srvs));
            ctx.OMSetRenderTargets(None, None);
        }

        Ok(())
    }

    fn update_cb(&self, cb: &CursorCB) -> Result<()> {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            if let Err(e) =
                self.ctx
                    .Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
            {
                return Err(DDApiError::Unexpected(format!(
                    "failed to map cursor constant buffer. {:?}",
                    e
                )));
            }
            std::ptr::copy_nonoverlapping(
                cb as *const CursorCB as *const u8,
                mapped.pData as *mut u8,
                size_of::<CursorCB>(),
            );
            self.ctx.Unmap(&self.cb, 0);
        }
        Ok(())
    }
}

/// The cursor's visible dimensions in pixels. For monochrome cursors DXGI
/// reports `height` as the total (AND + XOR masks), so the visible height is
/// half of that; for color/masked cursors `height` is already the visible height.
fn cursor_visual_size(shape: &CursorShape) -> (u32, u32) {
    let h = if matches!(shape.kind, CursorKind::SingleBit) {
        shape.height / 2
    } else {
        shape.height
    };
    (shape.width, h)
}

/// Build the cursor bitmap as a tightly-packed `[B, G, R, A]` buffer (pitch =
/// width*4) from the raw DXGI pointer shape, returning `(data, width, height)`
/// where `height` is the *visible* height.
fn preprocess(shape: &CursorShape) -> (Vec<u8>, u32, u32) {
    let (w, h) = cursor_visual_size(shape);
    let data = match shape.kind {
        CursorKind::SingleBit => preprocess_mono(shape),
        CursorKind::ARGB | CursorKind::Masked => preprocess_bgra(shape),
    };
    (data, w, h)
}

/// Monochrome cursor: the DXGI shape buffer holds an AND mask followed by an
/// XOR mask of equal size. DXGI reports `height` as the *total* (AND + XOR) so
/// the logical cursor height is `height / 2` (mirrors `get_image` in
/// rhinostream-rs, which does `shape.height /= 2` for SingleBit). Each mask is
/// 1 bit per pixel, `pitch = ceil(width/8)`. Encoded into BGRA so the pixel
/// shader can branch with a single texture:
///   (and, xor) -> A
///   (1, 0) transparent   -> A=0x00
///   (0, 0) black opaque  -> A=0xFF, RGB=0x00
///   (0, 1) white opaque  -> A=0xFF, RGB=0xFF
///   (1, 1) invert dst     -> A=0x80, RGB=0x00
fn preprocess_mono(shape: &CursorShape) -> Vec<u8> {
    let w = shape.width as usize;
    let h = (shape.height / 2) as usize;
    let pitch = shape.pitch as usize;
    let and_off = 0;
    let xor_off = h * pitch;

    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let and_byte = shape.buffer[and_off + y * pitch + (x >> 3)];
            let xor_byte = shape.buffer[xor_off + y * pitch + (x >> 3)];
            let and_bit = (and_byte >> (7 - (x & 7))) & 1;
            let xor_bit = (xor_byte >> (7 - (x & 7))) & 1;
            let i = (y * w + x) * 4;
            let px = match (and_bit, xor_bit) {
                (1, 0) => [0x00, 0x00, 0x00, 0x00], // transparent
                (0, 0) => [0x00, 0x00, 0x00, 0xFF], // black
                (0, 1) => [0xFF, 0xFF, 0xFF, 0xFF], // white
                (1, 1) => [0x00, 0x00, 0x00, 0x80], // invert dst
                _ => [0x00, 0x00, 0x00, 0x00],
            };
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    out
}

/// Color / masked-color cursor: 32-bit per pixel, already in BGRA memory order
/// (DXGI ARGB DIB layout maps to `DXGI_FORMAT_B8G8R8A8_UNORM`'s [B, G, R, A]).
/// Copy row-by-row to drop any pitch padding. For masked color the alpha channel
/// is the 0/0xFF mask; it is passed through verbatim and interpreted by the shader.
fn preprocess_bgra(shape: &CursorShape) -> Vec<u8> {
    let w = shape.width as usize;
    let h = shape.height as usize;
    let pitch = shape.pitch as usize;
    let row_bytes = w * 4;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src = &shape.buffer[y * pitch..y * pitch + row_bytes];
        let dst = &mut out[y * row_bytes..y * row_bytes + row_bytes];
        dst.copy_from_slice(src);
    }
    out
}

fn create_srv(
    dev: &ID3D11Device4,
    tex: &Texture,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView> {
    create_srv_raw(dev, tex.as_raw_ref(), format)
}

fn create_srv_raw(
    dev: &ID3D11Device4,
    tex: &ID3D11Texture2D,
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView> {
    let mut srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: format,
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: Default::default(),
    };
    srv_desc.Anonymous.Texture2D = D3D11_TEX2D_SRV {
        MostDetailedMip: 0,
        MipLevels: 1,
    };
    let mut srv = None;
    if let Err(e) =
        unsafe { dev.CreateShaderResourceView(tex, Some(&srv_desc), Some(&mut srv)) }
    {
        return Err(DDApiError::Unexpected(format!(
            "failed to create cursor shader resource view. {:?}",
            e
        )));
    }
    Ok(srv.unwrap())
}

fn create_rtv(
    dev: &ID3D11Device4,
    tex: &Texture,
    format: DXGI_FORMAT,
) -> Result<ID3D11RenderTargetView> {
    let mut rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: format,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: Default::default(),
    };
    rtv_desc.Anonymous.Texture2D = D3D11_TEX2D_RTV { MipSlice: 0 };
    let mut rtv = None;
    if let Err(e) =
        unsafe { dev.CreateRenderTargetView(tex.as_raw_ref(), Some(&rtv_desc), Some(&mut rtv)) }
    {
        return Err(DDApiError::Unexpected(format!(
            "failed to create cursor render target view. {:?}",
            e
        )));
    }
    Ok(rtv.unwrap())
}