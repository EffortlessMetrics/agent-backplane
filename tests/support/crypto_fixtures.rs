use uselesskey::{
    ChainSpec, Factory, RsaFactoryExt, RsaSpec, Seed, X509FactoryExt, negative::CorruptPem,
};

pub struct ChainPem {
    pub chain_pem: String,
    pub leaf_pem: String,
    pub root_pem: String,
}

fn factory(seed_value: &str) -> Factory {
    let seed = Seed::from_env_value(seed_value).expect("valid uselesskey seed");
    Factory::deterministic(seed)
}

pub fn rsa_private_key_pem(seed_value: &str, label: &str) -> String {
    factory(seed_value)
        .rsa(label, RsaSpec::rs256())
        .private_key_pkcs8_pem()
        .to_string()
}

pub fn rsa_private_key_pem_corrupt(seed_value: &str, label: &str, mode: CorruptPem) -> String {
    factory(seed_value)
        .rsa(label, RsaSpec::rs256())
        .private_key_pkcs8_pem_corrupt(mode)
        .to_string()
}

pub fn x509_chain_pems(seed_value: &str, label: &str, hostname: &str) -> ChainPem {
    let chain = factory(seed_value).x509_chain(label, ChainSpec::new(hostname));

    ChainPem {
        chain_pem: chain.chain_pem().to_string(),
        leaf_pem: chain.leaf_cert_pem().to_string(),
        root_pem: chain.root_cert_pem().to_string(),
    }
}
