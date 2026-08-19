//! Thin wrappers for creating D3D11 vertex/pixel shaders from compiled bytecode.
//!
//! The bytecode is produced at build time by the `shader_macro::compile_shader!`
//! proc-macro (build-time `D3DCompile2`), mirroring the pattern used by the
//! sibling `dxfilter` crate. At runtime only `CreateVertexShader`/`CreatePixelShader`
//! run, so no `d3dcompiler` DLL is needed.

use windows::Win32::Graphics::Direct3D11::{ID3D11Device4, ID3D11PixelShader, ID3D11VertexShader};

use crate::errors::DDApiError;
use crate::Result;

/// Holds an `ID3D11VertexShader`. Build the bytecode with `compile_shader!`.
pub struct VertexShader(ID3D11VertexShader);

impl VertexShader {
	/// Create a vertex shader from compiled shader bytecode and a device.
	pub fn new(blob: &[u8], device: &ID3D11Device4) -> Result<Self> {
		let mut shader = None;
		if let Err(e) =
			unsafe { device.CreateVertexShader(blob, None, Some(&mut shader)) }
		{
			return Err(DDApiError::Unexpected(format!(
				"failed to create vertex shader. {:?}",
				e
			)));
		}
		Ok(Self(shader.unwrap()))
	}

	/// Raw reference to the underlying `ID3D11VertexShader`.
	pub fn as_raw_ref(&self) -> &ID3D11VertexShader {
		&self.0
	}
}

/// Holds an `ID3D11PixelShader`. Build the bytecode with `compile_shader!`.
pub struct PixelShader(ID3D11PixelShader);

impl PixelShader {
	/// Create a pixel shader from compiled shader bytecode and a device.
	pub fn new(blob: &[u8], device: &ID3D11Device4) -> Result<Self> {
		let mut shader = None;
		if let Err(e) =
			unsafe { device.CreatePixelShader(blob, None, Some(&mut shader)) }
		{
			return Err(DDApiError::Unexpected(format!(
				"failed to create pixel shader. {:?}",
				e
			)));
		}
		Ok(Self(shader.unwrap()))
	}

	/// Raw reference to the underlying `ID3D11PixelShader`.
	pub fn as_raw_ref(&self) -> &ID3D11PixelShader {
		&self.0
	}
}