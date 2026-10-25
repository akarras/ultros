//! Independent plugin keys, persisted outside the image across Watchtower updates.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
use ultros_db::plugin::PluginKeys;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredKeys {
    version: u8,
    pairing: [u8; 32],
    identity: [u8; 32],
}

pub(super) fn deployment_keys(
    pairing: Option<&str>,
    identity: Option<&str>,
    file: Option<&Path>,
    origin: &str,
) -> Result<PluginKeys> {
    match (pairing, identity) {
        (Some(pairing), Some(identity)) => {
            let decode = |value: &str| -> Result<Vec<u8>> {
                ensure!(
                    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
                    "plugin deployment keys must contain 64 hexadecimal characters"
                );
                (0..64)
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(Into::into))
                    .collect()
            };
            PluginKeys::new(&decode(pairing)?, &decode(identity)?, origin)
                .map_err(|_| anyhow::anyhow!("invalid plugin deployment keys"))
        }
        (None, None) => load_or_create(
            file.filter(|path| !path.as_os_str().is_empty()).context(
                "configure both plugin deployment keys or a persistent ULTROS_PLUGIN_KEY_FILE",
            )?,
            origin,
        ),
        _ => anyhow::bail!(
            "configure both plugin deployment keys; refusing to replace one with file-backed keys"
        ),
    }
}

fn read_keys(path: &Path) -> Result<StoredKeys> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= 4096,
        "invalid plugin key file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "plugin key file must be accessible only to its owner (mode 0600)"
        );
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "invalid plugin key file");
    let keys: StoredKeys = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid plugin key file format"))?;
    ensure!(
        keys.version == 1
            && keys.pairing != [0; 32]
            && keys.identity != [0; 32]
            && keys.pairing != keys.identity,
        "invalid plugin key file values"
    );
    Ok(keys)
}

fn load_or_create(path: &Path, origin: &str) -> Result<PluginKeys> {
    let keys = match read_keys(path) {
        Ok(keys) => keys,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .context("plugin key file needs a persistent parent directory")?;
            fs::create_dir_all(parent).context("could not create plugin key directory")?;
            let mut keys = StoredKeys {
                version: 1,
                pairing: [0; 32],
                identity: [0; 32],
            };
            getrandom::fill(&mut keys.pairing)
                .map_err(|_| anyhow::anyhow!("plugin key entropy unavailable"))?;
            getrandom::fill(&mut keys.identity)
                .map_err(|_| anyhow::anyhow!("plugin key entropy unavailable"))?;
            // NamedTempFile creates owner-only files on Unix. Publishing without
            // clobbering means concurrent starts cannot rotate another replica's keys.
            let mut pending = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer(&mut pending, &keys)?;
            pending.as_file().sync_all()?;
            match pending.persist_noclobber(path) {
                Ok(_) => {
                    #[cfg(unix)]
                    fs::File::open(parent)?.sync_all()?;
                }
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => anyhow::bail!("could not atomically persist plugin keys"),
            }
            read_keys(path)?
        }
        // Never overwrite malformed, inaccessible or insecure files: changing
        // the identity key would orphan saved private retainer registrations.
        Err(error) => {
            return Err(
                error.context("could not load persistent plugin keys; preserve the existing file")
            );
        }
    };
    PluginKeys::new(&keys.pairing, &keys.identity, origin)
        .map_err(|_| anyhow::anyhow!("invalid plugin deployment keys"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_and_concurrent_starts_keep_the_same_private_identity() {
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("keys.json");
        let fingerprint = || {
            load_or_create(&file, "https://ultros.app")
                .unwrap()
                .private_fingerprint(12, "retainer", "1234")
                .unwrap()
        };
        let expected = fingerprint();
        let original = fs::read(&file).unwrap();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let expected = &expected;
                let fingerprint = &fingerprint;
                scope.spawn(move || assert_eq!(fingerprint(), *expected));
            }
        });
        assert_eq!(original, fs::read(&file).unwrap());
        let stored = read_keys(&file).unwrap();
        assert!(stored.pairing != stored.identity);
    }

    #[test]
    fn concurrent_first_starts_publish_one_complete_key_file() {
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("keys.json");
        let fingerprints = std::thread::scope(|scope| {
            (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        load_or_create(&file, "https://ultros.app")
                            .unwrap()
                            .private_fingerprint(12, "retainer", "1234")
                            .unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|task| task.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(fingerprints.windows(2).all(|pair| pair[0] == pair[1]));
        read_keys(&file).unwrap();
    }

    #[test]
    fn bad_files_and_partial_environment_keys_never_generate_replacements() {
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("keys.json");
        fs::write(&file, "invalid").unwrap();
        assert!(load_or_create(&file, "https://ultros.app").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"invalid");
        assert!(
            deployment_keys(
                Some(&"a".repeat(64)),
                None,
                Some(&file),
                "https://ultros.app"
            )
            .is_err()
        );
        assert_eq!(fs::read(&file).unwrap(), b"invalid");
    }

    #[test]
    fn malformed_key_values_are_not_in_error_messages() {
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("keys.json");
        load_or_create(&file, "https://ultros.app").unwrap();
        fs::write(
            &file,
            r#"{"version":1,"pairing":"do-not-log-this-key","identity":[]}"#,
        )
        .unwrap();
        let error = load_or_create(&file, "https://ultros.app").err().unwrap();
        assert!(!format!("{error:#}").contains("do-not-log-this-key"));
    }

    #[cfg(unix)]
    #[test]
    fn key_files_are_private_and_symlinks_or_broad_permissions_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let fixture = tempfile::tempdir().unwrap();
        let file = fixture.path().join("keys.json");
        load_or_create(&file, "https://ultros.app").unwrap();
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = fixture.path().join("link.json");
        symlink(&file, &link).unwrap();
        assert!(load_or_create(&link, "https://ultros.app").is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_or_create(&file, "https://ultros.app").is_err());
    }
}
