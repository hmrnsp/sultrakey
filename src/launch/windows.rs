//! Windows (developer laptops): no exec. The application runs as a child in a Job Object
//! that kills it when sultrakey goes away (a closed terminal must not leave a node process
//! holding the port), and Ctrl+C is left to the application.

use std::env;
use std::ffi::{OsStr, OsString, c_void};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::ptr;

use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

use super::{ChildEnv, start_fail};

pub fn spawn(program: &OsString, args: &[OsString], env: &ChildEnv) -> Result<i32> {
    let resolved = resolve(program, env::var_os("PATH"), env::var_os("PATHEXT"));
    let mut command = Command::new(&resolved);
    command.args(args);
    for name in &env.remove {
        command.env_remove(name);
    }
    for (key, value) in &env.set {
        command.env(key, value.expose());
    }
    let mut child = command.spawn().map_err(|err| start_fail(program, &err))?;
    kill_with_us(&child);

    // Ctrl+C goes to every process on the console. Ignore it here so only the application
    // reacts and we stay alive to pass on its exit code. Called after spawn on purpose:
    // the ignore flag is inherited by children created later.
    // SAFETY: a null handler with TRUE only toggles a per-process flag.
    unsafe {
        SetConsoleCtrlHandler(None, 1);
    }

    let status = child.wait().context("gagal menunggu aplikasi selesai")?;
    Ok(status.code().unwrap_or(1))
}

/// Puts the child in a Job Object that is closed (killing the child and its children)
/// when this process exits for any reason. Best effort: without it, things still run.
fn kill_with_us(child: &Child) {
    // SAFETY: plain Win32 calls with valid arguments; the job handle is intentionally
    // never closed, so it lives exactly as long as this process.
    unsafe {
        let job: HANDLE = CreateJobObjectW(ptr::null(), ptr::null());
        if job.is_null() {
            return;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let set = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if set != 0 {
            AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE);
        }
    }
}

/// Finds `program` like cmd.exe does, using `PATHEXT`: `npm`, `npx`, `pnpm`, `yarn` and
/// `mvnw` are `.cmd` files, which `Command` alone would not find. Returns the name
/// unchanged when nothing matches (spawning it then reports "not found").
pub fn resolve(program: &OsStr, path: Option<OsString>, pathext: Option<OsString>) -> PathBuf {
    let name = Path::new(program);
    let exts: Vec<String> = pathext
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.starts_with('.'))
        .map(str::to_string)
        .collect();
    let candidates = |base: &Path| {
        let mut list = Vec::new();
        if base.extension().is_some() {
            list.push(base.to_path_buf());
        }
        for ext in &exts {
            let mut with = base.as_os_str().to_owned();
            with.push(ext);
            list.push(PathBuf::from(with));
        }
        list
    };
    let has_dir = name.components().count() > 1 || name.is_absolute();
    if has_dir {
        return candidates(name)
            .into_iter()
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| name.to_path_buf());
    }
    path.iter()
        .flat_map(env::split_paths)
        .flat_map(|dir| candidates(&dir.join(name)))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| name.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_cmd_files_through_pathext() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("fake.cmd"), "@echo off").unwrap();
        fs::write(bin.join("tool.exe"), "").unwrap();
        let path = Some(env::join_paths([dir.path(), &bin]).unwrap());
        let pathext = Some(OsString::from(".com;.exe;.bat;.cmd"));

        assert_eq!(
            resolve(OsStr::new("fake"), path.clone(), pathext.clone()),
            bin.join("fake.cmd")
        );
        assert_eq!(
            resolve(OsStr::new("tool"), path.clone(), pathext.clone()),
            bin.join("tool.exe")
        );
        assert_eq!(
            resolve(OsStr::new("fake.cmd"), path.clone(), pathext.clone()),
            bin.join("fake.cmd")
        );
        assert_eq!(
            resolve(OsStr::new("missing"), path.clone(), pathext.clone()),
            PathBuf::from("missing")
        );
        let direct = bin.join("fake");
        assert_eq!(
            resolve(direct.as_os_str(), None, pathext),
            bin.join("fake.cmd")
        );
    }
}
