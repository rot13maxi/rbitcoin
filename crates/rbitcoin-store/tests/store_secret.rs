use rbitcoin_store::error::StoreError;
use rbitcoin_store::store_secret::*;

fn temp_dir() -> rbitcoin_store::testutil::TempDir {
    rbitcoin_store::testutil::TempDir::labeled("secret").unwrap()
}

#[test]
fn debug_does_not_print_secret_bytes() {
    let s = StoreSecret::from_bytes([0xab; SECRET_LEN]);
    let shown = format!("{s:?}");
    assert!(!shown.contains("abab"), "{shown}");
    assert!(shown.contains("redacted"), "{shown}");
}

#[test]
fn generate_write_load_roundtrip() {
    let dir = temp_dir();
    let s = StoreSecret::generate();
    assert!(s.as_bytes().iter().any(|&b| b != 0));
    s.write_to_store_dir(&dir).unwrap();
    let s2 = StoreSecret::load_from_store_dir(&dir).unwrap();
    assert_eq!(s, s2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn xor_is_involution_and_changes_plaintext() {
    let s = StoreSecret::from_bytes([0xA5; 32]);
    let plain = b"hello-script-pubkey-bytes!!".to_vec();
    let mut buf = plain.clone();
    s.xor_bytes(100, &mut buf);
    assert_ne!(buf, plain, "obfuscated must differ from plaintext");
    s.xor_bytes(100, &mut buf);
    assert_eq!(buf, plain);
}

#[test]
fn xor_bytes_matches_key_byte_stream() {
    let s = StoreSecret::from_bytes([0x3c; 32]);
    let lens = [0usize, 1, 7, 8, 9, 15, 16, 31, 32, 33, 64, 127, 256, 1000];
    let starts = [0u64, 1, 7, 8, 31, 32, 33, 100, 1_000_003];
    for &start in &starts {
        for &len in &lens {
            let mut got = vec![0x5a_u8; len];
            s.xor_bytes(start, &mut got);
            let mut want = vec![0x5a_u8; len];
            for (i, b) in want.iter_mut().enumerate() {
                *b ^= s.xor_key_byte(start.wrapping_add(i as u64));
            }
            assert_eq!(got, want, "start={start} len={len}");
        }
    }
}

#[test]
fn mix_txid_not_identity_and_stable() {
    let s = StoreSecret::from_bytes([7u8; 32]);
    let txid = [0x11u8; 32];
    let m = s.mix_txid(&txid);
    assert_ne!(m, txid, "mixed key must not equal raw txid");
    assert_eq!(m, s.mix_txid(&txid));
    let m2 = s.mix_txid(&[0x22; 32]);
    assert_ne!(m, m2);
}

#[test]
fn load_or_create_and_missing() {
    let dir = temp_dir();
    assert!(StoreSecret::load_existing(&dir).is_err());
    let s = StoreSecret::load_or_create(&dir).unwrap();
    let s2 = StoreSecret::load_existing(&dir).unwrap();
    assert_eq!(s, s2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_refuses_missing_store_secret() {
    let dir = temp_dir();
    {
        let store = rbitcoin_store::Store::create_tiny(&dir).unwrap();
        assert!(store.path().join(SECRET_FILE).is_file());
    }
    let path = dir.join(SECRET_FILE);
    std::fs::remove_file(&path).unwrap();

    match rbitcoin_store::Store::open_tiny(&dir) {
        Err(StoreError::Corrupt(msg)) => {
            assert!(msg.contains("store.secret missing"), "{msg}");
        }
        Ok(_) => panic!("Store::open regenerated a missing store.secret"),
        Err(other) => panic!("{other:?}"),
    }
    assert!(
        !path.exists(),
        "a failed open must not write a replacement secret"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_rejects_overlong_secret_file() {
    let dir = temp_dir();
    let path = dir.join(SECRET_FILE);
    // 33 bytes → corrupt (longer than 32).
    std::fs::write(&path, vec![0u8; 33]).unwrap();
    assert!(matches!(
        StoreSecret::load_from_store_dir(&dir),
        Err(StoreError::Corrupt(_))
    ));
    // generate forces first byte nonzero path when unlucky zeros — just call once more.
    let mut s = StoreSecret::generate();
    let mut b = *s.as_bytes();
    b[0] = 0;
    // from_bytes keeps zero first byte (generate would fix)
    s = StoreSecret::from_bytes(b);
    assert_eq!(s.as_bytes()[0], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The secret is published by a synced rename: the directory entry is
/// durable, and the file is owner-only from its first byte.
#[test]
fn write_publishes_an_owner_only_secret_through_a_synced_rename() {
    let dir = temp_dir();
    rbitcoin_store::file::arm_fsync_dir_trace();
    StoreSecret::generate()
        .write_to_store_dir(dir.path())
        .unwrap();
    let synced = rbitcoin_store::file::take_fsynced_dirs();
    assert!(
        synced.iter().any(|d| d == dir.path()),
        "store.secret name not synced: {synced:?}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("store.secret"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(
        StoreSecret::generate()
            .write_to_store_dir(dir.path())
            .is_err(),
        "an existing secret is never replaced"
    );
}

/// A private temp file is owner-only from the moment it is opened, so no
/// other local user can hold a readable fd on it. A stale world-readable
/// temp from an earlier crash is replaced, not reused.
#[cfg(unix)]
#[test]
fn private_temp_file_is_owner_only_when_opened() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let path = dir.path().join("store.secret");
    let tmp = rbitcoin_store::file::tmp_sidecar_path(&path);
    std::fs::write(&tmp, b"stale").unwrap();
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();
    rbitcoin_store::file::write_private_synced_tmp_file(&path, |f| {
        let mode = f.metadata().unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "temp opened readable by others");
        f.write_all(b"secret").unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"secret");
}

/// Store `meta` is the create commit. Its name is synced like the rewrite.
#[test]
fn store_meta_is_published_through_a_synced_rename() {
    let dir = temp_dir();
    rbitcoin_store::file::arm_fsync_dir_trace();
    rbitcoin_store::store::write_meta(dir.path()).unwrap();
    let synced = rbitcoin_store::file::take_fsynced_dirs();
    assert!(
        synced.iter().any(|d| d == dir.path()),
        "store meta name not synced: {synced:?}"
    );
    assert_eq!(std::fs::metadata(dir.path().join("meta")).unwrap().len(), 6);
}
