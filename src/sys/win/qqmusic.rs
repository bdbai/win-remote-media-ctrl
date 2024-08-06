use std::ffi::OsString;
use std::io;
use std::mem::MaybeUninit;
use std::os::raw::c_void;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::{
    ffi::CStr,
    mem::{size_of, size_of_val},
};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{FALSE, HANDLE, HMODULE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleFileNameExA, LIST_MODULES_32BIT,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

pub(super) struct QQMusicProcess {
    process: OwnedHandle,
    qqmusic_dll_base: isize,
    scratch_buf: Box<[u16]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FullInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_img_path_or_url: OsString,
    pub position: u32,
    pub duration: u32,
    pub paused: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RawTrackInfo {
    pub title_ptr: u32,
    pub artist_ptr: u32,
    pub album_ptr: u32,
    pub position: u32,
    pub duration: u32,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RawInfo {
    pub paused: u8,
    pub track_info: RawTrackInfo,
    pub album_img_ptr: u32,
}

impl QQMusicProcess {
    pub fn new() -> Self {
        Self {
            process: unsafe { OwnedHandle::from_raw_handle(-1isize as _) },
            qqmusic_dll_base: 0,
            scratch_buf: vec![0u16; 1024].into_boxed_slice(),
        }
    }

    fn try_open_process(&mut self) -> io::Result<()> {
        static PROCESS_NAME: &[u8; 12] = b"QQMusic.exe\0";
        static MODULE_NAME: &str = "\\QQMusic.dll";

        let mut pid = None;
        unsafe {
            let raw_snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
            let _snapshot: OwnedHandle = OwnedHandle::from_raw_handle(raw_snapshot.0 as _);
            let mut entry = PROCESSENTRY32::default();
            entry.dwSize = std::mem::size_of_val(&entry) as _;
            Process32First(raw_snapshot, &mut entry)?;
            while let Ok(()) = Process32Next(raw_snapshot, &mut entry) {
                if entry.szExeFile[..12] == *PROCESS_NAME {
                    pid = Some(entry.th32ProcessID);
                    break;
                }
            }
        }
        let Some(pid) = pid else { return Ok(()) };

        let mut mid = None;
        let process;
        unsafe {
            process = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid)?;
            self.process = OwnedHandle::from_raw_handle(process.0 as _);
            let mut hmods = [HMODULE::default(); 1024];
            let mut cb_needed = 0;
            EnumProcessModulesEx(
                process,
                &mut hmods as *mut _ as _,
                std::mem::size_of_val(&hmods) as _,
                &mut cb_needed,
                LIST_MODULES_32BIT,
            )?;

            for hmod in &hmods[..(cb_needed as usize / size_of::<HMODULE>())] {
                let mut lpfilename = [0u8; 1024];
                if GetModuleFileNameExA(process, *hmod, &mut lpfilename) == 0 {
                    continue;
                }

                let filename = CStr::from_ptr(lpfilename.as_ptr() as _).to_string_lossy();
                if filename.ends_with(MODULE_NAME) {
                    mid = Some(*hmod);
                    break;
                }
            }
        }

        self.qqmusic_dll_base = if let Some(mid) = mid {
            mid.0
        } else {
            return Ok(());
        };
        Ok(())
    }

    unsafe fn read_dll_data<T>(&self, offset: isize) -> io::Result<T> {
        let mut target = MaybeUninit::uninit();
        ReadProcessMemory(
            HANDLE(self.process.as_raw_handle() as _),
            (self.qqmusic_dll_base + offset) as *const c_void as _,
            target.as_mut_ptr() as _,
            size_of::<T>(),
            None,
        )?;
        Ok(target.assume_init())
    }
    fn fill_wstring(&mut self, addr: u32) -> io::Result<()> {
        unsafe {
            ReadProcessMemory(
                HANDLE(self.process.as_raw_handle() as _),
                addr as usize as *const c_void,
                self.scratch_buf.as_mut_ptr() as _,
                size_of_val(self.scratch_buf.as_ref()),
                None,
            )?;
        }
        Ok(())
    }
    fn read_wstring_to_string(&mut self, addr: u32) -> io::Result<Option<String>> {
        self.fill_wstring(addr)?;
        self.scratch_buf[self.scratch_buf.len() - 1] = 0;
        self.scratch_buf[self.scratch_buf.len() - 2] = 0;
        let wcstr = PCWSTR(self.scratch_buf.as_ptr());
        Ok(unsafe { wcstr.to_string() }.ok())
    }
    pub(super) fn collect_raw_info(&mut self) -> io::Result<Option<RawInfo>> {
        let version: u32 = match unsafe { self.read_dll_data(0xAAAA84) } {
            Ok(version) => version,
            Err(_) => {
                self.try_open_process()?;
                unsafe { self.read_dll_data(0xAAAA84) }?
            }
        };
        if version != 2036 {
            return Ok(None);
        }

        let mut last_raw_info = RawInfo::default();
        for _ in 0..6 {
            let paused = unsafe { self.read_dll_data(0xAAEEF4)? };
            let track_info = unsafe { self.read_dll_data(0xAAEF38)? };
            let album_img_ptr = unsafe { self.read_dll_data(0xAAF088)? };
            let mut raw_info = RawInfo {
                paused,
                track_info,
                album_img_ptr,
            };
            let latest_position = std::mem::replace(
                &mut raw_info.track_info.position,
                last_raw_info.track_info.position,
            );
            let stable = raw_info == last_raw_info;
            raw_info.track_info.position = latest_position;
            if stable {
                return Ok(Some(raw_info));
            }
            last_raw_info = raw_info;
        }
        Ok(None)
    }
    pub(super) fn collect_full_info(&mut self) -> io::Result<Option<FullInfo>> {
        let raw_info = match self.collect_raw_info()? {
            Some(raw_info) => raw_info,
            None => return Ok(None),
        };

        let Some(title) = self.read_wstring_to_string(raw_info.track_info.title_ptr)? else {
            return Ok(None);
        };
        let Some(artist) = self.read_wstring_to_string(raw_info.track_info.artist_ptr)? else {
            return Ok(None);
        };
        let Some(album) = self.read_wstring_to_string(raw_info.track_info.album_ptr)? else {
            return Ok(None);
        };
        let album_img_path_or_url = unsafe {
            self.fill_wstring(raw_info.album_img_ptr as _)?;
            OsString::from_wide(PCWSTR::from_raw(self.scratch_buf.as_ptr()).as_wide())
        };

        Ok(Some(FullInfo {
            title,
            artist,
            album,
            album_img_path_or_url,
            position: raw_info.track_info.position,
            duration: raw_info.track_info.duration,
            paused: raw_info.paused != 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qqmusic_process() -> io::Result<()> {
        let mut process = QQMusicProcess::new();
        let info = process.collect_full_info()?;
        println!("{:?}", info);
        Ok(())
    }
}
