mod support;

use support::crypto_fixtures;
use uselesskey::negative::CorruptPem;

#[test]
fn deterministic_rsa_fixtures_are_stable() {
    let first = crypto_fixtures::rsa_private_key_pem("abp-uselesskey-tests", "policy-key");
    let second = crypto_fixtures::rsa_private_key_pem("abp-uselesskey-tests", "policy-key");
    let different = crypto_fixtures::rsa_private_key_pem("abp-uselesskey-tests", "workspace-key");

    assert_eq!(first, second);
    assert_ne!(first, different);
    assert!(first.contains("-----BEGIN PRIVATE KEY-----"));
}

#[test]
fn x509_and_negative_fixtures_are_available() {
    let corrupted = crypto_fixtures::rsa_private_key_pem_corrupt(
        "abp-uselesskey-tests",
        "negative-key",
        CorruptPem::BadHeader,
    );
    let chain =
        crypto_fixtures::x509_chain_pems("abp-uselesskey-tests", "sidecar-chain", "sidecar.local");

    assert!(corrupted.contains("-----BEGIN CORRUPTED KEY-----"));
    assert!(chain.leaf_pem.contains("-----BEGIN CERTIFICATE-----"));
    assert!(chain.root_pem.contains("-----BEGIN CERTIFICATE-----"));
    assert!(
        chain
            .chain_pem
            .matches("-----BEGIN CERTIFICATE-----")
            .count()
            >= 2
    );
}
