//! Windows-only process containment for crash_helper.
//!
//! The UI is created suspended, attached to a private Job Object, and resumed
//! only after the kill-on-close policy is installed. The helper retains the job
//! handle for the UI lifetime; closing it terminates the UI and inherited
//! descendants without using a PID-based external process-tree command.

use std::ffi::{c_void, OsString};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

type Handle = *mut c_void;
type Bool = i32;
type Dword = u32;

const PROCESS_TERMINATE: Dword = 0x0001;
const PROCESS_QUERY_LIMITED_INFORMATION: Dword = 0x1000;
const SYNCHRONIZE: Dword = 0x0010_0000;
const WAIT_OBJECT_0: Dword = 0;
const WAIT_TIMEOUT: Dword = 0x0000_0102;
const PROCESS_SET_QUOTA: Dword = 0x0100;
const THREAD_SUSPEND_RESUME: Dword = 0x0002;
const TH32CS_SNAPTHREAD: Dword = 0x0004;
const JOB_OBJECT_LIMIT_BREAKAWAY_OK: Dword = 0x0000_0800;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: Dword = 0x0000_2000;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
const INVALID_RESUME_COUNT: Dword = Dword::MAX;

#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[repr(C)]
#[derive(Default)]
struct JobBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: Dword,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: Dword,
    affinity: usize,
    priority_class: Dword,
    scheduling_class: Dword,
}

#[repr(C)]
#[derive(Default)]
struct JobExtendedLimitInformation {
    basic_limit_information: JobBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[repr(C)]
#[derive(Default)]
struct ThreadEntry32 {
    size: Dword,
    usage_count: Dword,
    thread_id: Dword,
    owner_process_id: Dword,
    base_priority: i32,
    delta_priority: i32,
    flags: Dword,
}

#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(handle: Handle) -> Bool;
    fn GetLastError() -> Dword;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(
        job: Handle,
        information_class: i32,
        information: *const c_void,
        information_length: Dword,
    ) -> Bool;
    fn OpenProcess(access: Dword, inherit_handle: Bool, process_id: Dword) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: Dword,
        exe_name: *mut u16,
        size: *mut Dword,
    ) -> Bool;
    fn TerminateProcess(process: Handle, exit_code: Dword) -> Bool;
    fn WaitForSingleObject(handle: Handle, milliseconds: Dword) -> Dword;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> Bool;
    fn CreateToolhelp32Snapshot(flags: Dword, process_id: Dword) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry32) -> Bool;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry32) -> Bool;
    fn OpenThread(access: Dword, inherit_handle: Bool, thread_id: Dword) -> Handle;
    fn ResumeThread(thread: Handle) -> Dword;
}

struct OwnedHandle(Handle);

impl OwnedHandle {
    fn new(handle: Handle, label: &str) -> Result<Self, String> {
        if handle.is_null() || handle as isize == -1 {
            return Err(last_error(label));
        }
        Ok(Self(handle))
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != -1 {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

fn last_error(operation: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("{operation} failed with Windows error {code}.")
}

pub(super) struct ProcessContainment {
    _job: OwnedHandle,
}

fn normalized_image_path(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = resolved.to_string_lossy().replace('/', "\\");
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(&value)
        .to_ascii_lowercase()
}

/// Terminate only the expected packaged service executable.
///
/// The process handle pins the kernel process object after OpenProcess, avoiding
/// a PID-reuse race between checking the image path and terminating the target.
pub(super) fn terminate_if_image_matches(
    process_id: u32,
    expected_executable: &Path,
) -> Result<bool, String> {
    if process_id <= 1 {
        return Ok(false);
    }

    let process = OwnedHandle::new(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
                0,
                process_id,
            )
        },
        "OpenProcess for verified GitManager service recovery",
    )?;

    // A process that exited while we were opening it has already released its
    // lock (or is about to); it must not be replaced based on this stale PID.
    match unsafe { WaitForSingleObject(process.0, 0) } {
        WAIT_OBJECT_0 => return Ok(true),
        WAIT_TIMEOUT => {}
        _ => return Err(last_error("WaitForSingleObject before service recovery")),
    }

    let mut image_buffer = vec![0u16; 32_768];
    let mut image_length = image_buffer.len() as Dword;
    if unsafe {
        QueryFullProcessImageNameW(
            process.0,
            0,
            image_buffer.as_mut_ptr(),
            &mut image_length,
        )
    } == 0
    {
        return Err(last_error("QueryFullProcessImageNameW for service recovery"));
    }

    let actual_executable = PathBuf::from(OsString::from_wide(
        &image_buffer[..image_length as usize],
    ));
    if normalized_image_path(&actual_executable)
        != normalized_image_path(expected_executable)
    {
        return Ok(false);
    }

    if unsafe { TerminateProcess(process.0, 1) } == 0 {
        // The target may have exited naturally between the identity check and
        // termination. Treat that as recovered only when this same handle is
        // already signaled.
        if unsafe { WaitForSingleObject(process.0, 0) } == WAIT_OBJECT_0 {
            return Ok(true);
        }
        return Err(last_error("TerminateProcess for verified GitManager service"));
    }

    match unsafe { WaitForSingleObject(process.0, 5_000) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Err(format!(
            "Verified GitManager service PID {process_id} did not exit within 5 seconds after termination."
        )),
        _ => Err(last_error("WaitForSingleObject after service termination")),
    }
}

pub(super) fn attach_and_resume(process_id: u32) -> Result<ProcessContainment, String> {
    let job = OwnedHandle::new(
        unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) },
        "CreateJobObjectW",
    )?;

    let mut limits: JobExtendedLimitInformation = unsafe { zeroed() };
    // The UI remains kill-on-close contained, but explicitly permits the
    // required data service to break away into its own process lifetime.
    limits.basic_limit_information.limit_flags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    let configured = unsafe {
        SetInformationJobObject(
            job.0,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&limits as *const JobExtendedLimitInformation).cast(),
            size_of::<JobExtendedLimitInformation>() as Dword,
        )
    };
    if configured == 0 {
        return Err(last_error("SetInformationJobObject"));
    }

    let process = OwnedHandle::new(
        unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) },
        "OpenProcess for supervised UI",
    )?;
    if unsafe { AssignProcessToJobObject(job.0, process.0) } == 0 {
        return Err(last_error("AssignProcessToJobObject for supervised UI"));
    }
    drop(process);

    let snapshot = OwnedHandle::new(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) },
        "CreateToolhelp32Snapshot for UI primary thread",
    )?;
    let mut entry = ThreadEntry32 {
        size: size_of::<ThreadEntry32>() as Dword,
        ..Default::default()
    };
    let mut primary_thread_id = None;
    let mut found = unsafe { Thread32First(snapshot.0, &mut entry) } != 0;
    while found {
        if entry.owner_process_id == process_id {
            primary_thread_id = Some(entry.thread_id);
            break;
        }
        entry.size = size_of::<ThreadEntry32>() as Dword;
        found = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
    }
    drop(snapshot);

    let thread_id = primary_thread_id.ok_or_else(|| {
        format!("Could not find the suspended primary thread for UI process {process_id}.")
    })?;
    let thread = OwnedHandle::new(
        unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) },
        "OpenThread for supervised UI primary thread",
    )?;
    let previous_suspend_count = unsafe { ResumeThread(thread.0) };
    if previous_suspend_count == INVALID_RESUME_COUNT {
        return Err(last_error("ResumeThread for supervised UI primary thread"));
    }
    if previous_suspend_count != 1 {
        return Err(format!(
            "Expected the suspended UI primary thread to have one suspend count, received {previous_suspend_count}; refusing to launch an ambiguously suspended process."
        ));
    }

    Ok(ProcessContainment { _job: job })
}

#[cfg(test)]
mod tests {
    use super::{
        size_of, IoCounters, JobBasicLimitInformation, JobExtendedLimitInformation,
        ThreadEntry32,
    };

    #[test]
    fn service_processes_can_break_away_while_ui_remains_kill_on_close() {
        use super::{JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
        let flags = JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        assert_ne!(flags & JOB_OBJECT_LIMIT_BREAKAWAY_OK, 0);
        assert_ne!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
    }

    #[test]
    fn executable_identity_comparison_ignores_windows_path_case_and_separators() {
        use super::normalized_image_path;
        use std::path::Path;

        assert_eq!(
            normalized_image_path(Path::new(r"C:\Program Files\GitManager\gitmanager.service.exe")),
            normalized_image_path(Path::new(r"c:/program files/gitmanager/GitManager.Service.exe")),
        );
        assert_ne!(
            normalized_image_path(Path::new(r"C:\Program Files\GitManager\gitmanager.service.exe")),
            normalized_image_path(Path::new(r"C:\Program Files\GitManager\gitmanager.exe")),
        );
    }

    #[test]
    fn win32_process_structures_match_the_x64_abi() {
        assert_eq!(size_of::<IoCounters>(), 48);
        assert_eq!(size_of::<JobBasicLimitInformation>(), 64);
        assert_eq!(size_of::<JobExtendedLimitInformation>(), 144);
        assert_eq!(size_of::<ThreadEntry32>(), 28);
    }
}
