use std::{io, mem::size_of_val, slice};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_VOLUME_DOWN, VK_VOLUME_UP,
};

fn press_key(vk_code: VIRTUAL_KEY) -> io::Result<()> {
    unsafe {
        let mut input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_code,
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let res = SendInput(slice::from_ref(&input), size_of_val(&input) as _);
        let mut last_error = None;
        if res == 0 {
            last_error = Some(io::Error::last_os_error());
        }

        input.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        let res = SendInput(slice::from_ref(&input), size_of_val(&input) as _);
        if let Some(e) = last_error {
            return Err(e);
        }
        if res == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub fn press_play_pause() -> io::Result<()> {
    press_key(VK_MEDIA_PLAY_PAUSE)
}

pub fn press_next_track() -> io::Result<()> {
    press_key(VK_MEDIA_NEXT_TRACK)
}

pub fn press_prev_track() -> io::Result<()> {
    press_key(VK_MEDIA_PREV_TRACK)
}

pub fn press_volume_down() -> io::Result<()> {
    press_key(VK_VOLUME_DOWN)
}

pub fn press_volume_up() -> io::Result<()> {
    press_key(VK_VOLUME_UP)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_key() {
        super::press_play_pause().unwrap();
    }
}
