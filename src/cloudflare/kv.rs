use worker::*;

pub async fn get_nwc_uri(env: &Env, username: &str) -> Result<Option<String>> {
    let kv = env.kv("MEKHALA_NWC_KV")?;
    kv.get(username).text().await.map_err(|e| Error::from(e.to_string()))
}
