#![forbid(unsafe_code)]

use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use tg_factor_engine::default_registry;
use tg_factor_engine::grpc::FactorGrpcService;
use tg_factor_engine::storage::FactorValueStore;
use tg_persistence::repo::PostgresStore;
use tonic::transport::Server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL is required for tg-factor-engine"))?;
    let data_root = env::var("TG_DATA_ROOT").unwrap_or_else(|_| ".".to_owned());
    let listen_addr = env::var("TG_FACTOR_ENGINE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50055".to_owned())
        .parse::<SocketAddr>()?;

    let persistence = Arc::new(PostgresStore::connect(&database_url, &data_root).await?);
    let service = FactorGrpcService::new(
        default_registry(),
        persistence,
        FactorValueStore::new(&data_root),
    );

    Server::builder()
        .add_service(
            tg_contracts::proto::tg::v1::factor_service_server::FactorServiceServer::new(service),
        )
        .serve(listen_addr)
        .await?;
    Ok(())
}
