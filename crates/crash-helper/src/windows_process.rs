//! Windows-only process containment for crash_helper.
//!
//! The UI is created suspended, attached to a private Job Object, and resumed
//! only after the kill-on-close policy is installed. The helper retains the job
//! handle for the UI lifetime; closing it terminates the UI and inherited
//! descendants without using a PID-based external process-tree command.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};

type Handle = *mut c_void;
type Bool = i32;
type Dword = u32;

const PROCESS_TERMINATE: Dword = 0x0001;
const PROCESS_SET_QUOTA: Dword = 0x0100;
const THREAD_SUSPEND_RESUME: Dword = 0x0002;
const TH32CS_SNAPTHREAD: Dword = 0x0004;
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

pub(super) fn attach_and_resume(process_id: u32) -> Result<ProcessContainment, String> {
    let job = OwnedHandle::new(
        unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) },
        "CreateJobObjectW",
    )?;

    let mut limits: JobExtendedLimitInformation = unsafe { zeroed() };
    limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
    fn win32_process_structures_match_the_x64_abi() {
        assert_eq!(size_of::<IoCounters>(), 48);
        assert_eq!(size_of::<JobBasicLimitInformation>(), 64);
        assert_eq!(size_of::<JobExtendedLimitInformation>(), 144);
        assert_eq!(size_of::<ThreadEntry32>(), 28);
    }
}
