//! DEV-V3-02: ordinary tests contain only non-mutating ownership checks on
//! self-created empty directories. Real admission mutations require explicit
//! feature opt-in AND a producer-owned copied-export pair, never a Git worktree.
// BEGIN standalone ownership guard (stdlib only; extracted for short unit checks)
mod ownership {
    use std::os::unix::fs::MetadataExt;
    use std::{
        fs,
        path::{Component, Path, PathBuf},
    };

    pub const MARKER: &str = ".qn-admission-owner";
    pub const LOCATOR: &str = ".quantnight/taskctl-selection.json";
    #[derive(Clone, Debug)]
    pub struct Owned {
        pub root: PathBuf,
        pub frontend: PathBuf,
        pub backend: PathBuf,
        token: String,
        marker: String,
    }
    // Reject symlinks at EVERY component, including dangling final symlinks.
    // Existing regular files must not alias user files through hard links either.
    pub fn plain(path: &Path) -> Result<(), String> {
        if !path.is_absolute() {
            return Err("absolute path required".into());
        }
        let mut current = PathBuf::new();
        for component in path.components() {
            if !matches!(component, Component::RootDir | Component::Normal(_)) {
                return Err("non-normal path refused".into());
            }
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(m) if m.file_type().is_symlink() || (m.is_file() && m.nlink() != 1) => {
                    return Err(format!("symlink/hardlink refused: {}", current.display()))
                }
                Ok(m) if !m.is_dir() && !m.is_file() => return Err("special file refused".into()),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(())
    }
    fn directory(path: &Path) -> Result<fs::Metadata, String> {
        plain(path)?;
        let m = fs::metadata(path).map_err(|e| e.to_string())?;
        if !m.is_dir() || path.canonicalize().map_err(|e| e.to_string())? != path {
            return Err("canonical directory required".into());
        }
        Ok(m)
    }
    pub fn marker_text(
        root: &Path,
        frontend: &Path,
        backend: &Path,
        token: &str,
    ) -> Result<String, String> {
        let mut text = format!("quantnight-disposable-admission-v1\n{token}\n");
        for path in [root, frontend, backend] {
            let m = directory(path)?;
            let name = path.to_str().ok_or("UTF-8 path required")?;
            if name.contains(['\n', '\r']) {
                return Err("newline path refused".into());
            }
            text.push_str(&format!("{name}\n{}:{}\n", m.dev(), m.ino()));
        }
        Ok(text)
    }
    impl Owned {
        pub fn open(
            root: PathBuf,
            frontend: PathBuf,
            backend: PathBuf,
            token: String,
        ) -> Result<Self, String> {
            let marker = marker_text(&root, &frontend, &backend, &token)?;
            let owned = Self {
                root,
                frontend,
                backend,
                token,
                marker,
            };
            owned.validate()?;
            // Enrollment is necessary, never sufficient for ownership.
            for root in [&owned.frontend, &owned.backend] {
                let path = root.join(LOCATOR);
                plain(&path)?;
                if !path.is_file() {
                    return Err("both exports must already be enrolled".into());
                }
            }
            Ok(owned)
        }
        pub fn validate(&self) -> Result<(), String> {
            if self.token.len() != 32 || !self.token.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("explicit 128-bit ownership token required".into());
            }
            if self.frontend == self.backend
                || self.frontend.starts_with(&self.backend)
                || self.backend.starts_with(&self.frontend)
            {
                return Err("frontend/backend roots overlap".into());
            }
            if self.frontend != self.root.join("frontend_source")
                || self.backend != self.root.join("backend_source")
            {
                return Err("roots must be the two direct copied-export children".into());
            }
            let m = directory(&self.root)?;
            unsafe extern "C" {
                fn getuid() -> u32;
            }
            // SAFETY: getuid has no arguments and no memory preconditions.
            let uid = unsafe { getuid() };
            if m.uid() != uid || m.mode() & 0o077 != 0 {
                return Err("runtime fixture root must be owned and private (0700)".into());
            }
            for path in [&self.root, &self.frontend, &self.backend] {
                let m = directory(path)?;
                if m.uid() != uid || m.mode() & 0o022 != 0 {
                    return Err("unowned/writable root refused".into());
                }
                if fs::symlink_metadata(path.join(".git")).is_ok() {
                    return Err("Git checkout/worktree is not disposable; copy an export".into());
                }
            }
            let path = self.root.join(MARKER);
            plain(&path)?;
            let m = fs::metadata(&path).map_err(|e| format!("ownership marker required: {e}"))?;
            if !m.is_file() || m.uid() != uid || m.mode() & 0o077 != 0 {
                return Err("private owned marker required".into());
            }
            if marker_text(&self.root, &self.frontend, &self.backend, &self.token)? != self.marker
                || fs::read_to_string(path).map_err(|e| e.to_string())? != self.marker
            {
                return Err(
                    "ownership marker must bind exact paths AND directory inodes AND token".into(),
                );
            }
            Ok(())
        }
        pub fn path(&self, path: &Path) -> Result<(), String> {
            self.validate()?;
            if path == self.root || !path.starts_with(&self.root) || path == self.root.join(MARKER)
            {
                return Err("mutation outside owned runtime fixture refused".into());
            }
            plain(path)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Fixture {
            root: PathBuf,
            front: PathBuf,
            back: PathBuf,
            token: String,
        }
        impl Fixture {
            fn new() -> Self {
                let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                    "qn-guard-{}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                fs::create_dir(&root).unwrap();
                fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
                let front = root.join("frontend_source");
                let back = root.join("backend_source");
                fs::create_dir(&front).unwrap();
                fs::create_dir(&back).unwrap();
                Self {
                    root,
                    front,
                    back,
                    token: "0123456789abcdef0123456789abcdef".into(),
                }
            }
            fn mark(&self) {
                fs::write(
                    self.root.join(MARKER),
                    marker_text(&self.root, &self.front, &self.back, &self.token).unwrap(),
                )
                .unwrap();
                fs::set_permissions(self.root.join(MARKER), fs::Permissions::from_mode(0o600))
                    .unwrap();
            }
            fn enroll(&self) {
                for root in [&self.front, &self.back] {
                    fs::create_dir_all(root.join(".quantnight")).unwrap();
                    fs::write(root.join(LOCATOR), "guard-only enrollment sentinel").unwrap();
                }
            }
            fn open(&self) -> Result<Owned, String> {
                Owned::open(
                    self.root.clone(),
                    self.front.clone(),
                    self.back.clone(),
                    self.token.clone(),
                )
            }
            fn rejected_before_write(&self) {
                let result = self.open().and_then(|owned| {
                    owned.path(&self.front.join("MUST-NOT-WRITE"))?;
                    fs::write(self.front.join("MUST-NOT-WRITE"), "bad").map_err(|e| e.to_string())
                });
                assert!(result.is_err());
                assert!(!self.front.join("MUST-NOT-WRITE").exists());
            }
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.root).unwrap();
            }
        }
        #[test]
        fn unmarked_and_unenrolled_are_rejected_before_writes() {
            let f = Fixture::new();
            f.rejected_before_write();
            f.mark();
            f.rejected_before_write();
            f.enroll();
            f.open().unwrap();
            fs::remove_file(f.back.join(LOCATOR)).unwrap();
            f.rejected_before_write();
        }
        #[test]
        fn git_checkout_and_worktree_are_never_disposable() {
            let f = Fixture::new();
            f.mark();
            f.enroll();
            fs::write(f.front.join(".git"), "gitdir: unrelated").unwrap();
            f.rejected_before_write();
            fs::remove_file(f.front.join(".git")).unwrap();
            fs::create_dir(f.back.join(".git")).unwrap();
            f.rejected_before_write();
        }
        #[test]
        fn real_manifest_root_is_rejected_read_only() {
            let real = PathBuf::from(
                option_env!("CARGO_MANIFEST_DIR")
                    .expect("guard harness supplies actual manifest root"),
            )
            .parent()
            .unwrap()
            .to_owned();
            let f = Fixture::new();
            f.mark();
            f.enroll();
            assert!(Owned::open(f.root.clone(), real, f.back.clone(), f.token.clone()).is_err());
        }
        #[test]
        fn same_overlapping_conflicting_roots_and_tokens_are_rejected() {
            let f = Fixture::new();
            f.mark();
            f.enroll();
            for back in [f.front.clone(), f.root.clone(), f.front.join(".quantnight")] {
                assert!(
                    Owned::open(f.root.clone(), f.front.clone(), back, f.token.clone()).is_err()
                );
            }
            let other = Fixture::new();
            other.mark();
            other.enroll();
            assert!(Owned::open(
                f.root.clone(),
                f.front.clone(),
                other.back.clone(),
                f.token.clone()
            )
            .is_err());
            assert!(Owned::open(
                f.root.clone(),
                f.front.clone(),
                f.back.clone(),
                "ffffffffffffffffffffffffffffffff".into()
            )
            .is_err());
        }
        #[test]
        fn marker_is_bound_to_full_paths_and_inodes_not_basenames() {
            let a = Fixture::new();
            a.mark();
            a.enroll();
            let b = Fixture::new();
            b.mark();
            b.enroll();
            fs::copy(a.root.join(MARKER), b.root.join(MARKER)).unwrap();
            b.rejected_before_write();
            fs::rename(a.back.clone(), a.root.join("old-backend")).unwrap();
            fs::create_dir(&a.back).unwrap();
            a.rejected_before_write();
        }
        #[test]
        fn symlinks_and_hardlinks_are_refused_including_venv_and_restore() {
            let f = Fixture::new();
            f.mark();
            f.enroll();
            let owned = f.open().unwrap();
            let outside = Fixture::new();
            let victim = outside.root.join("python");
            fs::write(&victim, "untouched").unwrap();
            symlink(&outside.root, f.back.join(".venv")).unwrap();
            assert!(owned.path(&f.back.join(".venv/bin/python")).is_err());
            symlink(&victim, f.front.join("restore-target")).unwrap();
            assert!(owned.path(&f.front.join("restore-target")).is_err());
            fs::hard_link(&victim, f.front.join("hardlink")).unwrap();
            assert!(owned.path(&f.front.join("hardlink")).is_err());
            symlink(f.root.join("missing"), f.front.join("dangling")).unwrap();
            assert!(owned.path(&f.front.join("dangling")).is_err());
            fs::remove_file(f.back.join(LOCATOR)).unwrap();
            symlink(&victim, f.back.join(LOCATOR)).unwrap();
            f.rejected_before_write();
            assert_eq!(fs::read_to_string(victim).unwrap(), "untouched");
        }
        #[test]
        fn symlink_root_and_marker_are_rejected_before_writes() {
            let f = Fixture::new();
            f.mark();
            f.enroll();
            let other = Fixture::new();
            let alias = other.root.join("alias");
            symlink(&f.root, &alias).unwrap();
            assert!(Owned::open(
                alias.clone(),
                alias.join("frontend_source"),
                alias.join("backend_source"),
                f.token.clone()
            )
            .is_err());
            let marker = other.root.join("marker");
            fs::rename(f.root.join(MARKER), &marker).unwrap();
            symlink(&marker, f.root.join(MARKER)).unwrap();
            f.rejected_before_write();
        }
        #[test]
        fn lost_marker_blocks_later_mutation_and_restore() {
            let f = Fixture::new();
            f.mark();
            f.enroll();
            let owned = f.open().unwrap();
            fs::remove_file(f.root.join(MARKER)).unwrap();
            assert!(owned.path(&f.front.join("restore-target")).is_err());
        }
    }
}
// END standalone ownership guard

#[cfg(feature = "disposable-admission-fixture")]
mod fixtures {
    // Composed production admission paths, never Tauri/window/real DB. The native
    // test executable is a bound host fixture; only embedded build metadata and
    // the original lowest Mongo/Python I/O seams are synthetic.
    use super::super::*;
    use super::ownership::{self, Owned};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Child, Command, Stdio};

    // Every fixture mutation and restoration rechecks the immutable root/marker
    // binding and refuses linked paths. This is cooperative owned-test isolation,
    // not a sandbox against an adversarial same-UID filesystem replacer.
    impl Owned {
        fn write(&self, path: &Path, bytes: impl AsRef<[u8]>) {
            self.path(path).unwrap();
            fs::write(path, bytes).unwrap();
        }
        fn remove(&self, path: &Path) {
            self.path(path).unwrap();
            fs::remove_file(path).unwrap();
        }
        fn mkdir(&self, path: &Path) {
            self.path(path).unwrap();
            fs::create_dir_all(path).unwrap();
        }
        fn copy(&self, source: &Path, dest: &Path) {
            self.path(dest).unwrap();
            fs::copy(source, dest).unwrap();
        }
    }
    struct Restore {
        owned: Owned,
        path: PathBuf,
        original: Option<(Vec<u8>, std::time::SystemTime)>,
    }
    impl Restore {
        fn capture(owned: &Owned, path: &Path) -> Self {
            owned.path(path).unwrap();
            let original = path.exists().then(|| {
                (
                    fs::read(path).unwrap(),
                    path.metadata().unwrap().modified().unwrap(),
                )
            });
            Self {
                owned: owned.clone(),
                path: path.to_owned(),
                original,
            }
        }
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Err(error) = self.owned.path(&self.path) {
                if std::thread::panicking() {
                    eprintln!("restore REFUSED: {error}");
                    return;
                }
                panic!("restore REFUSED: {error}");
            }
            if let Some((bytes, modified)) = &self.original {
                self.owned.write(&self.path, bytes);
                self.owned.path(&self.path).unwrap();
                File::options()
                    .write(true)
                    .open(&self.path)
                    .unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(*modified))
                    .unwrap();
            } else if self.path.exists() {
                self.owned.remove(&self.path);
            }
        }
    }
    struct Directory {
        path: PathBuf,
        owned: Owned,
    }
    impl Directory {
        fn new(owned: &Owned, label: &str) -> Self {
            let p = owned.root.join(format!(
                "runtime-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            owned.path(&p).unwrap();
            fs::create_dir(&p).unwrap();
            Self {
                path: p,
                owned: owned.clone(),
            }
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            if self.owned.path(&self.path).is_ok() {
                let _ = fs::remove_dir_all(&self.path);
            }
        }
    }
    fn owned_from_environment(build: &BuildIdentity) -> Owned {
        let root = std::env::var_os("QN_ADMISSION_FIXTURE_ROOT")
            .expect("explicit disposable runtime fixture root required");
        let token =
            std::env::var("QN_ADMISSION_FIXTURE_TOKEN").expect("explicit ownership token required");
        Owned::open(
            root.into(),
            build.binding.frontend_root.clone(),
            build.binding.backend_root.clone(),
            token,
        )
        .expect("refusing admission fixture outside explicitly owned copied exports")
    }
    fn preflight(owned: &Owned, build: &BuildIdentity) {
        owned.validate().unwrap();
        assert_eq!(
            Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap(),
            owned.frontend
        );
        owned.path(&build.binding_path).unwrap();
        owned.path(&build.binding.executable).unwrap();
        assert_eq!(
            current_binding(build).unwrap(),
            build.binding,
            "both enrollments must match embedded identity"
        );
        // Check original production-owned output paths before any child can write.
        for root in [&owned.frontend, &owned.backend] {
            let directory = root.join(".quantnight");
            owned.path(&directory).unwrap();
            for entry in fs::read_dir(directory).unwrap() {
                owned.path(&entry.unwrap().path()).unwrap();
            }
        }
        for relative in [
            "src-tauri/capabilities/default.json",
            "src-tauri/capabilities/fixture-membership.json",
            "src-tauri/capabilities/fixture-membership.toml",
            "src-tauri/capabilities/fixture-membership.json5",
            "src-tauri/gen/schemas/fixture-output.json",
        ] {
            owned.path(&owned.frontend.join(relative)).unwrap();
        }
        owned.path(&owned.backend.join(".venv")).unwrap();
        assert!(
            fs::symlink_metadata(owned.backend.join(".venv")).is_err(),
            "fixture must not touch any preexisting interpreter"
        );
    }
    // One original fixture entry + a nonblocking OS flock on the already-existing
    // owner marker: default Rust test threads AND separate test processes cannot race.
    // Never create/unlink a lock file, and keep the descriptor through all restores.
    #[test]
    fn owned_disposable_admission_fixture() {
        use std::os::fd::AsRawFd;
        let build = embedded().unwrap();
        let owned = owned_from_environment(&build);
        let lock = File::open(owned.root.join(ownership::MARKER)).unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0,
            "another producer owns this disposable admission fixture"
        );
        preflight(&owned, &build);
        actual_debug_host_closed_gui_capabilities_and_generated_outputs_before_bootstrap(&owned);
        composed_bound_host_gui_owner_matching_unready_conflict_death_and_disk_replacement(&owned);
        drop(lock);
    }
    struct Owner {
        child: Child,
        lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
    }
    impl Owner {
        fn expect(&mut self, token: &str) {
            assert!(
                self.lines.by_ref().any(|s| s.unwrap().contains(token)),
                "owner did not emit {token}"
            );
        }
        fn send(&mut self, text: &str) {
            writeln!(self.child.stdin.as_mut().unwrap(), "{text}").unwrap();
            self.child.stdin.as_mut().unwrap().flush().unwrap();
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    fn locator(owned: &Owned, root: &Path, binding: &Path) {
        let p = root.join(crate::dev_identity_inputs::LOCATOR);
        owned.mkdir(p.parent().unwrap());
        owned.write(
            &p,
            serde_json::to_vec(
                &serde_json::json!({"schema_version":1,"mode":"dev","binding":binding}),
            )
            .unwrap(),
        );
    }
    fn fake_python(owned: &Owned, root: &Path) -> Directory {
        let p = root.join(".venv/bin/python");
        owned.mkdir(p.parent().unwrap());
        owned.path(&p).unwrap();
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&p)
            .unwrap();
        file.write_all(b"#!/bin/sh\nprintf 'original bootstrap reached\\n' > \"$QN_BOOTSTRAP_SENTINEL\"\nexit 41\n").unwrap();
        owned.path(&p).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
        Directory {
            path: root.join(".venv"),
            owned: owned.clone(),
        }
    }
    fn cli(
        owned: &Owned,
        binary: &Path,
        build: Option<&BuildIdentity>,
        backend: &Path,
        sentinel: &Path,
    ) -> std::process::Output {
        owned.path(sentinel).unwrap();
        let _ = fs::remove_file(sentinel);
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("QN_ADMISSION_FIXTURE_ROOT", &owned.root)
            .env(
                "QN_ADMISSION_FIXTURE_TOKEN",
                std::env::var("QN_ADMISSION_FIXTURE_TOKEN").unwrap(),
            )
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", backend)
            .env("QN_BOOTSTRAP_SENTINEL", sentinel);
        if let Some(build) = build {
            command
                .args([
                    "--exact",
                    "dev_identity::admission_chain_tests::fixtures::admission_child",
                    "--nocapture",
                ])
                .env(
                    "QN_TEST_EMBEDDED_IDENTITY",
                    serde_json::to_string(build).unwrap(),
                )
                .env("QN_ADMISSION_ROLE", "cli");
        } else {
            command
                .arg("taskctl")
                .arg("--backend-root")
                .arg(backend)
                .args(["status", "--name", "fixture"]);
        }
        command.output().unwrap()
    }
    fn refused(output: &std::process::Output, marker: &Path, reason: &str) {
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !marker.exists(),
            "admission failure reached original bootstrap I/O"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(reason),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    fn reached(output: &std::process::Output, marker: &Path) {
        assert_eq!(
            output.status.code(),
            Some(1),
            "bootstrap sentinel must stop before DB"
        );
        assert_eq!(
            fs::read_to_string(marker).unwrap(),
            "original bootstrap reached\n"
        );
    }

    #[test]
    fn admission_child() {
        let Ok(role) = std::env::var("QN_ADMISSION_ROLE") else {
            return;
        };
        let build = embedded().unwrap();
        let owned = owned_from_environment(&build);
        owned.path(&build.binding_path).unwrap();
        owned.path(&build.binding.executable).unwrap();
        for entry in fs::read_dir(owned.backend.join(".quantnight")).unwrap() {
            owned.path(&entry.unwrap().path()).unwrap();
        }
        if role == "cli" {
            let args = [
                "--backend-root".into(),
                build.binding.backend_root.as_os_str().to_owned(),
                "status".into(),
                "--name".into(),
                "fixture".into(),
            ];
            std::process::exit(quantnight_taskctl::run_os_with_admission(args, admit_cli));
        }
        if let Err(error) = initialize_gui() {
            println!("GUI_INIT_REJECT:{error}");
            std::process::exit(43);
        }
        // Original AppConfig parser, synthetic TOML, original lowest client-slot
        // factory seam. No Mongo Client is constructed and no Tauri Builder runs.
        let config = quantnight_task_core::config::load_config(
            &build.binding.backend_root.join("gui-fixture.toml"),
        )
        .unwrap();
        if let Err(error) = verify_gui_root(Path::new(&config.env.working_dir)) {
            println!("GUI_ROOT_REJECT:{error}");
            std::process::exit(45);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use quantnight_task_core::mongo_manager::local_config_test_support::FakeClients;
            let clients = FakeClients::<bool>::from_config(&config);
            assert!(**clients
                .client_with(false, |_| async { Ok(std::sync::Arc::new(true)) })
                .await
                .unwrap());
            assert!(**clients
                .client_with(true, |_| async { Ok(std::sync::Arc::new(true)) })
                .await
                .unwrap());
        });
        println!("GUI_UNREADY");
        std::io::stdout().flush().unwrap();
        for line in std::io::stdin().lock().lines() {
            owned.validate().unwrap();
            for entry in fs::read_dir(owned.backend.join(".quantnight")).unwrap() {
                owned.path(&entry.unwrap().path()).unwrap();
            }
            match line.unwrap().as_str() {
                "ready" => {
                    ready().unwrap();
                    println!("GUI_READY");
                }
                "check" => println!("GUI_ADMISSION:{:?}", admit_gui()),
                "exit" => break,
                _ => panic!("unknown owned fixture instruction"),
            }
            std::io::stdout().flush().unwrap();
        }
    }

    fn actual_debug_host_closed_gui_capabilities_and_generated_outputs_before_bootstrap(
        owned: &Owned,
    ) {
        let build = embedded().unwrap();
        let backend = &build.binding.backend_root;
        let _venv = fake_python(owned, backend);
        let temp = Directory::new(owned, "actual-host");
        let marker = temp.path.join("bootstrap");
        let run = || cli(owned, &build.binding.executable, None, backend, &marker);
        reached(&run(), &marker); // real bound GUI executable -> shared CLI -> fresh -> original bootstrap
                                  // Fresh Pi-like environment through the ACTUAL original backend shim, not
                                  // an exec-success stub: durable locator -> same bound host -> admission.
        owned.path(&marker).unwrap();
        let _ = fs::remove_file(&marker);
        let python =
            std::env::var_os("QUANTNIGHT_TEST_PYTHON").expect("existing fixture interpreter");
        let shim = backend.join(".agents/skills/manage-quantnight-frontend/scripts/taskctl.py");
        let through_shim = Command::new(python)
            .arg("-B")
            .arg(shim)
            .args(["status", "--name", "fixture"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &temp.path)
            .env("QN_BOOTSTRAP_SENTINEL", &marker)
            .output()
            .unwrap();
        reached(&through_shim, &marker);

        let capdir = build.binding.frontend_root.join("src-tauri/capabilities");
        for extension in ["json", "toml", "json5"] {
            let path = capdir.join(format!("fixture-membership.{extension}"));
            assert!(!path.exists());
            let guard = Restore::capture(owned, &path);
            owned.write(&path, "fixture source membership");
            refused(&run(), &marker, "inputs changed");
            drop(guard);
            reached(&run(), &marker);
        }
        let existing = capdir.join("default.json");
        {
            let _restore = Restore::capture(owned, &existing);
            let mut data = fs::read(&existing).unwrap();
            data.push(b' ');
            owned.write(&existing, data);
            refused(&run(), &marker, "inputs changed");
        }
        {
            let _restore = Restore::capture(owned, &existing);
            owned.remove(&existing);
            refused(&run(), &marker, "inputs changed");
        }
        reached(&run(), &marker);
        let schema = build
            .binding
            .frontend_root
            .join("src-tauri/gen/schemas/fixture-output.json");
        {
            let _restore = Restore::capture(owned, &schema);
            owned.mkdir(schema.parent().unwrap());
            owned.write(&schema, "generated only");
            reached(&run(), &marker);
        }
        refused(
            &cli(owned, &build.binding.executable, None, &temp.path, &marker),
            &marker,
            "shim backend root differs",
        );
        let frontloc = build
            .binding
            .frontend_root
            .join(crate::dev_identity_inputs::LOCATOR);
        {
            let _restore = Restore::capture(owned, &frontloc);
            owned.remove(&frontloc);
            refused(&run(), &marker, "enrollment unavailable");
        }
        reached(&run(), &marker);
    }

    fn composed_bound_host_gui_owner_matching_unready_conflict_death_and_disk_replacement(
        owned: &Owned,
    ) {
        let mut build = embedded().unwrap();
        let temp = Directory::new(owned, "composed-chain");
        let backend = owned.backend.clone();
        let executable = temp.path.join("bound-test-host");
        owned.copy(&std::env::current_exe().unwrap(), &executable);
        build.binding.backend_root = backend.clone();
        build.binding.executable = executable.clone();
        build.binding_path = temp.path.join("binding.json");
        owned.write(
            &build.binding_path,
            serde_json::to_vec(&build.binding).unwrap(),
        );
        let frontloc = build
            .binding
            .frontend_root
            .join(crate::dev_identity_inputs::LOCATOR);
        let _restore_locator = Restore::capture(owned, &frontloc);
        let _restore_backend_locator =
            Restore::capture(owned, &backend.join(crate::dev_identity_inputs::LOCATOR));
        let _restore_ready = Restore::capture(owned, &record_path(&backend));
        locator(owned, &build.binding.frontend_root, &build.binding_path);
        locator(owned, &backend, &build.binding_path);
        let _venv = fake_python(owned, &backend);
        let config = |working: &Path| {
            format!("[env]\nworking_dir = {:?}\n[python.scripts]\n[bash.scripts]\n[mongodb]\nlocal_uri=\"synthetic-local\"\nremote_uri=\"synthetic-remote\"\n[mongodb.databases]\nmission=\"fixture_mission\"\nsimulation=\"fixture_simulation\"\nalpha=\"fixture_alpha\"\n",working.to_str().unwrap())
        };
        let configpath = backend.join("gui-fixture.toml");
        let _restore_config = Restore::capture(owned, &configpath);
        owned.write(&configpath, config(&temp.path));
        let spawn_gui = || {
            let mut child = Command::new(&executable)
                .args([
                    "--exact",
                    "dev_identity::admission_chain_tests::fixtures::admission_child",
                    "--nocapture",
                ])
                .env_clear()
                .env("QN_ADMISSION_FIXTURE_ROOT", &owned.root)
                .env(
                    "QN_ADMISSION_FIXTURE_TOKEN",
                    std::env::var("QN_ADMISSION_FIXTURE_TOKEN").unwrap(),
                )
                .env("PATH", "/usr/bin:/bin")
                .env(
                    "QN_TEST_EMBEDDED_IDENTITY",
                    serde_json::to_string(&build).unwrap(),
                )
                .env("QN_ADMISSION_ROLE", "gui")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let lines = BufReader::new(child.stdout.take().unwrap()).lines();
            Owner { child, lines }
        };
        {
            let mut wrong = spawn_gui();
            wrong.expect("GUI_ROOT_REJECT");
            assert_eq!(wrong.child.wait().unwrap().code(), Some(45));
        }
        owned.write(&configpath, config(&backend));
        let marker = temp.path.join("bootstrap");
        let run = || cli(owned, &executable, Some(&build), &backend, &marker);
        reached(&run(), &marker); // no GUI/free lock
        let mut owner = spawn_gui();
        owner.expect("GUI_UNREADY");
        refused(&run(), &marker, "different/unready");
        {
            let mut competing = spawn_gui();
            competing.expect("GUI_INIT_REJECT");
            assert_eq!(competing.child.wait().unwrap().code(), Some(43));
        }
        owner.send("ready");
        owner.expect("GUI_READY");
        reached(&run(), &marker);
        let record = record_path(&backend);
        let original = fs::read(&record).unwrap();
        let mut mismatch: serde_json::Value = serde_json::from_slice(&original).unwrap();
        mismatch["source"] = "different-generation".into();
        owned.write(&record, serde_json::to_vec(&mismatch).unwrap());
        refused(&run(), &marker, "different/unready");
        owner.send("ready");
        owner.expect("GUI_READY");
        // Replace a real signed native image at its pathname. Only LC_UUID changes
        // in this compiled test fixture, then codesign validates the new image.
        let replacement = temp.path.join("replacement");
        owned.copy(&executable, &replacement);
        let mut data = fs::read(&replacement).unwrap();
        let n = u32::from_le_bytes(data[16..20].try_into().unwrap());
        let mut off = 32usize;
        let mut changed = false;
        for _ in 0..n {
            let kind = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            let len = u32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()) as usize;
            if kind == 0x1b {
                data[off + 8] ^= 0x5a;
                changed = true;
                break;
            }
            off += len;
        }
        assert!(changed);
        owned.write(&replacement, data);
        owned.path(&replacement).unwrap();
        assert!(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&replacement)
            .status()
            .unwrap()
            .success());
        assert_ne!(
            dev_image::disk(&replacement).unwrap(),
            dev_image::disk(&executable).unwrap()
        );
        owned.path(&replacement).unwrap();
        owned.path(&executable).unwrap();
        fs::rename(replacement, &executable).unwrap();
        owner.send("check");
        owner.expect("loaded dev image differs");
        refused(&run(), &marker, "different/unready"); // new loaded host vs live old GUI record
        owner.child.kill().unwrap();
        owner.child.wait().unwrap(); // actual crash: OS releases lock, stale ready persists
        reached(&run(), &marker); // fresh replacement + dead/free GUI admitted to lowest bootstrap sentinel
    }
}
