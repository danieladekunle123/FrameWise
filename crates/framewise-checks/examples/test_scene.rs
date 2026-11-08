//! A fixed, repeatable GPU workload for testing benchmark mode: a window that draws a heavy
//! full-screen shader with Direct3D 11, uncapped, for a set time. The animation advances by frame
//! number, not the clock, so every run does identical work.
//!
//! cargo run --release -p framewise-checks --example test_scene -- [seconds] [iterations]
//!   seconds     how long to run (default 20)
//!   iterations  shader loop count: more = heavier, lower FPS (default 5000, about 120 FPS on an RTX 4060 Ti)

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    use windows::core::{s, w, Interface, PCSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
    use windows::Win32::Graphics::Direct3D::*;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    let args: Vec<String> = std::env::args().collect();
    let seconds: f64 = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(20.0);
    let iterations: u32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(5000);
    let (width, height) = (1280u32, 720u32);

    extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if m == WM_DESTROY {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("FrameWiseTestScene");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW)?, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("FrameWise test scene"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            80,
            80,
            width as i32 + 16,
            height as i32 + 39,
            None,
            None,
            instance,
            None,
        )?;
        let _ = SetForegroundWindow(hwnd);

        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC { Width: width, Height: height, Format: DXGI_FORMAT_B8G8R8A8_UNORM, ..Default::default() },
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: hwnd,
            Windowed: TRUE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            // Uncapped like a game with V-Sync off: without tearing, a windowed flip-model app locks to
            // fractions of the refresh rate (120 FPS on a 240 Hz screen), which hides real variation.
            Flags: DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING.0 as u32,
        };
        let mut swap: Option<IDXGISwapChain> = None;
        let mut device: Option<ID3D11Device> = None;
        let mut ctx: Option<ID3D11DeviceContext> = None;
        D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&desc),
            Some(&mut swap),
            Some(&mut device),
            None,
            Some(&mut ctx),
        )?;
        let (swap, device, ctx) = (swap.unwrap(), device.unwrap(), ctx.unwrap());

        let hlsl = format!(
            r#"
            cbuffer C : register(b0) {{ float t; float3 pad; }};
            float4 vs(uint id : SV_VertexID) : SV_Position {{
                float2 uv = float2((id << 1) & 2, id & 2);
                return float4(uv * float2(2, -2) + float2(-1, 1), 0, 1);
            }}
            float4 ps(float4 p : SV_Position) : SV_Target {{
                float2 uv = p.xy / 720.0;
                float v = 0;
                [loop] for (int i = 0; i < {iterations}; i++) {{ v += sin(uv.x * i * 0.05 + t) * cos(uv.y * i * 0.07 - t); }}
                return float4(frac(v * 0.1), frac(v * 0.05), frac(v * 0.025), 1);
            }}"#
        );
        let compile = |entry: PCSTR, target: PCSTR| -> windows::core::Result<Vec<u8>> {
            let mut code = None;
            let mut errors = None;
            let r = D3DCompile(hlsl.as_ptr() as *const _, hlsl.len(), None, None, None, entry, target, 0, 0, &mut code, Some(&mut errors));
            if let Err(e) = r {
                if let Some(err) = errors {
                    let msg = std::slice::from_raw_parts(err.GetBufferPointer() as *const u8, err.GetBufferSize());
                    eprintln!("{}", String::from_utf8_lossy(msg));
                }
                return Err(e);
            }
            let code = code.unwrap();
            Ok(std::slice::from_raw_parts(code.GetBufferPointer() as *const u8, code.GetBufferSize()).to_vec())
        };
        let vs_code = compile(s!("vs"), s!("vs_5_0"))?;
        let ps_code = compile(s!("ps"), s!("ps_5_0"))?;
        let mut vs = None;
        device.CreateVertexShader(&vs_code, None, Some(&mut vs))?;
        let mut ps = None;
        device.CreatePixelShader(&ps_code, None, Some(&mut ps))?;
        let mut cb = None;
        device.CreateBuffer(
            &D3D11_BUFFER_DESC { ByteWidth: 16, Usage: D3D11_USAGE_DEFAULT, BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32, ..Default::default() },
            None,
            Some(&mut cb),
        )?;
        let back: ID3D11Texture2D = swap.GetBuffer(0)?;
        let mut rtv = None;
        device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
        let viewport = D3D11_VIEWPORT { Width: width as f32, Height: height as f32, MaxDepth: 1.0, ..Default::default() };

        let start = std::time::Instant::now();
        let mut frame: u64 = 0;
        let mut msg = MSG::default();
        'outer: while start.elapsed().as_secs_f64() < seconds {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    break 'outer;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let t = [frame as f32 / 60.0, 0.0, 0.0, 0.0];
            ctx.UpdateSubresource(cb.as_ref().unwrap(), 0, None, t.as_ptr() as *const _, 0, 0);
            ctx.OMSetRenderTargets(Some(&[rtv.clone()]), None);
            ctx.RSSetViewports(Some(&[viewport]));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.VSSetShader(vs.as_ref(), None);
            ctx.PSSetShader(ps.as_ref(), None);
            ctx.PSSetConstantBuffers(0, Some(&[cb.clone()]));
            ctx.Draw(3, 0);
            swap.Present(0, DXGI_PRESENT_ALLOW_TEARING).ok()?;
            frame += 1;
        }
        let secs = start.elapsed().as_secs_f64();
        println!("test scene: {frame} frames in {secs:.1} s = {:.1} FPS (iterations {iterations})", frame as f64 / secs);
        let _ = DestroyWindow(hwnd);
        let _ = swap.cast::<IDXGISwapChain>();
    }
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
