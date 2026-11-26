//! Local HTTPS (PRD §7): a Stackyard certificate authority, trusted by the OS
//! once, signs one certificate covering every project domain.
//!
//! The CA is name-constrained to `.test`, `localhost` and loopback addresses:
//! even if its key leaks, it cannot vouch for any other site.

use std::path::{Path, PathBuf};

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose, NameConstraints, SanType, SerialNumber,
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::admin::{self, EXIT_INVALID, EXIT_IO};
use crate::error::{AppError, AppResult};
use crate::paths::{write_atomic, Layout};
use crate::projects::is_valid_domain;
use crate::registry::sha256_hex;

pub const HELPER_FLAG: &str = "--trust-ca";
const CA_NAME: &str = "Stackyard Local CA";
const CA_CERT: &str = "stackyard-ca.pem";
const CA_KEY: &str = "stackyard-ca-key.pem";
/// Holds the SHA-256 of the CA certificate the OS was told to trust.
const TRUSTED_MARKER: &str = "stackyard-ca.trusted";
const SITE_CERT: &str = "projects.pem";
const SITE_KEY: &str = "projects-key.pem";
const SITE_META: &str = "projects.json";
/// File name of the CA inside the system trust store.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const STORE_NAME: &str = "stackyard-local-ca.crt";
const CA_DAYS: i64 = 3650;
/// Within the 398-day limit browsers apply to server certificates.
const SITE_DAYS: i64 = 397;
const RENEW_DAYS: i64 = 30;
const MAX_PEM: u64 = 16 * 1024;

fn tls_err(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(format!("certificate: {e}"))
}

pub fn ca_cert_path(layout: &Layout) -> PathBuf {
    layout.certs().join(CA_CERT)
}

/// HTTPS is on once the CA exists (created when the user enables it).
pub fn enabled(layout: &Layout) -> bool {
    ca_cert_path(layout).is_file() && layout.certs().join(CA_KEY).is_file()
}

/// The subject and constraints of the CA. Also the issuer of site
/// certificates (only its name, key usages and key identifier are used).
fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, CA_NAME);
    dn.push(DnType::OrganizationName, "Stackyard");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::DnsName(crate::projects::TLD.into()),
            GeneralSubtree::DnsName("localhost".into()),
            GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([127, 0, 0, 0], 8)),
            GeneralSubtree::IpAddress(CidrSubnet::from_v6_prefix(std::net::Ipv6Addr::LOCALHOST.octets(), 128)),
        ],
        excluded_subtrees: Vec::new(),
    });
    p
}

/// A random positive serial: browsers reject two certificates from one
/// issuer sharing a serial.
fn serial() -> AppResult<SerialNumber> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(tls_err)?;
    b[0] &= 0x7f;
    Ok(SerialNumber::from_slice(&b))
}

/// Private keys are readable by the user only.
fn write_private(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = path.with_extension("tmp-write");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    std::io::Write::write_all(&mut opts.open(&tmp)?, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Creates the CA when missing; returns its certificate file.
pub fn ensure_ca(layout: &Layout) -> AppResult<PathBuf> {
    let cert_path = ca_cert_path(layout);
    if enabled(layout) {
        return Ok(cert_path);
    }
    std::fs::create_dir_all(layout.certs())?;
    let key = KeyPair::generate().map_err(tls_err)?;
    let mut params = ca_params();
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(CA_DAYS);
    params.serial_number = Some(serial()?);
    let cert = params.self_signed(&key).map_err(tls_err)?;
    write_private(&layout.certs().join(CA_KEY), key.serialize_pem().as_bytes())?;
    write_atomic(&cert_path, cert.pem().as_bytes())?;
    Ok(cert_path)
}

fn ca_fingerprint(layout: &Layout) -> Option<String> {
    std::fs::read(ca_cert_path(layout)).ok().map(|b| sha256_hex(&b))
}

/// The current CA was installed in the OS trust store.
pub fn is_trusted(layout: &Layout) -> bool {
    let marker = std::fs::read_to_string(layout.certs().join(TRUSTED_MARKER)).unwrap_or_default();
    ca_fingerprint(layout).is_some_and(|f| marker.trim() == f)
}

fn mark_trusted(layout: &Layout) -> AppResult<()> {
    let f = ca_fingerprint(layout).ok_or_else(|| AppError::NotFound("CA".into()))?;
    write_atomic(&layout.certs().join(TRUSTED_MARKER), f.as_bytes())
}

pub struct SiteCert {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Serialize, Deserialize, PartialEq)]
struct SiteMeta {
    domains: Vec<String>,
    /// Unix seconds.
    not_after: i64,
    /// Fingerprint of the CA that signed it.
    ca: String,
}

/// The certificate for `domains` (plus `localhost`), issued again when the
/// domains or the CA change or it nears expiry. `None` while HTTPS is off.
pub fn site_cert(layout: &Layout, domains: &[String]) -> AppResult<Option<SiteCert>> {
    if !enabled(layout) {
        return Ok(None);
    }
    let dir = layout.certs();
    let files = SiteCert { cert: dir.join(SITE_CERT), key: dir.join(SITE_KEY) };
    let mut wanted: Vec<String> = domains.iter().filter(|d| is_valid_domain(d)).cloned().collect();
    wanted.sort();
    wanted.dedup();
    let ca_pem = std::fs::read_to_string(ca_cert_path(layout))?;
    let ca = sha256_hex(ca_pem.as_bytes());
    let now = OffsetDateTime::now_utc();
    let current: Option<SiteMeta> = std::fs::read(dir.join(SITE_META)).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let fresh = current.is_some_and(|m| {
        m.domains == wanted && m.ca == ca && m.not_after - now.unix_timestamp() > RENEW_DAYS * 86_400
    });
    if fresh && files.cert.is_file() && files.key.is_file() {
        return Ok(Some(files));
    }

    let ca_key = KeyPair::from_pem(&std::fs::read_to_string(dir.join(CA_KEY))?).map_err(tls_err)?;
    let issuer = Issuer::new(ca_params(), ca_key);
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, wanted.first().map_or("localhost", String::as_str));
    params.distinguished_name = dn;
    params.subject_alt_names = std::iter::once("localhost".to_string())
        .chain(wanted.iter().cloned())
        .map(|d| d.try_into().map(SanType::DnsName))
        .collect::<Result<_, _>>()
        .map_err(tls_err)?;
    params.subject_alt_names.push(SanType::IpAddress(std::net::Ipv4Addr::LOCALHOST.into()));
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(SITE_DAYS);
    params.serial_number = Some(serial()?);
    let key = KeyPair::generate().map_err(tls_err)?;
    let cert = params.signed_by(&key, &issuer).map_err(tls_err)?;
    write_private(&files.key, key.serialize_pem().as_bytes())?;
    write_atomic(&files.cert, cert.pem().as_bytes())?;
    let meta = SiteMeta { domains: wanted, not_after: params.not_after.unix_timestamp(), ca };
    write_atomic(&dir.join(SITE_META), &serde_json::to_vec_pretty(&meta)?)?;
    Ok(Some(files))
}

// ---------- trusting the CA ----------

/// Exactly one PEM certificate and nothing else (never a key).
fn is_single_cert_pem(pem: &str) -> bool {
    let t = pem.trim();
    t.starts_with("-----BEGIN CERTIFICATE-----")
        && t.ends_with("-----END CERTIFICATE-----")
        && t.matches("-----BEGIN ").count() == 1
        && t.matches("-----END ").count() == 1
}

/// Elevated helper job: `stackyard --trust-ca <file.pem>`. The certificate is
/// copied (after validation) to Stackyard's own fixed name in the store.
pub fn trust_helper(args: &[String]) -> i32 {
    use std::io::Read;
    let [path] = args else { return EXIT_INVALID };
    let mut pem = String::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(MAX_PEM).read_to_string(&mut pem));
    if read.is_err() || !is_single_cert_pem(&pem) {
        eprintln!("stackyard: {path}: not a single PEM certificate");
        return EXIT_INVALID;
    }
    match install_in_store(pem.trim()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("stackyard: trust store: {e}");
            EXIT_IO
        }
    }
}

fn run_tool(program: &Path, args: &[&std::ffi::OsStr]) -> std::io::Result<()> {
    let out = std::process::Command::new(program).args(args).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{} failed: {}",
            program.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// A private copy of the validated PEM that the user cannot swap underneath
/// the store tool (root's or the admin's own temp folder).
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn private_copy(pem: &str) -> std::io::Result<(PathBuf, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("stackyard-ca-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir)?;
    let file = dir.join(STORE_NAME);
    std::fs::write(&file, format!("{pem}\n"))?;
    Ok((dir, file))
}

#[cfg(target_os = "linux")]
fn install_in_store(pem: &str) -> std::io::Result<()> {
    // (anchor folder, refresh command): Debian/Ubuntu, Fedora/RHEL, Arch, openSUSE.
    const STORES: [(&str, &str, &[&str]); 4] = [
        ("/usr/local/share/ca-certificates", "update-ca-certificates", &[]),
        ("/etc/pki/ca-trust/source/anchors", "update-ca-trust", &["extract"]),
        ("/etc/ca-certificates/trust-source/anchors", "trust", &["extract-compat"]),
        ("/usr/share/pki/trust/anchors", "update-ca-certificates", &[]),
    ];
    let find = |tool: &str| {
        ["/usr/sbin", "/usr/bin", "/sbin", "/bin"].iter().map(|d| Path::new(d).join(tool)).find(|p| p.is_file())
    };
    let (dir, tool, args) = STORES
        .iter()
        .find(|(dir, tool, _)| Path::new(dir).is_dir() && find(tool).is_some())
        .ok_or_else(|| std::io::Error::other("no supported system trust store found"))?;
    std::fs::write(Path::new(dir).join(STORE_NAME), format!("{pem}\n"))?;
    let args: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_ref()).collect();
    run_tool(&find(tool).unwrap_or_default(), &args)
}

#[cfg(target_os = "macos")]
fn install_in_store(pem: &str) -> std::io::Result<()> {
    let (dir, file) = private_copy(pem)?;
    let result = run_tool(
        Path::new("/usr/bin/security"),
        &["add-trusted-cert", "-d", "-r", "trustRoot", "-k", "/Library/Keychains/System.keychain"]
            .map(std::ffi::OsStr::new)
            .into_iter()
            .chain([file.as_os_str()])
            .collect::<Vec<_>>(),
    );
    let _ = std::fs::remove_dir_all(dir);
    result
}

#[cfg(windows)]
fn install_in_store(pem: &str) -> std::io::Result<()> {
    let (dir, file) = private_copy(pem)?;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let certutil = PathBuf::from(root).join("System32").join("certutil.exe");
    let result = run_tool(
        &certutil,
        &["-addstore", "-f", "Root"].map(std::ffi::OsStr::new).into_iter().chain([file.as_os_str()]).collect::<Vec<_>>(),
    );
    let _ = std::fs::remove_dir_all(dir);
    result
}

/// Outcome for browsers with their own certificate database (Linux).
#[derive(Debug, Default, Serialize)]
pub struct BrowserTrust {
    /// Chrome/Firefox profiles found that keep their own certificates.
    pub databases: usize,
    pub imported: usize,
    /// Profiles exist but NSS `certutil` (libnss3-tools / nss-tools) is missing.
    pub tool_missing: bool,
}

/// NSS databases of Chromium-based browsers and Firefox profiles (Linux;
/// elsewhere browsers use the OS store).
fn nss_databases(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in [".pki/nssdb", "snap/chromium/current/.pki/nssdb"] {
        let db = home.join(d);
        if db.join("cert9.db").is_file() {
            out.push(db);
        }
    }
    for base in [
        ".mozilla/firefox",
        "snap/firefox/common/.mozilla/firefox",
        ".var/app/org.mozilla.firefox/.mozilla/firefox",
    ] {
        let Ok(entries) = std::fs::read_dir(home.join(base)) else { continue };
        let mut profiles: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.join("cert9.db").is_file()).collect();
        profiles.sort();
        out.extend(profiles);
    }
    out
}

fn find_in_path(tool: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).chain([PathBuf::from("/usr/bin")]).map(|d| d.join(tool)).find(|p| p.is_file())
}

/// Adds the CA to the user's browser databases (no admin rights needed).
fn trust_in_browsers(ca: &Path) -> BrowserTrust {
    if !cfg!(target_os = "linux") {
        return BrowserTrust::default();
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return BrowserTrust::default() };
    let dbs = nss_databases(&home);
    let mut out = BrowserTrust { databases: dbs.len(), ..Default::default() };
    let Some(certutil) = find_in_path("certutil") else {
        out.tool_missing = !dbs.is_empty();
        return out;
    };
    for db in dbs {
        let status = std::process::Command::new(&certutil)
            .arg("-d")
            .arg(format!("sql:{}", db.display()))
            .args(["-A", "-t", "C,,", "-n", CA_NAME, "-i"])
            .arg(ca)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if status.is_ok_and(|s| s.success()) {
            out.imported += 1;
        }
    }
    out
}

/// Creates the CA when needed and asks the OS to trust it (one admin prompt;
/// skipped when already trusted), then adds it to the browsers that keep
/// their own certificates.
pub async fn enable(layout: &Layout) -> AppResult<BrowserTrust> {
    let ca = ensure_ca(layout)?;
    if !is_trusted(layout) {
        let args = [HELPER_FLAG.to_string(), ca.to_string_lossy().into_owned()];
        admin::run_elevated(&args, "Stackyard needs to trust its local certificate authority for HTTPS on .test domains.")
            .await?;
        mark_trusted(layout)?;
    }
    let ca2 = ca.clone();
    tokio::task::spawn_blocking(move || trust_in_browsers(&ca2)).await.map_err(tls_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_certificate_follows_domains() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::new(tmp.path().to_path_buf());
        let domains = vec!["toko.test".to_string(), "blog.test".to_string(), "evil.com".to_string()];
        assert!(site_cert(&layout, &domains).unwrap().is_none(), "off until enabled");

        let ca = ensure_ca(&layout).unwrap();
        assert!(enabled(&layout) && !is_trusted(&layout));
        assert_eq!(ensure_ca(&layout).unwrap(), ca, "kept");
        let first = site_cert(&layout, &domains).unwrap().unwrap();
        let pem = std::fs::read_to_string(&first.cert).unwrap();
        let meta: SiteMeta = serde_json::from_slice(&std::fs::read(layout.certs().join(SITE_META)).unwrap()).unwrap();
        assert_eq!(meta.domains, ["blog.test", "toko.test"], "sorted, invalid names dropped");

        site_cert(&layout, &["toko.test".into(), "blog.test".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(&first.cert).unwrap(), pem, "same domains: not reissued");
        site_cert(&layout, &["toko.test".into()]).unwrap();
        assert_ne!(std::fs::read_to_string(&first.cert).unwrap(), pem, "domains changed: reissued");

        mark_trusted(&layout).unwrap();
        assert!(is_trusted(&layout));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for key in [CA_KEY, SITE_KEY] {
                let mode = std::fs::metadata(layout.certs().join(key)).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "{key} private");
            }
        }
    }

    #[test]
    fn helper_accepts_only_one_certificate() {
        let cert = "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n";
        assert!(is_single_cert_pem(cert));
        assert!(!is_single_cert_pem(&format!("{cert}{cert}")));
        assert!(!is_single_cert_pem("-----BEGIN PRIVATE KEY-----\nAA\n-----END PRIVATE KEY-----"));
        assert!(!is_single_cert_pem(&format!("{cert}-----BEGIN PRIVATE KEY-----\nAA\n-----END PRIVATE KEY-----")));
        let tmp = tempfile::tempdir().unwrap();
        let key = tmp.path().join("k.pem");
        std::fs::write(&key, "-----BEGIN PRIVATE KEY-----\nAA\n-----END PRIVATE KEY-----\n").unwrap();
        assert_eq!(trust_helper(&[key.display().to_string()]), EXIT_INVALID);
        assert_eq!(trust_helper(&[]), EXIT_INVALID);
    }

    #[test]
    fn finds_browser_databases() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        for d in [".pki/nssdb", ".mozilla/firefox/abc.default-release", ".mozilla/firefox/empty"] {
            std::fs::create_dir_all(h.join(d)).unwrap();
        }
        std::fs::write(h.join(".pki/nssdb/cert9.db"), "").unwrap();
        std::fs::write(h.join(".mozilla/firefox/abc.default-release/cert9.db"), "").unwrap();
        let dbs = nss_databases(h);
        assert_eq!(dbs, [h.join(".pki/nssdb"), h.join(".mozilla/firefox/abc.default-release")]);
    }
}
