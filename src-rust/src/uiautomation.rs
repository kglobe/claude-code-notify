//! UI Automation COM interface for Windows Terminal tab detection.
//!
//! Uses IUIAutomation to enumerate tabs, find the selected one,
//! and capture/match its RuntimeId.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;
use windows::Win32::UI::Accessibility::*;

/// Get the RuntimeId string of the currently selected WT tab.
/// Returns empty string on failure.
pub fn get_selected_tab_runtime_id(hwnd: HWND) -> String {
    unsafe { get_selected_tab_runtime_id_inner(hwnd).unwrap_or_default() }
}

unsafe fn get_selected_tab_runtime_id_inner(hwnd: HWND) -> Result<String> {
    for tab in find_tabs(hwnd)? {
        let pattern: Result<IUIAutomationSelectionItemPattern> =
            tab.GetCurrentPatternAs(UIA_SelectionItemPatternId);
        if let Ok(pattern) = pattern {
            if pattern.CurrentIsSelected()?.as_bool() {
                return get_runtime_id_string(&tab);
            }
        }
    }
    Ok(String::new())
}

/// Select a WT tab by matching its RuntimeId string.
/// Returns true if the tab was found and selected.
pub fn select_tab_by_runtime_id(hwnd: HWND, target_runtime_id: &str) -> bool {
    unsafe { select_tab_inner(hwnd, target_runtime_id).unwrap_or(false) }
}

unsafe fn select_tab_inner(hwnd: HWND, target_runtime_id: &str) -> Result<bool> {
    for tab in find_tabs(hwnd)? {
        if get_runtime_id_string(&tab).unwrap_or_default() == target_runtime_id {
            let pattern: Result<IUIAutomationSelectionItemPattern> =
                tab.GetCurrentPatternAs(UIA_SelectionItemPatternId);
            if let Ok(pattern) = pattern {
                let _ = pattern.Select();
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// All TabItem descendants of the window.
unsafe fn find_tabs(hwnd: HWND) -> Result<Vec<IUIAutomationElement>> {
    let automation: IUIAutomation = CoCreateInstance(
        &CUIAutomation as *const GUID,
        None,
        CLSCTX_INPROC_SERVER,
    )?;
    let element = automation.ElementFromHandle(hwnd)?;
    let val = VARIANT::from(UIA_TabItemControlTypeId.0);
    let condition = automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &val)?;
    let tabs = element.FindAll(TreeScope_Descendants, &condition)?;
    (0..tabs.Length()?).map(|i| tabs.GetElement(i)).collect()
}

unsafe fn get_runtime_id_string(element: &IUIAutomationElement) -> Result<String> {
    let sa_ptr = element.GetRuntimeId()?;
    if sa_ptr.is_null() {
        return Ok(String::new());
    }

    let read = || -> Result<String> {
        let lower = SafeArrayGetLBound(sa_ptr, 1)?;
        let upper = SafeArrayGetUBound(sa_ptr, 1)?;
        let mut parts = Vec::new();
        for i in lower..=upper {
            let mut val: i32 = 0;
            SafeArrayGetElement(
                sa_ptr,
                &i as *const i32 as *const _,
                &mut val as *mut i32 as *mut _,
            )?;
            parts.push(val.to_string());
        }
        Ok(parts.join("."))
    };
    let result = read();
    // Destroy on every path; an early `?` used to leak the array.
    let _ = SafeArrayDestroy(sa_ptr);
    result
}
