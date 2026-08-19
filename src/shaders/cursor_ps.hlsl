//--------------------------------------------------------------------------------------
// cursor_ps.hlsl
// Composites the DXGI pointer shape onto the desktop frame. Works for all frame
// formats (B8G8R8A8_UNORM, R10G10B10A2_UNORM, R16G16B16A16_FLOAT) because it samples
// the clean frame as a shader resource and writes the blended result to a separate
// render target. Sampling the destination is what makes monochrome/masked XOR
// inversion possible without GDI.
//--------------------------------------------------------------------------------------
struct PS_INPUT
{
	float4 Pos : SV_POSITION;
	float2 Tex : TEXCOORD;
};

// Must match the Rust `CursorCB` (cursor_renderer.rs). 40 bytes of data; the
// constant buffer is sized up to 48 (multiple of 16) and only the first 40 are used.
cbuffer CursorCB : register(b0)
{
	uint2 g_FrameSize;     // frame width, height (pixels)
	int2  g_CursorRect;    // cursor rect top-left = position - hotspot (pixels, signed for off-screen)
	uint2 g_CursorSize;    // cursor width, height (pixels)
	uint  g_Kind;          // 1 = monochrome, 2 = color, 4 = masked color
	uint  g_Visible;       // 0 = no cursor (passthrough), 1 = composite
	uint  g_FrameFormat;   // 0 = B8G8R8A8_UNORM, 1 = R10G10B10A2_UNORM, 2 = R16G16B16A16_FLOAT
	uint  g_pad0;
};

Texture2D<float4> FrameTex  : register(t0);   // desktop frame (frame's DXGI format)
Texture2D<float4> CursorTex : register(t1);   // cursor bitmap (B8G8R8A8_UNORM)
SamplerState      LinearSamp : register(s0);   // linear, clamp

// sRGB -> linear (precise sRGB EOTF). Used to bring 8-bit sRGB cursor colors into
// the linear scRGB space of R16G16B16A16_FLOAT frames before blending.
float3 srgb_to_linear(float3 c)
{
	float3 lo = c / 12.92;
	float3 hi = pow(max(c + 0.055, 0.0) / 1.055, 2.4);
	return (c <= 0.04045) ? lo : hi;
}

float4 main(PS_INPUT IN) : SV_Target
{
	float2 px = IN.Pos.xy;
	float2 frameUV = px / float2(g_FrameSize);
	float4 dst = FrameTex.Sample(LinearSamp, frameUV);

	// No cursor / hidden -> copy the frame through unchanged.
	if (g_Visible == 0)
		return dst;

	// Cursor rect membership. Out-of-rect pixels keep the destination; the explicit
	// guard is essential because the CLAMP sampler would otherwise bleed edge
	// texels into the surrounding frame.
	float2 curUV = (px - float2(g_CursorRect)) / float2(g_CursorSize);
	if (any(curUV < 0.0) || any(curUV >= 1.0))
		return dst;

	float4 cur = CursorTex.Sample(LinearSamp, curUV); // B8G8R8A8_UNORM -> normalized rgba

	float3 outc;
	if (g_Kind == 1)
	{
		// Monochrome. A encoding (see cursor_renderer::preprocess_mono):
		//   0x00 = transparent, 0xFF = opaque (RGB is 0 black / 1 white), 0x80 = invert dst.
		if (abs(cur.a - 0.5) < 0.25)
		{
			outc = 1.0 - dst.rgb;            // invert (ill-defined for HDR > 1; mono never used there)
		}
		else if (cur.a < 0.25)
		{
			return dst;                       // transparent
		}
		else
		{
			outc = cur.rgb;                   // opaque black or white (0/1 -> unchanged by linear conversion)
		}
	}
	else if (g_Kind == 4)
	{
		// Masked color. alpha is a 1-bit mask:
		//   0x00 -> replace dst with RGB (opaque)
		//   0xFF -> XOR(RGB, dst) replaces dst
		if (cur.a < 0.5)
		{
			outc = cur.rgb;                   // opaque
		}
		else
		{
			// 8-bit integer XOR; exact for 8-bit frames. HDR never uses masked-color XOR.
			uint3 d = (uint3)(dst.rgb * 255.0 + 0.5);
			uint3 c = (uint3)(cur.rgb * 255.0 + 0.5);
			outc = float3(d ^ c) / 255.0;
		}
	}
	else
	{
		// Color (ARGB, straight alpha).
		outc = lerp(dst.rgb, cur.rgb, cur.a);
	}

	// Bring the cursor contribution into the frame's color space for scRGB (linear)
	// frames so the cursor's appearance matches. For 8-bit/10-bit (sRGB-encoded)
	// frames a direct blend is correct (matches how Windows composites the desktop
	// cursor onto sRGB/HDR10 output).
	if (g_FrameFormat == 2)
	{
		if (g_Kind == 1)
		{
			// mono opaque black/white is 0/1 in both spaces; invert already done in linear.
			if (abs(cur.a - 0.5) >= 0.25 && cur.a >= 0.25)
				outc = srgb_to_linear(cur.rgb);
		}
		else if (g_Kind == 4)
		{
			if (cur.a < 0.5)
				outc = srgb_to_linear(cur.rgb);
			// (XOR branch: leave as-is; masked-color XOR on HDR is not a real case.)
		}
		else
		{
			float3 curLinear = srgb_to_linear(cur.rgb);
			outc = lerp(dst.rgb, curLinear, cur.a);
		}
	}

	return float4(outc, dst.a);
}