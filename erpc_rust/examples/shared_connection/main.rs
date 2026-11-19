//! Run two generated service clients in separate tasks on one connection.
//! Author: modolet <y@xxyx.io>, 2026-11-19.

mod services;

use async_trait::async_trait;
use erpc_rust::{
    codec::BasicCodecFactory, transport::MemoryTransport, Connection, Server, SimpleServer,
};
use services::{
    motor_server::{Motor, MotorClient, MotorServer},
    sensor_server::{Sensor, SensorClient, SensorServer},
};
use std::{error::Error, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

struct Thermometer;

#[async_trait]
impl Sensor for Thermometer {
    async fn read_temperature(&self) -> Result<i32> {
        Ok(24)
    }
}

struct Drive;

#[async_trait]
impl Motor for Drive {
    async fn set_speed(&self, rpm: u32) -> Result<u32> {
        Ok(rpm.min(1800))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (transport, peer) = MemoryTransport::pair();
    let mut server = SimpleServer::new(peer, BasicCodecFactory::new());
    server
        .add_service(Arc::new(SensorServer::new(Thermometer)))
        .await?;
    server
        .add_service(Arc::new(MotorServer::new(Drive)))
        .await?;
    let server_task = tokio::spawn(async move { server.run().await });

    let connection = Connection::new(transport, BasicCodecFactory::new());
    let sensor = SensorClient::new(&connection);
    let motor = MotorClient::new(&connection);
    let temperature = tokio::spawn(async move { sensor.read_temperature().await });
    let speed = tokio::spawn(async move { motor.set_speed(2400).await });
    let (temperature, speed) = tokio::try_join!(temperature, speed)?;
    let (temperature, speed) = (temperature?, speed?);
    assert_eq!((temperature, speed), (24, 1800));
    println!("Temperature: {temperature} °C, motor speed: {speed} rpm");

    connection.close().await?;
    server_task.abort();
    Ok(())
}
