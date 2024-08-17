use std::{io, mem::size_of_val, slice};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_CONTROL, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MENU, VK_V,
    VK_VOLUME_DOWN, VK_VOLUME_UP,
};

fn new_vkeydown_input(vk_code: VIRTUAL_KEY) -> INPUT {
    INPUT {
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
    }
}
fn send_input(input: &INPUT) -> io::Result<()> {
    unsafe {
        let res = SendInput(slice::from_ref(input), size_of_val(input) as _);
        if res == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

struct KeyPressGuard(INPUT, io::Result<()>);
impl KeyPressGuard {
    fn keydown(vk_code: VIRTUAL_KEY) -> Self {
        let input = new_vkeydown_input(vk_code);
        let keydown_res = send_input(&input);
        Self(input, keydown_res)
    }
    fn manual_keyup(&mut self) -> io::Result<()> {
        self.0.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP;
        send_input(&self.0)
    }
    fn keyup(mut self) -> io::Result<()> {
        let keyup_res = self.manual_keyup();
        std::mem::replace(&mut self.1, Ok(()))?;
        keyup_res
    }
}

impl Drop for KeyPressGuard {
    fn drop(&mut self) {
        self.manual_keyup().ok();
    }
}

fn press_key(vk_code: VIRTUAL_KEY) -> io::Result<()> {
    KeyPressGuard::keydown(vk_code).keyup()
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

pub fn press_like() -> io::Result<()> {
    let ctrl_guard = KeyPressGuard::keydown(VK_CONTROL);
    let alt_guard = KeyPressGuard::keydown(VK_MENU);
    let v_guard = KeyPressGuard::keydown(VK_V);
    v_guard.keyup()?;
    alt_guard.keyup()?;
    ctrl_guard.keyup()?;
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_key() {
        super::press_play_pause().unwrap();
        super::press_like().unwrap();
    }
}
