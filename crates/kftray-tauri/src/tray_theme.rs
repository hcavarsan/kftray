#![cfg(target_os = "windows")]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskbarTheme {
    Light,
    Dark,
}

pub fn current() -> TaskbarTheme {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER,
        RRF_RT_REG_DWORD,
        RegGetValueW,
    };
    use windows::core::w;

    let mut value: u32 = 1;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };

    if status != ERROR_SUCCESS {
        return TaskbarTheme::Light;
    }

    if value == 0 {
        TaskbarTheme::Dark
    } else {
        TaskbarTheme::Light
    }
}
