//! Reads the active sort order of an Explorer window showing a given folder,
//! so prev/next navigation matches what the user sees in Explorer.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IDispatch, IServiceProvider,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    IFolderView2, IPersistFolder2, IShellBrowser, IShellWindows, SHGetPathFromIDListW,
    SID_STopLevelBrowser, SORT_DESCENDING, SORTCOLUMN, ShellWindows,
};
use windows::core::{GUID, Interface, VARIANT};

use crate::wic::RPC_E_CHANGED_MODE_HRESULT;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    DateModified,
    DateCreated,
    Size,
}

#[derive(Clone, Copy)]
pub struct SortSpec {
    pub key: SortKey,
    pub ascending: bool,
}

pub const DEFAULT_SORT: SortSpec = SortSpec {
    key: SortKey::Name,
    ascending: true,
};

/// Looks for an open Explorer window showing `folder` and returns its current
/// sort order. Returns `None` when no window matches or the sort column has no
/// file-system equivalent (e.g. sort by type or rating).
pub fn query_sort_for_folder(folder: &Path) -> Option<SortSpec> {
    let _com = ComInit::new()?;
    unsafe {
        let shell_windows: IShellWindows =
            CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER).ok()?;
        let count = shell_windows.Count().ok()?;
        for i in 0..count {
            let Ok(disp) = shell_windows.Item(&VARIANT::from(i)) else {
                continue;
            };
            let Some(view) = folder_view_of_window(&disp) else {
                continue;
            };
            let Some(view_path) = folder_path_of_view(&view) else {
                continue;
            };
            if !same_folder(&view_path, folder) {
                continue;
            }
            return primary_sort_of_view(&view);
        }
    }
    None
}

unsafe fn folder_view_of_window(disp: &IDispatch) -> Option<IFolderView2> {
    unsafe {
        let provider: IServiceProvider = disp.cast().ok()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser).ok()?;
        let view = browser.QueryActiveShellView().ok()?;
        view.cast().ok()
    }
}

unsafe fn folder_path_of_view(view: &IFolderView2) -> Option<PathBuf> {
    unsafe {
        let persist: IPersistFolder2 = view.GetFolder().ok()?;
        let pidl = persist.GetCurFolder().ok()?;
        if pidl.is_null() {
            return None;
        }
        let mut buf = [0u16; 260];
        let has_path = SHGetPathFromIDListW(pidl, &mut buf).as_bool();
        CoTaskMemFree(Some(pidl as *const c_void));
        if !has_path {
            // Virtual folder (This PC, search results, ...) — no filesystem path.
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

unsafe fn primary_sort_of_view(view: &IFolderView2) -> Option<SortSpec> {
    unsafe {
        let count = view.GetSortColumnCount().ok()?;
        if count <= 0 {
            return None;
        }
        let mut columns = vec![SORTCOLUMN::default(); count as usize];
        view.GetSortColumns(&mut columns).ok()?;
        let primary = columns[0];
        let key = sort_key_for(&primary.propkey)?;
        Some(SortSpec {
            key,
            ascending: primary.direction != SORT_DESCENDING,
        })
    }
}

fn sort_key_for(propkey: &PROPERTYKEY) -> Option<SortKey> {
    // FMTID_Storage: System.ItemNameDisplay / Size / DateModified / DateCreated.
    const FMTID_STORAGE: GUID = GUID::from_u128(0xB725F130_47EF_101A_A5F1_02608C9EEBAC);
    // System.ItemDate (pid 100), the default "Date" column in picture folders.
    const FMTID_ITEM_DATE: GUID = GUID::from_u128(0xF7DB74B4_4287_4103_AFBA_F1B13DCD75CF);
    // System.Photo.DateTaken (pid 36867).
    const FMTID_PHOTO: GUID = GUID::from_u128(0x14B81DA1_0135_4D31_96D9_6CBFC9671A99);

    if propkey.fmtid == FMTID_STORAGE {
        return match propkey.pid {
            10 => Some(SortKey::Name),
            12 => Some(SortKey::Size),
            14 => Some(SortKey::DateModified),
            15 => Some(SortKey::DateCreated),
            _ => None,
        };
    }

    // EXIF dates are not readable from file metadata; modified time is the
    // closest approximation that keeps the order roughly matching Explorer.
    if (propkey.fmtid == FMTID_ITEM_DATE && propkey.pid == 100)
        || (propkey.fmtid == FMTID_PHOTO && propkey.pid == 36867)
    {
        return Some(SortKey::DateModified);
    }

    None
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.as_os_str().eq_ignore_ascii_case(b.as_os_str()),
    }
}

struct ComInit {
    should_uninitialize: bool,
}

impl ComInit {
    fn new() -> Option<Self> {
        let res = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if res == RPC_E_CHANGED_MODE_HRESULT {
            Some(Self {
                should_uninitialize: false,
            })
        } else if res.is_err() {
            None
        } else {
            Some(Self {
                should_uninitialize: true,
            })
        }
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}
