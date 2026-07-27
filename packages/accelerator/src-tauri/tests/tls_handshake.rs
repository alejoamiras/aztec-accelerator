//! Tier-1 certainty test: an IN-PROCESS TLS handshake against the real generated cert set.
//!
//! This is the cheapest independent proof that HTTPS actually works — no OS trust store, no browser,
//! runs in a normal `cargo test` on every OS. It generates the real keyless-CA + leaf via the public
//! `certs` API, serves it with the SAME `load_rustls_config` the app uses, and connects a rustls
//! CLIENT that trusts ONLY our `ca.pem`. A successful handshake proves, together:
//!   - the leaf + key are a matching pair (server side loads them),
//!   - the leaf actually chains to the CA (client verifies against our anchor),
//!   - the CA's loopback NAME CONSTRAINTS permit a `localhost` / `127.0.0.1` leaf (webpki enforces
//!     name constraints during verification — a misconfigured constraint would reject the leaf here),
//!   - rustls end-to-end accepts the whole set exactly as a real TLS client (e.g. a browser) would.
//!
//! A regression in cert generation (bad SANs, wrong key usage, broken constraints, mismatched key)
//! fails THIS test on Linux CI in milliseconds, long before the manual browser smoke on macOS/Windows.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio_rustls::{TlsAcceptor, TlsConnector};

/// One full handshake to a fresh loopback listener, verifying the server cert as `name`. The
/// handshake itself is the assertion (it fails if the leaf doesn't verify against our CA).
async fn handshake(
    server_config: Arc<ServerConfig>,
    client_config: Arc<ClientConfig>,
    name: &'static str,
) {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    let acceptor = TlsAcceptor::from(server_config);
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept tcp");
        let mut tls = acceptor.accept(stream).await.expect("server TLS accept");
        let mut buf = [0u8; 1];
        tls.read_exact(&mut buf).await.expect("server read");
        tls.write_all(&buf).await.expect("server write");
        tls.flush().await.expect("server flush");
    });

    let connector = TlsConnector::from(client_config);
    let tcp = TcpStream::connect(addr).await.expect("connect tcp");
    let domain = ServerName::try_from(name).expect("server name");
    let mut tls = connector.connect(domain, tcp).await.unwrap_or_else(|e| {
        panic!("client TLS handshake to '{name}' must succeed against our CA: {e}")
    });

    tls.write_all(b"x").await.expect("client write");
    let mut buf = [0u8; 1];
    tls.read_exact(&mut buf).await.expect("client read");
    assert_eq!(&buf, b"x", "round-trip byte over the TLS channel ({name})");

    server.await.expect("server task");
}

#[tokio::test]
async fn generated_certs_complete_a_real_tls_handshake_to_loopback() {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Generate the real cert set into a throwaway HOME (both HOME and USERPROFILE so certs_dir()
    // resolves on every OS). Single test in this file → no intra-process env race.
    let home = tempfile::tempdir().expect("temp HOME");
    std::env::set_var("HOME", home.path());
    std::env::set_var("USERPROFILE", home.path());

    aztec_accelerator::certs::generate_and_save().expect("generate certs");
    let server_config =
        aztec_accelerator::certs::load_rustls_config().expect("load server TLS config");

    // The CLIENT trusts ONLY our CA anchor — nothing from the OS store.
    let ca_pem = std::fs::read(aztec_accelerator::certs::live_ca_cert_path()).expect("read ca.pem");
    let mut roots = RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut &ca_pem[..]) {
        roots
            .add(cert.expect("parse ca cert"))
            .expect("add ca to root store");
    }
    let client_config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    // Verify by DNS `localhost` AND by the loopback IP (a browser hitting https://127.0.0.1 verifies
    // by IP) — both must pass under the CA's DnsName + IpAddress name constraints.
    handshake(server_config.clone(), client_config.clone(), "localhost").await;
    handshake(server_config, client_config, "127.0.0.1").await;
}
