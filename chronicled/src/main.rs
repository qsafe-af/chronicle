use anyhow::Result;
use subxt::{backend::rpc::RpcClient, OnlineClient};
use tokio_postgres::NoTls;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // env: WS_URL, PG_DSN, FINALITY_CONFIRMATIONS (optional)
    let ws = std::env::var("WS_URL").unwrap_or_else(|_| "wss://a.t.res.fm".into());
    let pg_dsn = std::env::var("PG_DSN")
        .unwrap_or_else(|_| "postgres://res:change-me@127.0.0.1:5432/res_index".into());

    // connect to chain
    let client = OnlineClient::from_rpc_client(RpcClient::from_url(&ws).await?).await?;
    let genesis = client.rpc().genesis_hash();
    let chain_id = bs58::encode(genesis.as_bytes()).into_string();
    info!(%chain_id, "connected; computed base58 chain-id from genesis");

    // connect to Postgres
    let (pg, conn) = tokio_postgres::connect(&pg_dsn, NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("postgres connection error: {e}");
        }
    });

    // ensure schema exists (named exactly the base58 chain-id)
    let schema_name = format!("\"{}\"", chain_id); // quote identifier safely
    pg.execute(&*format!("CREATE SCHEMA IF NOT EXISTS {schema_name};"), &[]).await?;
    // create tables (simplified)
    pg.batch_execute(&*format!(r#"
        CREATE TABLE IF NOT EXISTS {schema}.blocks(
          number BIGINT PRIMARY KEY,
          hash BYTEA NOT NULL,
          parent_hash BYTEA NOT NULL,
          timestamp TIMESTAMPTZ NOT NULL,
          is_canonical BOOLEAN NOT NULL,
          runtime_spec BIGINT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS {schema}.balance_changes(
          id BIGSERIAL PRIMARY KEY,
          account BYTEA NOT NULL,
          block_number BIGINT NOT NULL,
          event_index INT NOT NULL,
          delta NUMERIC(78,0) NOT NULL,
          reason TEXT NOT NULL,
          extrinsic_hash BYTEA,
          event_pallet TEXT NOT NULL,
          event_variant TEXT NOT NULL,
          block_ts TIMESTAMPTZ NOT NULL
        );
    "#, schema = schema_name)).await?;

    // TODO: turn balance_changes into a Timescale hypertable (once extension enabled)
    // pg.batch_execute(&*format!("SELECT create_hypertable('{schema}.balance_changes', by_range('block_ts'), if_not_exists => true);", schema = chain_id)).await?;

    // ingest loop placeholder
    let mut sub = client.blocks().subscribe_finalized().await?;
    while let Some(block) = sub.next().await {
        match block {
            Ok(b) => {
                let num = b.number();
                info!(%num, "finalized block");
                // decode events -> per-account signed deltas -> INSERT into {schema}.balance_changes
            }
            Err(e) => warn!("block stream error: {e}"),
        }
    }

    Ok(())
}
