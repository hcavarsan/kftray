use std::fs;
use std::io::{
    ErrorKind,
    Write,
};
use std::path::Path;
use std::sync::Arc;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{
    Algorithm,
    HashAlg,
    PrivateKey,
    PublicKey,
};

use crate::consts::{
    KEY_COMMENT,
    KEY_RELATIVE_PATH,
};
use crate::error::SshError;

/// kftray's own ed25519 key, shared by every provider.
#[derive(Clone)]
pub struct KftrayKey {
    private: Arc<PrivateKey>,
}

impl std::fmt::Debug for KftrayKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KftrayKey")
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

impl KftrayKey {
    /// Loads `{data_dir}/ssh/id_ed25519`, creating it on first use. An
    /// existing file that cannot be read is an error and is never replaced.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, SshError> {
        let path = data_dir.join(KEY_RELATIVE_PATH);
        if path.exists() {
            return Self::read(&path);
        }
        if let Some(dir) = path.parent() {
            create_private_dir(dir)
                .map_err(|e| SshError::Key(format!("cannot create {}: {e}", dir.display())))?;
        }

        let mut private = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519)
            .map_err(|e| SshError::Key(format!("cannot generate a key: {e}")))?;
        private.set_comment(KEY_COMMENT);
        let pem = private
            .to_openssh(LineEnding::LF)
            .map_err(|e| SshError::Key(format!("cannot encode the key: {e}")))?;

        // Written in full under a name of its own, then linked into place:
        // another kftray starting at the same time sees either no key or a
        // complete one, and a key someone else published first is kept.
        let unique: String = private
            .fingerprint(HashAlg::Sha256)
            .as_bytes()
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let tmp = path.with_file_name(format!(".id_ed25519.{unique}.tmp"));
        let written = create_private_file(&tmp).and_then(|mut file| {
            file.write_all(pem.as_bytes())?;
            file.sync_all()
        });
        let published = written.and_then(|()| fs::hard_link(&tmp, &path));
        let _ = fs::remove_file(&tmp);
        match published {
            Ok(()) => {
                log::info!(
                    "[ssh] created {} ({})",
                    path.display(),
                    private.fingerprint(HashAlg::Sha256)
                );
                Ok(Self {
                    private: Arc::new(private),
                })
            }
            // Another kftray process created it first.
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Self::read(&path),
            Err(e) => Err(SshError::Key(format!(
                "cannot create {}: {e}",
                path.display()
            ))),
        }
    }

    fn read(path: &Path) -> Result<Self, SshError> {
        let private = PrivateKey::read_openssh_file(path)
            .map_err(|e| SshError::Key(format!("cannot read {}: {e}", path.display())))?;
        if private.is_encrypted() {
            return Err(SshError::Key(format!(
                "cannot read {}: the key is encrypted",
                path.display()
            )));
        }
        Ok(Self {
            private: Arc::new(private),
        })
    }

    pub fn private(&self) -> Arc<PrivateKey> {
        Arc::clone(&self.private)
    }

    pub fn public(&self) -> PublicKey {
        self.private.public_key().clone()
    }

    /// `ssh-ed25519 AAAA… kftray`.
    pub fn public_openssh(&self) -> String {
        // Encoding an in-memory ed25519 public key cannot fail.
        self.private.public_key().to_openssh().unwrap_or_default()
    }

    /// SHA-256 fingerprint, the only form of the key allowed in logs.
    pub fn fingerprint(&self) -> String {
        self.private.fingerprint(HashAlg::Sha256).to_string()
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    owner_only::create_dir(dir)
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    owner_only::create_file(path)
}

#[cfg(windows)]
mod owner_only {
    use std::ffi::c_void;
    use std::fs;
    use std::io::{
        Error,
        ErrorKind,
        Result,
    };
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use std::path::Path;

    use windows::Win32::Foundation::{
        CloseHandle,
        HANDLE,
        HLOCAL,
        LocalFree,
    };
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW,
        ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation,
        PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES,
        TOKEN_QUERY,
        TOKEN_USER,
        TokenUser,
    };
    use windows::Win32::Storage::FileSystem::{
        CREATE_NEW,
        CreateDirectoryW,
        CreateFileW,
        FILE_ATTRIBUTE_NORMAL,
        FILE_GENERIC_WRITE,
        FILE_SHARE_NONE,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess,
        OpenProcessToken,
    };
    use windows::core::{
        PCWSTR,
        PWSTR,
    };

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(Some(0)).collect()
    }

    pub(super) fn current_user_sid() -> Result<String> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
            let sid = token_user_sid(token);
            let _ = CloseHandle(token);
            sid
        }
    }

    unsafe fn token_user_sid(token: HANDLE) -> Result<String> {
        unsafe {
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast::<c_void>()),
                len,
                &mut len,
            )?;
            let user = &*buf.as_ptr().cast::<TOKEN_USER>();
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text)?;
            let sid = text.to_string();
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            sid.map_err(|e| Error::new(ErrorKind::InvalidData, e))
        }
    }

    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn owner_only() -> Result<Self> {
            let sddl = wide(format!("D:P(A;OICI;FA;;;{})", current_user_sid()?).as_ref());
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )?;
            }
            Ok(Self(descriptor))
        }

        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0.0,
                bInheritHandle: false.into(),
            }
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }

    pub(super) fn create_dir(dir: &Path) -> Result<()> {
        if dir.is_dir() {
            return Ok(());
        }
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)?;
        }
        let descriptor = Descriptor::owner_only()?;
        let attributes = descriptor.attributes();
        let name = wide(dir.as_os_str());
        match unsafe { CreateDirectoryW(PCWSTR(name.as_ptr()), Some(&attributes)) } {
            Err(e) if Error::from(e.clone()).kind() == ErrorKind::AlreadyExists => Ok(()),
            result => result.map_err(Error::from),
        }
    }

    pub(super) fn create_file(path: &Path) -> Result<fs::File> {
        let descriptor = Descriptor::owner_only()?;
        let attributes = descriptor.attributes();
        let name = wide(path.as_os_str());
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                FILE_GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                Some(&attributes),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }?;
        Ok(unsafe { fs::File::from_raw_handle(handle.0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_create_should_reload_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let created = KftrayKey::load_or_create(dir.path()).unwrap();
        let reloaded = KftrayKey::load_or_create(dir.path()).unwrap();
        assert_eq!(created.public_openssh(), reloaded.public_openssh());
    }

    #[test]
    fn load_or_create_should_give_every_concurrent_caller_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let barrier = std::sync::Barrier::new(16);
        let keys: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        KftrayKey::load_or_create(dir.path()).map(|key| key.public_openssh())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let first = keys[0].as_ref().unwrap();
        assert!(
            keys.iter().all(|key| key.as_ref().ok() == Some(first)),
            "{keys:?}"
        );
    }

    #[test]
    fn public_openssh_should_carry_the_kftray_comment() {
        let dir = tempfile::tempdir().unwrap();
        let key = KftrayKey::load_or_create(dir.path()).unwrap();
        assert!(key.public_openssh().starts_with("ssh-ed25519 "));
        assert!(key.public_openssh().ends_with(" kftray"));
    }

    #[cfg(unix)]
    #[test]
    fn load_or_create_should_write_the_key_with_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        KftrayKey::load_or_create(dir.path()).unwrap();
        let mode = fs::metadata(dir.path().join(KEY_RELATIVE_PATH))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(windows)]
    #[test]
    fn load_or_create_should_restrict_the_key_and_its_dir_to_the_current_user() {
        use std::os::windows::ffi::OsStrExt;

        use windows::Win32::Foundation::{
            HLOCAL,
            LocalFree,
        };
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            GetNamedSecurityInfoW,
            SDDL_REVISION_1,
            SE_FILE_OBJECT,
        };
        use windows::Win32::Security::{
            DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR,
        };
        use windows::core::{
            PCWSTR,
            PWSTR,
        };

        fn dacl(path: &Path) -> String {
            let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                let mut descriptor = PSECURITY_DESCRIPTOR::default();
                let status = GetNamedSecurityInfoW(
                    PCWSTR(name.as_ptr()),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    None,
                    None,
                    &mut descriptor,
                );
                assert!(status.is_ok(), "{status:?}");
                let mut text = PWSTR::null();
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    None,
                )
                .unwrap();
                let sddl = text.to_string().unwrap();
                let _ = LocalFree(Some(HLOCAL(text.0.cast())));
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
                sddl
            }
        }

        let dir = tempfile::tempdir().unwrap();
        KftrayKey::load_or_create(dir.path()).unwrap();
        let key = dir.path().join(KEY_RELATIVE_PATH);
        let sid = owner_only::current_user_sid().unwrap();
        for path in [key.parent().unwrap(), key.as_path()] {
            let sddl = dacl(path);
            assert!(sddl.starts_with("D:P"), "{sddl}");
            assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
            assert!(sddl.contains(&format!(";FA;;;{sid})")), "{sddl}");
        }
    }

    #[test]
    fn load_or_create_should_refuse_and_keep_a_corrupt_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_RELATIVE_PATH);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "garbage").unwrap();
        assert!(matches!(
            KftrayKey::load_or_create(dir.path()),
            Err(SshError::Key(_))
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "garbage");
    }
}
