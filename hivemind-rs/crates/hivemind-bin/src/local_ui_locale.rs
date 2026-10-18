use super::LocalUiOrigin;
use anyhow::{anyhow, Context, Result};
use hivemind_client_runtime::ClientRole;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Locale {
    En,
    Zh,
}

impl Locale {
    fn bytes(self) -> &'static [u8; 2] {
        match self {
            Self::En => b"en",
            Self::Zh => b"zh",
        }
    }
}

pub(super) struct LocaleStore {
    directory: Option<PathBuf>,
}

impl LocaleStore {
    pub(super) fn new(role: ClientRole, origin: LocalUiOrigin) -> Self {
        Self::from_root(
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            role,
            origin,
        )
    }

    fn from_root(root: Option<PathBuf>, role: ClientRole, origin: LocalUiOrigin) -> Self {
        Self {
            directory: root.filter(|root| root.is_absolute()).map(|root| {
                root.join("Hivemind")
                    .join("ui-preferences")
                    .join(role.as_str())
                    .join(origin.port.to_string())
            }),
        }
    }

    fn directory(&self) -> Result<&Path> {
        self.directory
            .as_deref()
            .ok_or_else(|| anyhow!("native locale preferences require an absolute LOCALAPPDATA"))
    }

    pub(super) fn read(&self) -> Result<Option<Locale>> {
        let path = self.directory()?.join("locale.v1");
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("could not open native locale preference"),
        };
        // One extra byte distinguishes an exact value from a truncated, oversized file.
        let mut bytes = Vec::with_capacity(3);
        file.take(3)
            .read_to_end(&mut bytes)
            .context("could not read native locale preference")?;
        match bytes.as_slice() {
            b"en" => Ok(Some(Locale::En)),
            b"zh" => Ok(Some(Locale::Zh)),
            _ => Err(anyhow!("invalid native locale preference")),
        }
    }

    pub(super) fn write(&self, locale: Locale) -> Result<()> {
        let directory = self.directory()?;
        fs::create_dir_all(directory).context("could not create native locale directory")?;
        let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .context("could not create temporary native locale preference")?;
        let written = file
            .write_all(locale.bytes())
            .and_then(|()| file.sync_all());
        drop(file);
        let result = written
            .context("could not flush native locale preference")
            .and_then(|()| {
                // Same-directory rename replaces the old file without an unlink gap.
                // This is not a guarantee against every filesystem/power-loss failure.
                fs::rename(&temporary, directory.join("locale.v1"))
                    .context("could not replace native locale preference")
            });
        if result.is_err() {
            if let Err(error) = fs::remove_file(&temporary) {
                eprintln!("Could not remove temporary native locale preference: {error}");
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "hivemind-locale-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn store(&self, role: ClientRole, port: u16) -> LocaleStore {
            LocaleStore::from_root(Some(self.0.clone()), role, LocalUiOrigin { port })
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn locale_allowlist_is_exact() {
        assert_eq!(
            serde_json::from_str::<Locale>("\"en\"").unwrap(),
            Locale::En
        );
        assert_eq!(
            serde_json::from_str::<Locale>("\"zh\"").unwrap(),
            Locale::Zh
        );
        for value in [
            "\"zh-TW\"",
            "\"EN\"",
            "\"en\\n\"",
            "\"\"",
            "null",
            "42",
            "{}",
        ] {
            assert!(serde_json::from_str::<Locale>(value).is_err(), "{value}");
        }
    }

    #[test]
    fn missing_preference_does_not_create_a_directory_or_default_file() {
        let root = TestRoot::new();
        let store = root.store(ClientRole::Master, 8080);
        assert_eq!(store.read().unwrap(), None);
        assert_eq!(fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn each_read_uses_disk_and_reconstructed_store_keeps_the_preference() {
        let root = TestRoot::new();
        let store = root.store(ClientRole::Master, 8080);
        store.write(Locale::Zh).unwrap();
        let reconstructed = root.store(ClientRole::Master, 8080);
        assert_eq!(reconstructed.read().unwrap(), Some(Locale::Zh));
        store.write(Locale::En).unwrap();
        assert_eq!(reconstructed.read().unwrap(), Some(Locale::En));
        let directory = store.directory().unwrap();
        assert_eq!(fs::read(directory.join("locale.v1")).unwrap(), b"en");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_short_and_oversized_preferences_are_errors_without_rewriting() {
        let root = TestRoot::new();
        let store = root.store(ClientRole::Master, 8080);
        store.write(Locale::En).unwrap();
        let path = store.directory().unwrap().join("locale.v1");
        for bytes in [b"".as_slice(), b"e", b"xx", b"zh\n", b"en\0", &[b'z'; 4096]] {
            fs::write(&path, bytes).unwrap();
            assert!(store.read().is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn read_io_failure_is_not_a_missing_preference() {
        let root = TestRoot::new();
        let store = root.store(ClientRole::Worker, 18080);
        store.write(Locale::En).unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(store.directory().unwrap().join("locale.v1"))
            .unwrap();
        assert!(store.read().is_err());
        drop(locked);
        assert_eq!(store.read().unwrap(), Some(Locale::En));
    }

    #[test]
    fn failed_atomic_replacement_preserves_old_file_and_cleans_own_temporary() {
        let root = TestRoot::new();
        let store = root.store(ClientRole::Master, 8080);
        store.write(Locale::En).unwrap();
        let directory = store.directory().unwrap();
        let other_temporary = directory.join("other-writer.tmp");
        fs::write(&other_temporary, b"other").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(directory.join("locale.v1"))
            .unwrap();
        assert!(store.write(Locale::Zh).is_err());
        drop(locked);
        assert_eq!(store.read().unwrap(), Some(Locale::En));
        assert_eq!(fs::read(&other_temporary).unwrap(), b"other");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 2);
    }

    #[test]
    fn role_and_port_are_isolated_under_the_fixed_namespace() {
        let root = TestRoot::new();
        let master = root.store(ClientRole::Master, 8080);
        let worker = root.store(ClientRole::Worker, 8080);
        let other_port = root.store(ClientRole::Master, 8081);
        master.write(Locale::Zh).unwrap();
        assert_eq!(worker.read().unwrap(), None);
        assert_eq!(other_port.read().unwrap(), None);
        worker.write(Locale::En).unwrap();
        other_port.write(Locale::En).unwrap();
        assert_eq!(master.read().unwrap(), Some(Locale::Zh));
        assert_eq!(
            master.directory().unwrap(),
            root.0.join("Hivemind/ui-preferences/master/8080")
        );
    }

    #[test]
    fn missing_or_relative_root_never_falls_back_to_working_directory() {
        let relative = format!(
            "hivemind-invalid-locale-root-{}",
            uuid::Uuid::new_v4().simple()
        );
        for root in [
            None,
            Some(PathBuf::new()),
            Some(PathBuf::from(&relative)),
            Some(PathBuf::from("C:relative")),
            Some(PathBuf::from("\\root-relative")),
        ] {
            let store =
                LocaleStore::from_root(root, ClientRole::Master, LocalUiOrigin { port: 8080 });
            assert!(store.read().is_err());
            assert!(store.write(Locale::Zh).is_err());
        }
        assert!(!Path::new(&relative).exists());
    }
}
