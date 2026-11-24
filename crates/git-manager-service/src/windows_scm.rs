//! Native Windows Service Control Manager (SCM) integration for `gitmanager.service`.
//!
//! This module implements a dual-mode Windows service entrypoint using direct
//! `advapi32` FFI so `gitmanager.service` requires zero extra crates and remains
//! compatible across Windows 7, 8, 8.1, 10, and 11:
//! - When launched by Windows SCM (`services.msc` or `sc.exe start`), it
//!   registers a control handler, reports `SERVICE_RUNNING` once the IPC pipe
//!   is ready, and drains gracefully on `SERVICE_CONTROL_STOP` / `SHUTDOWN`.
//! - When launched interactively, by `crash_helper`, Task Scheduler, or
//!   `HKCU\...\Run`, `StartServiceCtrlDispatcherW` fails with
//!   `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT` (1063) and returns `false` so
//!   `main()` runs the standard per-user loop.

#[cfg(windows)]
mod imp {
	use std::ffi::c_void;
	use std::os::windows::ffi::OsStrExt;
	use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

	const SERVICE_WIN32_OWN_PROCESS: u32 = 0x0000_0010;
	const SERVICE_STOPPED: u32 = 0x0000_0001;
	const SERVICE_START_PENDING: u32 = 0x0000_0002;
	const SERVICE_STOP_PENDING: u32 = 0x0000_0003;
	const SERVICE_RUNNING: u32 = 0x0000_0004;

	const SERVICE_ACCEPT_STOP: u32 = 0x0000_0001;
	const SERVICE_ACCEPT_SHUTDOWN: u32 = 0x0000_0004;

	const SERVICE_CONTROL_STOP: u32 = 0x0000_0001;
	const SERVICE_CONTROL_INTERROGATE: u32 = 0x0000_0004;
	const SERVICE_CONTROL_SHUTDOWN: u32 = 0x0000_0005;

	const ERROR_CALL_NOT_IMPLEMENTED: u32 = 120;
	const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: u32 = 1063;
	const NO_ERROR: u32 = 0;

	pub const WINDOWS_SERVICE_NAME: &str = "GitManagerCoreService";

	static SCM_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
	static SCM_STATUS_HANDLE: AtomicIsize = AtomicIsize::new(0);

	#[repr(C)]
	struct ServiceTableEntryW {
		service_name: *mut u16,
		service_proc: Option<unsafe extern "system" fn(u32, *mut *mut u16)>,
	}

	#[repr(C)]
	struct ServiceStatus {
		service_type: u32,
		current_state: u32,
		controls_accepted: u32,
		win32_exit_code: u32,
		service_specific_exit_code: u32,
		check_point: u32,
		wait_hint: u32,
	}

	#[link(name = "advapi32")]
	extern "system" {
		fn StartServiceCtrlDispatcherW(service_start_table: *const ServiceTableEntryW) -> i32;
		fn RegisterServiceCtrlHandlerExW(
			service_name: *const u16,
			handler_proc: unsafe extern "system" fn(u32, u32, *mut c_void, *mut c_void) -> u32,
			context: *mut c_void,
		) -> isize;
		fn SetServiceStatus(service_status_handle: isize, service_status: *const ServiceStatus) -> i32;
	}

	#[link(name = "kernel32")]
	extern "system" {
		fn GetLastError() -> u32;
	}

	fn wide_null(value: &str) -> Vec<u16> {
		std::ffi::OsStr::new(value)
			.encode_wide()
			.chain(std::iter::once(0))
			.collect()
	}

	/// Reports the current lifecycle state back to the Windows Service Control Manager.
	fn report_service_status(state: u32, win32_exit_code: u32, wait_hint: u32) {
		let handle = SCM_STATUS_HANDLE.load(Ordering::Acquire);
		if handle == 0 {
			return;
		}
		let controls_accepted = if state == SERVICE_RUNNING {
			SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
		} else {
			0
		};
		let status = ServiceStatus {
			service_type: SERVICE_WIN32_OWN_PROCESS,
			current_state: state,
			controls_accepted,
			win32_exit_code,
			service_specific_exit_code: 0,
			check_point: 0,
			wait_hint,
		};
		unsafe {
			let _ = SetServiceStatus(handle, &status);
		}
	}

	/// Handles stop, shutdown, and interrogate signals sent by Windows SCM.
	unsafe extern "system" fn service_control_handler(
		control: u32,
		_event_type: u32,
		_event_data: *mut c_void,
		_context: *mut c_void,
	) -> u32 {
		match control {
			SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
				SCM_STOP_REQUESTED.store(true, Ordering::Release);
				report_service_status(SERVICE_STOP_PENDING, NO_ERROR, 10_000);
				NO_ERROR
			}
			SERVICE_CONTROL_INTERROGATE => NO_ERROR,
			_ => ERROR_CALL_NOT_IMPLEMENTED,
		}
	}

	/// SCM-invoked service entrypoint that runs the core service loop until stopped.
	unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
		let service_name = wide_null(WINDOWS_SERVICE_NAME);
		let handle = RegisterServiceCtrlHandlerExW(
			service_name.as_ptr(),
			service_control_handler,
			std::ptr::null_mut(),
		);
		if handle == 0 {
			return;
		}
		SCM_STATUS_HANDLE.store(handle, Ordering::Release);
		SCM_STOP_REQUESTED.store(false, Ordering::Release);
		report_service_status(SERVICE_START_PENDING, NO_ERROR, 15_000);

		let exit_code = crate::background_service::run_until_stopped(
			&SCM_STOP_REQUESTED,
			|| report_service_status(SERVICE_RUNNING, NO_ERROR, 0),
		);

		report_service_status(SERVICE_STOPPED, exit_code, 0);
	}

	/// Attempts to connect to Windows SCM. Returns `true` if the process ran as
	/// an OS-managed Windows Service, or `false` if launched normally by a user.
	pub fn try_run_as_windows_service() -> bool {
		let mut service_name = wide_null(WINDOWS_SERVICE_NAME);
		let table = [
			ServiceTableEntryW {
				service_name: service_name.as_mut_ptr(),
				service_proc: Some(service_main),
			},
			ServiceTableEntryW {
				service_name: std::ptr::null_mut(),
				service_proc: None,
			},
		];

		let dispatched = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
		if dispatched != 0 {
			return true;
		}

		let error = unsafe { GetLastError() };
		if error == ERROR_FAILED_SERVICE_CONTROLLER_CONNECT {
			return false;
		}

		eprintln!("GitManager core service: Windows SCM dispatcher returned error {error}; falling back to interactive user mode.");
		false
	}
}

#[cfg(windows)]
pub use imp::try_run_as_windows_service;

#[cfg(not(windows))]
pub fn try_run_as_windows_service() -> bool {
	false
}
