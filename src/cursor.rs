use std::sync::LazyLock;
use windows::core::PCWSTR;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum CursorID {
    Unknown = 0,
    Default,
    Help,
    Pointer,
    Progress,
    Wait,
    Crosshair,
    Text,
    Move,
    NotAllowed,
    NResize,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
}

static CURSOR_MAP: LazyLock<[(isize, CursorID); 14]> = LazyLock::new(|| {
    let pairs: [(PCWSTR, CursorID); 14] = [
        (IDC_ARROW, CursorID::Default),
        (IDC_HAND, CursorID::Pointer),
        (IDC_HELP, CursorID::Help),
        (IDC_APPSTARTING, CursorID::Progress),
        (IDC_WAIT, CursorID::Wait),
        (IDC_CROSS, CursorID::Crosshair),
        (IDC_IBEAM, CursorID::Text),
        (IDC_SIZEALL, CursorID::Move),
        (IDC_NO, CursorID::NotAllowed),
        (IDC_SIZENS, CursorID::NsResize),
        (IDC_SIZEWE, CursorID::EwResize),
        (IDC_SIZENWSE, CursorID::NwseResize),
        (IDC_SIZENESW, CursorID::NeswResize),
        (IDC_UPARROW, CursorID::NResize),
    ];

    pairs.map(|(idc, id)| {
        let handle = unsafe { LoadCursorW(None, idc) }
            .map(|h| h.0)
            .unwrap_or(0);
        (handle, id)
    })
});

pub fn get_cursor_id() -> Option<CursorID> {
    unsafe {
        let mut info = CURSORINFO::default();
        info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;

        if GetCursorInfo(&mut info).is_err() || info.flags.0 == 0 {
            return None;
        }

        let hcursor = info.hCursor.0;

        for (handle, id) in CURSOR_MAP.iter() {
            if *handle == hcursor {
                return Some(*id);
            }
        }

        None
    }
}