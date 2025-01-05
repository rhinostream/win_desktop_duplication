use windows::core::HSTRING;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, COINIT_SPEED_OVER_MEMORY, CoInitializeEx};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};

fn find_terminal_idx(content: &[u16]) -> usize {
    for (i, val) in content.iter().enumerate() {
        if *val == 0 {
            return i;
        }
    }
    content.len()
}

pub fn convert_u16_to_string(data: &[u16]) -> String {
    let terminal_idx = find_terminal_idx(data);
    HSTRING::from_wide(&data[0..terminal_idx]).unwrap().to_string_lossy()
}

pub fn set_process_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

pub fn co_init() {
    unsafe {
        CoInitializeEx(None, COINIT_SPEED_OVER_MEMORY).unwrap();
    }
}