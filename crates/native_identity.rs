//! macOS dev-host identity: compare LC_UUID in the mapped main image and disk.
//! A hash of current_exe() alone would identify only the replacement disk file.
use std::{fs::File, io::Read, path::Path};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity { pub pid: u32, pub started_seconds: u64, pub started_micros: u64 }
#[cfg(target_os = "macos")]
fn process_info(pid:u32)->Result<libc::proc_bsdinfo,String>{
    let mut info=std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();let size=std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let result=unsafe{libc::proc_pidinfo(pid as i32,libc::PROC_PIDTBSDINFO,0,info.as_mut_ptr().cast(),size)};
    if result!=size{return Err("dev GUI process identity is unavailable/stale".into());}
    let info=unsafe{info.assume_init()};if info.pbi_pid!=pid || info.pbi_status==5{return Err("dev GUI process is not live".into());}Ok(info)
}
#[cfg(target_os = "macos")]
fn identity_from_info(info:&libc::proc_bsdinfo)->ProcessIdentity{ProcessIdentity{pid:info.pbi_pid,started_seconds:info.pbi_start_tvsec,started_micros:info.pbi_start_tvusec}}
#[cfg(target_os = "macos")]
pub fn process_identity(pid:u32)->Result<ProcessIdentity,String>{process_info(pid).map(|info|identity_from_info(&info))}
#[cfg(not(target_os = "macos"))]
pub fn process_identity(_:u32)->Result<ProcessIdentity,String>{Err("dev host process identity requires macOS".into())}

/// Native attestation extends, but does not change, the original three-field ready owner schema.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ProcessAttestation {pub identity:ProcessIdentity,pub uid:u32,pub parent_pid:u32,pub executable:std::path::PathBuf}
#[cfg(target_os = "macos")]
fn executable_path(pid:u32)->Result<std::path::PathBuf,String>{
    use std::os::unix::ffi::OsStrExt;
    let mut bytes=vec![0_u8;libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length=unsafe{libc::proc_pidpath(pid as i32,bytes.as_mut_ptr().cast(),bytes.len() as u32)};
    if length<=0{return Err("native host executable path unavailable".into());}
    let end=bytes.iter().position(|byte|*byte==0).ok_or("invalid native executable path")?;
    Ok(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..end])))
}
#[cfg(target_os = "macos")]
pub fn attest_process(expected:&ProcessIdentity)->Result<ProcessAttestation,String>{
    let before=process_info(expected.pid)?;if identity_from_info(&before)!=*expected{return Err("native process birth changed/reused".into());}
    let executable=executable_path(expected.pid)?;
    let after=process_info(expected.pid)?;let final_executable=executable_path(expected.pid)?;
    if identity_from_info(&after)!=*expected || before.pbi_uid!=after.pbi_uid || before.pbi_ppid!=after.pbi_ppid || executable!=final_executable{return Err("native process identity/executable changed during attestation".into());}
    Ok(ProcessAttestation{identity:expected.clone(),uid:after.pbi_uid,parent_pid:after.pbi_ppid,executable})
}
#[cfg(not(target_os = "macos"))]
pub fn attest_process(_:&ProcessIdentity)->Result<ProcessAttestation,String>{Err("runtime process attestation requires macOS".into())}


const HEADER: usize = 32;
const MAX_COMMANDS: usize = 16 * 1024 * 1024;

fn layout(header: &[u8]) -> Result<(usize, usize), String> {
    if header.len() != HEADER || header[..4] != 0xfeedfacfu32.to_le_bytes() {
        return Err("dev host requires a thin little-endian 64-bit Mach-O image".into());
    }
    let n = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
    if size > MAX_COMMANDS || n > size / 8 { return Err("invalid Mach-O load commands".into()); }
    Ok((n, size))
}
fn uuid(n: usize, commands: &[u8]) -> Result<[u8; 16], String> {
    let mut offset = 0;
    let mut result = None;
    for _ in 0..n {
        let header = commands.get(offset..offset + 8).ok_or("truncated Mach-O command")?;
        let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
        let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        if size < 8 { return Err("invalid Mach-O command size".into()); }
        let command = commands.get(offset..offset.checked_add(size).ok_or("Mach-O overflow")?)
            .ok_or("truncated Mach-O command body")?;
        if kind == 0x1b {
            if size != 24 || result.is_some() { return Err("ambiguous Mach-O UUID".into()); }
            result = Some(command[8..24].try_into().unwrap());
        }
        offset += size;
    }
    if offset != commands.len() { return Err("invalid Mach-O command table length".into()); }
    result.ok_or_else(|| "dev host Mach-O UUID missing".into())
}
pub fn disk(path: &Path) -> Result<[u8; 16], String> {
    let mut file = File::open(path).map_err(|e| format!("dev host executable: {e}"))?;
    let mut header = [0; HEADER];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let (n, size) = layout(&header)?;
    let mut commands = vec![0; size];
    file.read_exact(&mut commands).map_err(|e| e.to_string())?;
    uuid(n, &commands)
}
#[cfg(target_os = "macos")]
pub fn loaded() -> Result<[u8; 16], String> {
    extern "C" { fn _dyld_get_image_header(index: u32) -> *const u8; }
    // SAFETY: dyld owns image zero (the running main executable) for process
    // lifetime. Read the mapped header/load-command region, not a pathname.
    unsafe {
        let ptr = _dyld_get_image_header(0);
        if ptr.is_null() { return Err("dyld main image unavailable".into()); }
        let (n, size) = layout(std::slice::from_raw_parts(ptr, HEADER))?;
        uuid(n, std::slice::from_raw_parts(ptr.add(HEADER), size))
    }
}
#[cfg(not(target_os = "macos"))]
pub fn loaded() -> Result<[u8; 16], String> {
    Err("development GUI parity currently requires macOS native image identity; standalone taskctl is unchanged".into())
}

pub fn ensure_current(path: &Path) -> Result<[u8; 16], String> {
    let image = loaded()?;
    if disk(path)? != image {
        return Err("loaded dev image differs from the on-disk host; fix and rerun `npm run tauri dev`".into());
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uuid_is_image_identity_not_a_disk_path_hash() {
        let mut command = Vec::from(0x1bu32.to_le_bytes());
        command.extend(24u32.to_le_bytes()); command.extend([7; 16]);
        assert_eq!(uuid(1, &command).unwrap(), [7; 16]);
        assert!(uuid(2, &command).is_err());
        let duplicate = [command.clone(), command.clone()].concat();
        assert!(uuid(2, &duplicate).is_err());
        command[4] = 0;
        assert!(uuid(1, &command).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn mapped_probe_child() {
        if std::env::var_os("QN_MAPPED_PROBE_CHILD").is_none() { return; }
        use std::io::Write;
        let path = std::env::current_exe().unwrap();
        assert_eq!(loaded().unwrap(), disk(&path).unwrap());
        println!("MAPPED_PROBE_READY"); std::io::stdout().flush().unwrap();
        let mut line = String::new(); std::io::stdin().read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "replace-complete");
        assert!(ensure_current(&path).unwrap_err().contains("loaded dev image differs"));
        println!("MAPPED_PROBE_REJECTED");
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn actual_mapped_old_process_rejects_new_disk_executable() {
        use std::io::{BufRead, Write};
        use std::process::{Command, Stdio};
        let root = std::env::temp_dir().join(format!("qn-mapped-image-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(root.clone());
        let old = root.join("host");
        std::fs::copy(std::env::current_exe().unwrap(), &old).unwrap();
        std::fs::write(root.join("new.rs"), "fn main() { println!(\"replacement-leaf\"); }").unwrap();
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        assert!(Command::new(rustc).arg(root.join("new.rs")).arg("-o").arg(root.join("new-host")).status().unwrap().success());
        assert_ne!(disk(&old).unwrap(), disk(&root.join("new-host")).unwrap());
        let child_test = format!("{}::mapped_probe_child", module_path!().split_once("::").unwrap().1);
        let child = Command::new(&old).args(["--exact", &child_test, "--nocapture"])
            .env("QN_MAPPED_PROBE_CHILD", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
        let mut child = OwnedChild(child);
        let mut lines = std::io::BufReader::new(child.0.stdout.take().unwrap()).lines();
        assert!(lines.by_ref().any(|line| line.unwrap().contains("MAPPED_PROBE_READY")));
        std::fs::rename(root.join("new-host"), &old).unwrap();
        child.0.stdin.take().unwrap().write_all(b"replace-complete\n").unwrap();
        assert!(lines.any(|line| line.unwrap().contains("MAPPED_PROBE_REJECTED")));
        assert!(child.0.wait().unwrap().success());
    }

    #[cfg(target_os="macos")]
    #[test]
    fn native_attestation_retains_original_birth_schema_and_checks_current_owner_executable(){
        let owner=process_identity(std::process::id()).unwrap();let attested=attest_process(&owner).unwrap();assert_eq!(attested.identity,owner);assert_eq!(attested.uid,unsafe{libc::geteuid()});
        assert_eq!(attested.executable.canonicalize().unwrap(),std::env::current_exe().unwrap().canonicalize().unwrap());
        let mut reused=owner.clone();reused.started_micros=reused.started_micros.wrapping_add(1);assert!(attest_process(&reused).is_err());
        let value=serde_json::to_value(&owner).unwrap();assert_eq!(value.as_object().unwrap().len(),3);
    }

}
